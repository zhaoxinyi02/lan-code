use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use lan_core::{AgentCore, LanConfig, ModelProvider, SqliteStore};
use lan_protocol::{
    ApprovalDecision, ApprovalMode, ClientInfo, RpcNotification, RpcRequest, RpcResponse,
    SessionId, ToolCall,
};
use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    client: ClientInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateSessionParams {
    cwd: String,
    title: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CallToolParams {
    session_id: SessionId,
    call: ToolCall,
    mode: ApprovalMode,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartTurnParams {
    session_id: SessionId,
    prompt: String,
    mode: ApprovalMode,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionParams {
    session_id: SessionId,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveApprovalParams {
    request_id: uuid::Uuid,
    decision: ApprovalDecision,
}

#[tokio::main]
async fn main() {
    let config = LanConfig::load().expect("valid Lan Code configuration");
    let provider = config
        .provider()
        .expect("valid provider configuration")
        .map(|provider| Arc::new(provider) as Arc<dyn ModelProvider>);
    let store = config
        .database()
        .map(SqliteStore::open)
        .transpose()
        .expect("valid configured database");
    let core = Arc::new(match (provider, store) {
        (Some(provider), Some(store)) => {
            AgentCore::with_provider_and_store(provider, store).expect("load persistent sessions")
        }
        (Some(provider), None) => AgentCore::with_provider(provider),
        (None, Some(store)) => AgentCore::with_store(store).expect("load persistent sessions"),
        (None, None) => AgentCore::new(),
    });
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut requests = tokio::task::JoinSet::new();
    let initialized = Arc::new(AtomicBool::new(false));
    let (responses, mut response_rx) = tokio::sync::mpsc::channel::<serde_json::Value>(128);
    let mut events = core.subscribe();
    let event_responses = responses.clone();
    let event_forwarder = tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            let notification = RpcNotification {
                method: "event".into(),
                params: serde_json::to_value(event).expect("event is serializable"),
            };
            if event_responses
                .send(serde_json::to_value(notification).expect("notification is serializable"))
                .await
                .is_err()
            {
                break;
            }
        }
    });
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(response) = response_rx.recv().await {
            let mut bytes = serde_json::to_vec(&response).expect("response is serializable");
            bytes.push(b'\n');
            if stdout.write_all(&bytes).await.is_err() {
                break;
            }
            let _ = stdout.flush().await;
        }
    });

    while let Ok(Some(line)) = lines.next_line().await {
        match serde_json::from_str::<RpcRequest>(&line) {
            Ok(request) => {
                if request.method == "initialize" {
                    let response = handle(core.clone(), request, initialized.clone()).await;
                    let _ = responses
                        .send(serde_json::to_value(response).expect("response is serializable"))
                        .await;
                    continue;
                }
                let core = core.clone();
                let initialized = initialized.clone();
                let responses = responses.clone();
                requests.spawn(async move {
                    let response = handle(core, request, initialized).await;
                    let _ = responses
                        .send(serde_json::to_value(response).expect("response is serializable"))
                        .await;
                });
            }
            Err(error) => {
                let _ = responses
                    .send(
                        serde_json::to_value(RpcResponse::error(
                            String::new(),
                            -32700,
                            error.to_string(),
                        ))
                        .expect("response is serializable"),
                    )
                    .await;
            }
        }
    }
    while requests.join_next().await.is_some() {}
    tokio::task::yield_now().await;
    event_forwarder.abort();
    drop(responses);
    let _ = writer.await;
}

async fn handle(
    core: Arc<AgentCore>,
    request: RpcRequest,
    initialized: Arc<AtomicBool>,
) -> RpcResponse {
    if request.method != "initialize" && !initialized.load(Ordering::Acquire) {
        return RpcResponse::error(request.id, -32002, "client must initialize first");
    }
    match request.method.as_str() {
        "initialize" => match serde_json::from_value::<InitializeParams>(request.params) {
            Ok(params) => {
                initialized.store(true, Ordering::Release);
                RpcResponse::success(
                    request.id,
                    json!({
                        "server": {"name": "lan-daemon", "version": env!("CARGO_PKG_VERSION")},
                        "client": params.client,
                        "protocolVersion": 1
                    }),
                )
            }
            Err(error) => RpcResponse::error(request.id, -32602, error.to_string()),
        },
        "session/create" => match serde_json::from_value::<CreateSessionParams>(request.params) {
            Ok(params) => RpcResponse::success(
                request.id,
                core.create_session(params.cwd, params.title).await,
            ),
            Err(error) => RpcResponse::error(request.id, -32602, error.to_string()),
        },
        "session/list" => RpcResponse::success(request.id, core.list_sessions().await),
        "tool/list" => RpcResponse::success(request.id, core.list_tools()),
        "turn/start" => match serde_json::from_value::<StartTurnParams>(request.params) {
            Ok(params) => match core
                .start_turn(params.session_id, params.prompt, params.mode)
                .await
            {
                Ok(result) => RpcResponse::success(request.id, result),
                Err(error) => RpcResponse::error(request.id, -32020, error.to_string()),
            },
            Err(error) => RpcResponse::error(request.id, -32602, error.to_string()),
        },
        "turn/interrupt" => match serde_json::from_value::<SessionParams>(request.params) {
            Ok(params) => RpcResponse::success(
                request.id,
                json!({"interrupted": core.interrupt_turn(params.session_id).await}),
            ),
            Err(error) => RpcResponse::error(request.id, -32602, error.to_string()),
        },
        "approval/list" => RpcResponse::success(request.id, core.pending_approvals().await),
        "approval/resolve" => match serde_json::from_value::<ResolveApprovalParams>(request.params)
        {
            Ok(params) => match core
                .resolve_approval(params.request_id, params.decision)
                .await
            {
                Ok(()) => RpcResponse::success(request.id, json!({})),
                Err(error) => RpcResponse::error(request.id, -32030, error.to_string()),
            },
            Err(error) => RpcResponse::error(request.id, -32602, error.to_string()),
        },
        "session/events" => match serde_json::from_value::<SessionParams>(request.params) {
            Ok(params) => match core.events_for_session(params.session_id) {
                Ok(events) => RpcResponse::success(request.id, events),
                Err(error) => RpcResponse::error(request.id, -32040, error.to_string()),
            },
            Err(error) => RpcResponse::error(request.id, -32602, error.to_string()),
        },
        "tool/call" => match serde_json::from_value::<CallToolParams>(request.params) {
            Ok(params) => match core
                .call_tool(params.session_id, params.call, params.mode)
                .await
            {
                Ok(output) => RpcResponse::success(request.id, output),
                Err(error) => RpcResponse::error(request.id, -32010, error.to_string()),
            },
            Err(error) => RpcResponse::error(request.id, -32602, error.to_string()),
        },
        _ => RpcResponse::error(request.id, -32601, "method not found"),
    }
}
