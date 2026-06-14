use std::sync::Arc;

use lan_core::{AgentCore, LanConfig, SqliteStore};
use lan_protocol::{ApprovalMode, Session, SessionId, TurnResult};
use tauri::State;

struct AppState {
    core: Arc<AgentCore>,
}

#[tauri::command]
async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<Session>, String> {
    Ok(state.core.list_sessions().await)
}

#[tauri::command]
async fn create_session(
    cwd: String,
    title: Option<String>,
    state: State<'_, AppState>,
) -> Result<Session, String> {
    Ok(state.core.create_session(cwd, title).await)
}

#[tauri::command]
async fn start_turn(
    session_id: SessionId,
    prompt: String,
    mode: ApprovalMode,
    state: State<'_, AppState>,
) -> Result<TurnResult, String> {
    state
        .core
        .start_turn(session_id, prompt, mode)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn interrupt_turn(session_id: SessionId, state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.core.interrupt_turn(session_id).await)
}

fn build_core() -> anyhow::Result<AgentCore> {
    let config = LanConfig::load()?;
    let provider = config.provider()?;
    let store = config.database().map(SqliteStore::open).transpose()?;
    match (provider, store) {
        (Some(provider), Some(store)) => {
            AgentCore::with_provider_and_store(Arc::new(provider), store)
        }
        (Some(provider), None) => Ok(AgentCore::with_provider(Arc::new(provider))),
        (None, Some(store)) => AgentCore::with_store(store),
        (None, None) => Ok(AgentCore::new()),
    }
}

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            core: Arc::new(build_core().expect("valid Lan Code configuration")),
        })
        .invoke_handler(tauri::generate_handler![
            list_sessions,
            create_session,
            start_turn,
            interrupt_turn
        ])
        .run(tauri::generate_context!())
        .expect("run Lan Code desktop");
}
