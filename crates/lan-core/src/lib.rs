mod config;
mod model;
mod policy;
mod store;
mod tools;

use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use lan_protocol::{
    ApprovalDecision, ApprovalMode, ApprovalRequest, CoreEvent, PolicyDecision, Session, SessionId,
    SessionStatus, TokenUsage, ToolCall, ToolDescriptor, TurnResult,
};
use serde_json::Value;
use tokio::sync::{RwLock, broadcast, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub use config::{LanConfig, ProviderConfig};
pub use model::{
    ModelMessage, ModelProvider, ModelRequest, ModelResponse, ModelRole, OpenAiCompatibleProvider,
    WireFunctionCall, WireToolCall,
};
pub use policy::PermissionPolicy;
pub use store::SqliteStore;
pub use tools::{
    ApplyEditsTool, CreateFileTool, EchoTool, GitDiffTool, GitStatusTool, ImageGenerationTool,
    ListFilesTool, ReadFileTool, ReplaceTextTool, RunCommandTool, SearchTextTool, Tool,
    ToolContext, ToolRegistry, VisionTool,
};

const DEFAULT_MAX_PROVIDER_ROUNDS: usize = 48;
const DEFAULT_MODEL_CONTEXT_TOKENS: usize = 128_000;
const CONTEXT_COMPACT_RATIO: f64 = 0.82;
const CONTEXT_KEEP_RECENT_MESSAGES: usize = 10;

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

struct SessionState {
    session: Session,
    messages: Vec<ModelMessage>,
}

struct PendingApproval {
    request: ApprovalRequest,
    response: oneshot::Sender<ApprovalDecision>,
}

pub struct AgentCore {
    sessions: RwLock<HashMap<SessionId, SessionState>>,
    tools: ToolRegistry,
    provider: Option<Arc<dyn ModelProvider>>,
    store: Option<SqliteStore>,
    active_turns: RwLock<HashMap<SessionId, CancellationToken>>,
    pending_approvals: RwLock<HashMap<Uuid, PendingApproval>>,
    events: broadcast::Sender<CoreEvent>,
    max_provider_rounds: usize,
    model_context_tokens: usize,
}

impl AgentCore {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(512);
        let tools = ToolRegistry::new();
        tools.register(Arc::new(EchoTool));
        tools.register(Arc::new(ListFilesTool));
        tools.register(Arc::new(ReadFileTool));
        tools.register(Arc::new(SearchTextTool));
        tools.register(Arc::new(ReplaceTextTool));
        tools.register(Arc::new(RunCommandTool));
        tools.register(Arc::new(CreateFileTool));
        tools.register(Arc::new(GitStatusTool));
        tools.register(Arc::new(GitDiffTool));
        tools.register(Arc::new(ApplyEditsTool));
        Self {
            sessions: RwLock::new(HashMap::new()),
            tools,
            provider: None,
            store: None,
            active_turns: RwLock::new(HashMap::new()),
            pending_approvals: RwLock::new(HashMap::new()),
            events,
            max_provider_rounds: DEFAULT_MAX_PROVIDER_ROUNDS,
            model_context_tokens: DEFAULT_MODEL_CONTEXT_TOKENS,
        }
    }

    pub fn with_max_provider_rounds(mut self, max_provider_rounds: usize) -> Self {
        self.max_provider_rounds = max_provider_rounds.clamp(4, 256);
        self
    }

    pub fn with_model_context_tokens(mut self, model_context_tokens: usize) -> Self {
        self.model_context_tokens = model_context_tokens.clamp(8_000, 2_000_000);
        self
    }

    pub fn with_provider(provider: Arc<dyn ModelProvider>) -> Self {
        let mut core = Self::new();
        core.provider = Some(provider);
        core
    }

    pub fn with_store(store: SqliteStore) -> Result<Self> {
        let mut core = Self::new();
        core.sessions = RwLock::new(
            store
                .load_sessions()?
                .into_iter()
                .map(|(mut session, messages)| {
                    if matches!(
                        session.status,
                        SessionStatus::Running | SessionStatus::WaitingForApproval
                    ) {
                        session.status = SessionStatus::Interrupted;
                        let _ = store.save_session(&session, &messages);
                    }
                    (session.id, SessionState { session, messages })
                })
                .collect(),
        );
        core.store = Some(store);
        Ok(core)
    }

    pub fn with_provider_and_store(
        provider: Arc<dyn ModelProvider>,
        store: SqliteStore,
    ) -> Result<Self> {
        let mut core = Self::with_store(store)?;
        core.provider = Some(provider);
        Ok(core)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.events.subscribe()
    }

    pub async fn create_session(&self, cwd: String, title: Option<String>) -> Session {
        let session = Session {
            id: Uuid::new_v4(),
            cwd,
            title,
            status: SessionStatus::Idle,
            updated_at: unix_timestamp(),
        };
        self.sessions
            .write()
            .await
            .insert(session.id, SessionState {
                session: session.clone(),
                messages: vec![ModelMessage::text(
                    ModelRole::System,
                    "You are Lan Code, a careful coding agent. Inspect the workspace with tools before answering. Never invent file contents. Prefer small, reviewable edits. When a model has limited context, summarize older conversation state before continuing. Keep the final answer concise and list changed files when relevant.",
                )],
            });
        self.persist_session(session.id).await;
        self.emit(
            session.id,
            CoreEvent::SessionCreated {
                event_id: Uuid::new_v4(),
                session: session.clone(),
            },
        );
        session
    }

    pub async fn list_sessions(&self) -> Vec<Session> {
        let mut sessions = self
            .sessions
            .read()
            .await
            .values()
            .map(|state| state.session.clone())
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
        sessions
    }

    pub async fn delete_session(&self, session_id: SessionId) -> Result<()> {
        if self.active_turns.read().await.contains_key(&session_id) {
            bail!("cannot delete a session while a turn is running");
        }
        self.sessions
            .write()
            .await
            .remove(&session_id)
            .context("session not found")?;
        if let Some(store) = &self.store {
            store.delete_session(session_id)?;
        }
        Ok(())
    }

    pub async fn rename_session(&self, session_id: SessionId, title: String) -> Result<()> {
        let title = title.trim();
        if title.is_empty() {
            bail!("session title cannot be empty");
        }
        let mut sessions = self.sessions.write().await;
        let session = &mut sessions
            .get_mut(&session_id)
            .context("session not found")?
            .session;
        session.title = Some(title.chars().take(80).collect());
        session.updated_at = unix_timestamp();
        drop(sessions);
        self.persist_session(session_id).await;
        Ok(())
    }

    pub async fn messages_for_session(&self, session_id: SessionId) -> Result<Vec<ModelMessage>> {
        self.sessions
            .read()
            .await
            .get(&session_id)
            .map(|state| state.messages.clone())
            .context("session not found")
    }

    pub fn list_tools(&self) -> Vec<ToolDescriptor> {
        self.tools.list()
    }

    pub fn register_tool(&self, tool: Arc<dyn Tool>) {
        self.tools.register(tool);
    }

    pub fn events_for_session(&self, session_id: SessionId) -> Result<Vec<CoreEvent>> {
        self.store
            .as_ref()
            .context("no persistent store configured")?
            .load_events(session_id)
    }

    fn emit(&self, session_id: SessionId, event: CoreEvent) {
        if let Some(store) = &self.store {
            let _ = store.append_event(session_id, &event);
        }
        let _ = self.events.send(event);
    }

    fn text_emitter(
        &self,
        session_id: SessionId,
        turn_id: Uuid,
    ) -> Arc<dyn Fn(String) + Send + Sync> {
        let events = self.events.clone();
        let store = self.store.clone();
        Arc::new(move |text| {
            let event = CoreEvent::TextDelta {
                event_id: Uuid::new_v4(),
                session_id,
                turn_id,
                text,
            };
            if let Some(store) = &store {
                let _ = store.append_event(session_id, &event);
            }
            let _ = events.send(event);
        })
    }

    async fn persist_session(&self, session_id: SessionId) {
        let Some(store) = &self.store else {
            return;
        };
        if let Some(state) = self.sessions.read().await.get(&session_id) {
            let _ = store.save_session(&state.session, &state.messages);
        }
    }

    async fn set_status(&self, session_id: SessionId, status: SessionStatus) {
        if let Some(state) = self.sessions.write().await.get_mut(&session_id) {
            state.session.status = status;
            state.session.updated_at = unix_timestamp();
        }
        self.persist_session(session_id).await;
    }

    pub async fn call_tool(
        &self,
        session_id: SessionId,
        call: ToolCall,
        mode: ApprovalMode,
    ) -> Result<Value> {
        let result = self
            .execute_tool(session_id, call, mode, CancellationToken::new())
            .await;
        self.set_status(session_id, SessionStatus::Idle).await;
        result
    }

    async fn execute_tool(
        &self,
        session_id: SessionId,
        call: ToolCall,
        mode: ApprovalMode,
        cancel: CancellationToken,
    ) -> Result<Value> {
        let session = self
            .sessions
            .read()
            .await
            .get(&session_id)
            .map(|state| state.session.clone())
            .context("session not found")?;
        let tool = self.tools.get(&call.name).context("tool not found")?;
        match PermissionPolicy::evaluate(mode, &tool.descriptor(), call.arguments.clone()) {
            PolicyDecision::Allow => {}
            PolicyDecision::Ask { request } => {
                self.await_approval(session_id, request, cancel.clone())
                    .await?;
                self.set_status(session_id, SessionStatus::Running).await;
            }
            PolicyDecision::Deny { reason } => bail!(reason),
        }
        let call_id = call.id.clone();
        let tool_name = call.name.clone();
        self.emit(
            session_id,
            CoreEvent::ToolStarted {
                event_id: Uuid::new_v4(),
                session_id,
                tool_call_id: call_id.clone(),
                tool_name: tool_name.clone(),
                arguments: call.arguments.clone(),
            },
        );
        let result: Result<Value> = tokio::select! {
            _ = cancel.cancelled() => bail!("turn interrupted"),
            output = tool.execute(
                ToolContext {
                    session_id,
                    cwd: session.cwd,
                    allow_unsandboxed_commands: mode == ApprovalMode::FullAccess,
                },
                call.arguments,
            ) => output,
        };
        match result {
            Ok(output) => {
                self.emit(
                    session_id,
                    CoreEvent::ToolCompleted {
                        event_id: Uuid::new_v4(),
                        session_id,
                        tool_call_id: call_id,
                        tool_name,
                        output: output.clone(),
                    },
                );
                Ok(output)
            }
            Err(error) => {
                self.emit(
                    session_id,
                    CoreEvent::ToolFailed {
                        event_id: Uuid::new_v4(),
                        session_id,
                        tool_call_id: call_id,
                        tool_name,
                        error: error.to_string(),
                    },
                );
                Err(error)
            }
        }
    }

    async fn await_approval(
        &self,
        session_id: SessionId,
        request: ApprovalRequest,
        cancel: CancellationToken,
    ) -> Result<()> {
        let request_id = request.id;
        let (tx, rx) = oneshot::channel();
        self.pending_approvals.write().await.insert(
            request_id,
            PendingApproval {
                request: request.clone(),
                response: tx,
            },
        );
        self.set_status(session_id, SessionStatus::WaitingForApproval)
            .await;
        self.emit(
            session_id,
            CoreEvent::ApprovalRequested {
                event_id: Uuid::new_v4(),
                session_id,
                request,
            },
        );
        let decision = tokio::select! {
            _ = cancel.cancelled() => {
                self.pending_approvals.write().await.remove(&request_id);
                bail!("turn interrupted");
            }
            result = rx => result.context("approval channel closed")?,
        };
        match decision {
            ApprovalDecision::AllowOnce => Ok(()),
            ApprovalDecision::Deny => bail!("approval denied"),
        }
    }

    pub async fn resolve_approval(
        &self,
        request_id: Uuid,
        decision: ApprovalDecision,
    ) -> Result<()> {
        let pending = self
            .pending_approvals
            .write()
            .await
            .remove(&request_id)
            .context("approval request not found")?;
        pending
            .response
            .send(decision)
            .map_err(|_| anyhow::anyhow!("approval requester no longer active"))
    }

    pub async fn pending_approvals(&self) -> Vec<ApprovalRequest> {
        self.pending_approvals
            .read()
            .await
            .values()
            .map(|pending| pending.request.clone())
            .collect()
    }

    pub async fn interrupt_turn(&self, session_id: SessionId) -> bool {
        if let Some(cancel) = self.active_turns.read().await.get(&session_id) {
            cancel.cancel();
            true
        } else {
            false
        }
    }

    pub async fn start_turn(
        &self,
        session_id: SessionId,
        prompt: String,
        mode: ApprovalMode,
    ) -> Result<TurnResult> {
        let provider = self
            .provider
            .clone()
            .context("no model provider configured")?;
        let turn_id = Uuid::new_v4();
        let cancel = CancellationToken::new();
        {
            let mut active = self.active_turns.write().await;
            if active.contains_key(&session_id) {
                bail!("session already has an active turn");
            }
            active.insert(session_id, cancel.clone());
        }
        let result = self
            .run_turn(session_id, turn_id, prompt, mode, provider, cancel.clone())
            .await;
        self.active_turns.write().await.remove(&session_id);
        match &result {
            Ok(_) => self.set_status(session_id, SessionStatus::Idle).await,
            Err(error) if cancel.is_cancelled() => {
                self.set_status(session_id, SessionStatus::Interrupted)
                    .await;
                self.emit(
                    session_id,
                    CoreEvent::TurnInterrupted {
                        event_id: Uuid::new_v4(),
                        session_id,
                        turn_id,
                    },
                );
                let _ = error;
            }
            Err(error) => {
                self.set_status(session_id, SessionStatus::Failed).await;
                self.emit(
                    session_id,
                    CoreEvent::TurnFailed {
                        event_id: Uuid::new_v4(),
                        session_id,
                        turn_id,
                        error: error.to_string(),
                    },
                );
            }
        }
        result
    }

    async fn run_turn(
        &self,
        session_id: SessionId,
        turn_id: Uuid,
        prompt: String,
        mode: ApprovalMode,
        provider: Arc<dyn ModelProvider>,
        cancel: CancellationToken,
    ) -> Result<TurnResult> {
        {
            let mut sessions = self.sessions.write().await;
            let state = sessions.get_mut(&session_id).context("session not found")?;
            state.session.status = SessionStatus::Running;
            state.session.updated_at = unix_timestamp();
            state
                .messages
                .push(ModelMessage::text(ModelRole::User, prompt));
        }
        self.persist_session(session_id).await;
        self.emit(
            session_id,
            CoreEvent::TurnStarted {
                event_id: Uuid::new_v4(),
                session_id,
                turn_id,
            },
        );

        let mut repeated_calls = HashMap::<String, usize>::new();
        let mut usage = TokenUsage::default();
        for round in 1..=self.max_provider_rounds {
            let messages = self.prepare_messages_for_provider(
                self.sessions
                    .read()
                    .await
                    .get(&session_id)
                    .context("session not found")?
                    .messages
                    .clone(),
            );
            let response = tokio::select! {
                _ = cancel.cancelled() => bail!("turn interrupted"),
                response = provider.complete_stream(ModelRequest {
                    messages,
                    tools: self.list_tools(),
                }, self.text_emitter(session_id, turn_id)) => response?,
            };
            accumulate_usage(&mut usage, response.usage);
            {
                let mut sessions = self.sessions.write().await;
                sessions
                    .get_mut(&session_id)
                    .context("session not found")?
                    .messages
                    .push(response.message.clone());
            }
            self.persist_session(session_id).await;
            if response.tool_calls.is_empty() {
                let text = response.text;
                self.emit(
                    session_id,
                    CoreEvent::UsageRecorded {
                        event_id: Uuid::new_v4(),
                        session_id,
                        turn_id,
                        model: provider.model_name().to_string(),
                        usage,
                    },
                );
                self.emit(
                    session_id,
                    CoreEvent::TurnCompleted {
                        event_id: Uuid::new_v4(),
                        session_id,
                        turn_id,
                    },
                );
                return Ok(TurnResult {
                    session_id,
                    turn_id,
                    text,
                    provider_rounds: round as u32,
                    usage,
                });
            }
            for call in response.tool_calls {
                let call_id = call.id.clone();
                let signature = format!("{}:{}", call.name, call.arguments);
                let repeated = repeated_calls.entry(signature).or_default();
                *repeated += 1;
                let output = if *repeated > 3 {
                    let error = format!(
                        "blocked repeated identical tool call after {} attempts; inspect the previous result and choose a different action",
                        *repeated - 1
                    );
                    self.emit(
                        session_id,
                        CoreEvent::ToolFailed {
                            event_id: Uuid::new_v4(),
                            session_id,
                            tool_call_id: call_id.clone(),
                            tool_name: call.name,
                            error: error.clone(),
                        },
                    );
                    serde_json::json!({"error": error})
                } else {
                    match self
                        .execute_tool(session_id, call, mode, cancel.clone())
                        .await
                    {
                        Ok(value) => value,
                        Err(error) if cancel.is_cancelled() => return Err(error),
                        Err(error) => serde_json::json!({"error": error.to_string()}),
                    }
                };
                self.sessions
                    .write()
                    .await
                    .get_mut(&session_id)
                    .context("session not found")?
                    .messages
                    .push(ModelMessage {
                        role: ModelRole::Tool,
                        content: Some(output.to_string()),
                        reasoning_content: None,
                        tool_call_id: Some(call_id),
                        tool_calls: None,
                    });
                self.persist_session(session_id).await;
            }
        }
        {
            let mut sessions = self.sessions.write().await;
            sessions
                .get_mut(&session_id)
                .context("session not found")?
                .messages
                .push(ModelMessage::text(
                    ModelRole::System,
                    "The execution budget is exhausted. Do not call tools. Summarize what was completed, list unfinished work, and explain how the user can continue.",
                ));
        }
        self.persist_session(session_id).await;
        let messages = self.prepare_messages_for_provider(
            self.sessions
                .read()
                .await
                .get(&session_id)
                .context("session not found")?
                .messages
                .clone(),
        );
        let response = tokio::select! {
            _ = cancel.cancelled() => bail!("turn interrupted"),
            response = provider.complete_stream(ModelRequest {
                messages,
                tools: Vec::new(),
            }, self.text_emitter(session_id, turn_id)) => response?,
        };
        accumulate_usage(&mut usage, response.usage);
        let text = if response.text.trim().is_empty() {
            format!(
                "任务已达到 {} 轮执行预算。已经完成的文件修改会保留，请检查 Git diff 后继续任务。",
                self.max_provider_rounds
            )
        } else {
            response.text.clone()
        };
        self.sessions
            .write()
            .await
            .get_mut(&session_id)
            .context("session not found")?
            .messages
            .push(response.message);
        self.persist_session(session_id).await;
        self.emit(
            session_id,
            CoreEvent::UsageRecorded {
                event_id: Uuid::new_v4(),
                session_id,
                turn_id,
                model: provider.model_name().to_string(),
                usage,
            },
        );
        self.emit(
            session_id,
            CoreEvent::TurnCompleted {
                event_id: Uuid::new_v4(),
                session_id,
                turn_id,
            },
        );
        Ok(TurnResult {
            session_id,
            turn_id,
            text,
            provider_rounds: (self.max_provider_rounds + 1) as u32,
            usage,
        })
    }

    fn prepare_messages_for_provider(&self, messages: Vec<ModelMessage>) -> Vec<ModelMessage> {
        compact_messages_for_context(messages, self.model_context_tokens)
    }
}

fn compact_messages_for_context(
    messages: Vec<ModelMessage>,
    model_context_tokens: usize,
) -> Vec<ModelMessage> {
    let budget = ((model_context_tokens as f64) * CONTEXT_COMPACT_RATIO) as usize;
    if estimate_messages_tokens(&messages) <= budget
        || messages.len() <= CONTEXT_KEEP_RECENT_MESSAGES + 2
    {
        return messages;
    }

    let mut compacted = Vec::new();
    let mut iter = messages.into_iter();
    if let Some(first) = iter.next() {
        if first.role == ModelRole::System {
            compacted.push(first);
        } else {
            compacted.push(ModelMessage::text(
                ModelRole::System,
                "Conversation context was compacted before this request.",
            ));
            compacted.push(first);
        }
    }
    let remaining = iter.collect::<Vec<_>>();
    let keep_count = CONTEXT_KEEP_RECENT_MESSAGES.min(remaining.len());
    let split_at = remaining.len().saturating_sub(keep_count);
    let omitted = &remaining[..split_at];
    let recent = &remaining[split_at..];
    let omitted_tokens = estimate_messages_tokens(omitted);
    if !omitted.is_empty() {
        compacted.push(ModelMessage::text(
            ModelRole::System,
            format!(
                "Lan Code compacted {} older messages (~{} tokens estimated) because the active model context window is {} tokens. Preserve the user's current goal, rely on recent messages and tools, and ask for missing details only when necessary.",
                omitted.len(),
                omitted_tokens,
                model_context_tokens
            ),
        ));
    }
    compacted.extend(recent.iter().cloned());
    while estimate_messages_tokens(&compacted) > budget && compacted.len() > 3 {
        compacted.remove(2);
    }
    compacted
}

fn estimate_messages_tokens(messages: &[ModelMessage]) -> usize {
    messages
        .iter()
        .map(|message| {
            let mut chars = 12;
            chars += message
                .content
                .as_deref()
                .unwrap_or_default()
                .chars()
                .count();
            chars += message
                .reasoning_content
                .as_deref()
                .unwrap_or_default()
                .chars()
                .count();
            chars += message
                .tool_calls
                .as_ref()
                .map(|calls| {
                    calls
                        .iter()
                        .map(|call| {
                            call.function.arguments.chars().count() + call.function.name.len()
                        })
                        .sum::<usize>()
                })
                .unwrap_or_default();
            (chars / 4).max(1)
        })
        .sum()
}

fn accumulate_usage(total: &mut TokenUsage, value: TokenUsage) {
    total.input_tokens += value.input_tokens;
    total.output_tokens += value.output_tokens;
    total.total_tokens += value.total_tokens;
    total.cached_input_tokens += value.cached_input_tokens;
}

impl Default for AgentCore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        fs, future,
        sync::{Arc, Mutex},
    };

    use anyhow::Result;
    use async_trait::async_trait;
    use lan_protocol::{
        ApprovalDecision, ApprovalMode, CoreEvent, SessionStatus, TokenUsage, ToolCall,
    };
    use serde_json::json;

    use super::{
        AgentCore, ModelMessage, ModelProvider, ModelRequest, ModelResponse, ModelRole,
        SqliteStore, WireFunctionCall, WireToolCall,
    };

    #[tokio::test]
    async fn creates_session_and_executes_safe_tool() {
        let core = AgentCore::new();
        let session = core.create_session(".".into(), None).await;
        let output = core
            .call_tool(
                session.id,
                ToolCall {
                    id: "call-1".into(),
                    name: "echo".into(),
                    arguments: json!({"text": "hello"}),
                },
                ApprovalMode::ReadOnly,
            )
            .await
            .unwrap();
        assert_eq!(output, json!({"text": "hello"}));
    }

    #[tokio::test]
    async fn read_only_mode_denies_host_commands_before_execution() {
        let core = AgentCore::new();
        let session = core.create_session(".".into(), None).await;
        let error = core
            .call_tool(
                session.id,
                ToolCall {
                    id: "call-command".into(),
                    name: "run_command".into(),
                    arguments: json!({"program": "definitely-does-not-exist", "args": []}),
                },
                ApprovalMode::ReadOnly,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not allowed in read-only mode"));
    }

    struct ScriptedProvider {
        responses: Mutex<VecDeque<ModelResponse>>,
    }

    #[async_trait]
    impl ModelProvider for ScriptedProvider {
        fn model_name(&self) -> &str {
            "scripted"
        }

        async fn complete(&self, _request: ModelRequest) -> Result<ModelResponse> {
            Ok(self.responses.lock().unwrap().pop_front().unwrap())
        }
    }

    #[tokio::test]
    async fn agent_loop_executes_tool_before_final_answer() {
        let tool_call = WireToolCall {
            id: "call-1".into(),
            kind: "function".into(),
            function: WireFunctionCall {
                name: "echo".into(),
                arguments: r#"{"text":"observed"}"#.into(),
            },
        };
        let provider = ScriptedProvider {
            responses: Mutex::new(VecDeque::from([
                ModelResponse {
                    message: ModelMessage {
                        role: ModelRole::Assistant,
                        content: None,
                        reasoning_content: None,
                        tool_call_id: None,
                        tool_calls: Some(vec![tool_call]),
                    },
                    text: String::new(),
                    tool_calls: vec![ToolCall {
                        id: "call-1".into(),
                        name: "echo".into(),
                        arguments: json!({"text": "observed"}),
                    }],
                    usage: TokenUsage::default(),
                },
                ModelResponse {
                    message: ModelMessage::text(ModelRole::Assistant, "done"),
                    text: "done".into(),
                    tool_calls: Vec::new(),
                    usage: TokenUsage::default(),
                },
            ])),
        };
        let core = AgentCore::with_provider(Arc::new(provider));
        let session = core.create_session(".".into(), None).await;
        let result = core
            .start_turn(session.id, "inspect".into(), ApprovalMode::ReadOnly)
            .await
            .unwrap();
        assert_eq!(result.text, "done");
        assert_eq!(result.provider_rounds, 2);
    }

    #[tokio::test]
    async fn turn_aggregates_and_persists_token_usage() {
        let root = std::env::temp_dir().join(format!("lan-usage-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let provider = ScriptedProvider {
            responses: Mutex::new(VecDeque::from([ModelResponse {
                message: ModelMessage::text(ModelRole::Assistant, "done"),
                text: "done".into(),
                tool_calls: Vec::new(),
                usage: TokenUsage {
                    input_tokens: 120,
                    output_tokens: 30,
                    total_tokens: 150,
                    cached_input_tokens: 20,
                },
            }])),
        };
        let core = AgentCore::with_provider_and_store(
            Arc::new(provider),
            SqliteStore::open(root.join("lan.sqlite")).unwrap(),
        )
        .unwrap();
        let session = core.create_session(root.display().to_string(), None).await;
        let result = core
            .start_turn(session.id, "test usage".into(), ApprovalMode::ReadOnly)
            .await
            .unwrap();
        assert_eq!(result.usage.total_tokens, 150);
        assert!(core.events_for_session(session.id).unwrap().iter().any(
            |event| matches!(event, CoreEvent::UsageRecorded { usage, .. } if usage.total_tokens == 150)
        ));
        drop(core);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn execution_budget_forces_a_final_summary_instead_of_failing() {
        let mut responses = VecDeque::new();
        for index in 0..4 {
            responses.push_back(scripted_tool_response(
                &format!("call-{index}"),
                "echo",
                json!({"text": format!("step-{index}")}),
            ));
        }
        responses.push_back(ModelResponse {
            message: ModelMessage::text(ModelRole::Assistant, "已完成四个步骤，剩余工作可继续。"),
            text: "已完成四个步骤，剩余工作可继续。".into(),
            tool_calls: Vec::new(),
            usage: TokenUsage::default(),
        });
        let core = AgentCore::with_provider(Arc::new(ScriptedProvider {
            responses: Mutex::new(responses),
        }))
        .with_max_provider_rounds(4);
        let session = core.create_session(".".into(), None).await;
        let result = core
            .start_turn(session.id, "执行长任务".into(), ApprovalMode::ReadOnly)
            .await
            .unwrap();
        assert_eq!(result.provider_rounds, 5);
        assert!(result.text.contains("剩余工作"));
        assert_eq!(core.list_sessions().await[0].status, SessionStatus::Idle);
    }

    fn scripted_tool_response(id: &str, name: &str, arguments: serde_json::Value) -> ModelResponse {
        let arguments = arguments.to_string();
        let call = WireToolCall {
            id: id.into(),
            kind: "function".into(),
            function: WireFunctionCall {
                name: name.into(),
                arguments: arguments.clone(),
            },
        };
        ModelResponse {
            message: ModelMessage {
                role: ModelRole::Assistant,
                content: None,
                reasoning_content: None,
                tool_call_id: None,
                tool_calls: Some(vec![call]),
            },
            text: String::new(),
            tool_calls: vec![ToolCall {
                id: id.into(),
                name: name.into(),
                arguments: serde_json::from_str(&arguments).unwrap(),
            }],
            usage: TokenUsage::default(),
        }
    }

    #[test]
    fn compacts_old_messages_for_small_context_windows() {
        let mut messages = vec![ModelMessage::text(ModelRole::System, "system prompt")];
        for index in 0..40 {
            messages.push(ModelMessage::text(
                if index % 2 == 0 {
                    ModelRole::User
                } else {
                    ModelRole::Assistant
                },
                format!("message-{index} {}", "x".repeat(900)),
            ));
        }
        messages.push(ModelMessage::text(ModelRole::User, "current request"));

        let compacted = super::compact_messages_for_context(messages, 8_000);

        assert!(compacted.len() < 20);
        assert_eq!(compacted.first().unwrap().role, ModelRole::System);
        assert!(compacted.iter().any(|message| {
            message
                .content
                .as_deref()
                .unwrap_or_default()
                .contains("compacted")
        }));
        assert!(
            compacted
                .last()
                .unwrap()
                .content
                .as_deref()
                .unwrap()
                .contains("current request")
        );
    }

    #[tokio::test]
    async fn coding_eval_completes_multi_file_edit_and_review_loop() {
        let root = std::env::temp_dir().join(format!("lan-eval-edit-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("alpha.txt"), "alpha = false\n").unwrap();
        fs::write(root.join("beta.txt"), "beta = false\n").unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&root)
            .status()
            .unwrap();

        let provider = ScriptedProvider {
            responses: Mutex::new(VecDeque::from([
                scripted_tool_response(
                    "edit",
                    "apply_edits",
                    json!({"edits": [
                        {"path": "alpha.txt", "old_text": "false", "new_text": "true"},
                        {"path": "beta.txt", "old_text": "false", "new_text": "true"}
                    ]}),
                ),
                scripted_tool_response("review", "git_diff", json!({})),
                ModelResponse {
                    message: ModelMessage::text(ModelRole::Assistant, "implemented and reviewed"),
                    text: "implemented and reviewed".into(),
                    tool_calls: Vec::new(),
                    usage: TokenUsage::default(),
                },
            ])),
        };
        let core = AgentCore::with_provider(Arc::new(provider));
        let session = core.create_session(root.display().to_string(), None).await;
        let result = core
            .start_turn(
                session.id,
                "enable both flags".into(),
                ApprovalMode::Workspace,
            )
            .await
            .unwrap();

        assert_eq!(result.text, "implemented and reviewed");
        assert_eq!(result.provider_rounds, 3);
        assert_eq!(
            fs::read_to_string(root.join("alpha.txt")).unwrap(),
            "alpha = true\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("beta.txt")).unwrap(),
            "beta = true\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn coding_eval_creates_a_new_file() {
        let root = std::env::temp_dir().join(format!("lan-eval-create-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let provider = ScriptedProvider {
            responses: Mutex::new(VecDeque::from([
                scripted_tool_response(
                    "create",
                    "create_file",
                    json!({"path": "hello.txt", "content": "hello from Lan Code\n"}),
                ),
                ModelResponse {
                    message: ModelMessage::text(ModelRole::Assistant, "created"),
                    text: "created".into(),
                    tool_calls: Vec::new(),
                    usage: TokenUsage::default(),
                },
            ])),
        };
        let core = AgentCore::with_provider(Arc::new(provider));
        let session = core.create_session(root.display().to_string(), None).await;
        core.start_turn(
            session.id,
            "create greeting".into(),
            ApprovalMode::Workspace,
        )
        .await
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("hello.txt")).unwrap(),
            "hello from Lan Code\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn sqlite_store_restores_sessions_and_events() {
        let root = std::env::temp_dir().join(format!("lan-store-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let db = root.join("lan.sqlite");
        let session_id;
        {
            let core = AgentCore::with_store(SqliteStore::open(&db).unwrap()).unwrap();
            let session = core.create_session(root.display().to_string(), None).await;
            session_id = session.id;
            assert_eq!(core.events_for_session(session_id).unwrap().len(), 1);
        }
        {
            let core = AgentCore::with_store(SqliteStore::open(&db).unwrap()).unwrap();
            assert_eq!(core.list_sessions().await[0].id, session_id);
            assert_eq!(core.events_for_session(session_id).unwrap().len(), 1);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn deleting_a_session_removes_persisted_history_and_events() {
        let root = std::env::temp_dir().join(format!("lan-delete-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let db = root.join("lan.sqlite");
        let session_id;
        {
            let core = AgentCore::with_store(SqliteStore::open(&db).unwrap()).unwrap();
            let session = core.create_session(root.display().to_string(), None).await;
            session_id = session.id;
            core.delete_session(session_id).await.unwrap();
            assert!(core.list_sessions().await.is_empty());
        }
        {
            let core = AgentCore::with_store(SqliteStore::open(&db).unwrap()).unwrap();
            assert!(core.list_sessions().await.is_empty());
            assert!(core.events_for_session(session_id).unwrap().is_empty());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn renaming_a_session_is_persisted() {
        let root = std::env::temp_dir().join(format!("lan-rename-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let db = root.join("lan.sqlite");
        let session_id;
        {
            let core = AgentCore::with_store(SqliteStore::open(&db).unwrap()).unwrap();
            let session = core.create_session(root.display().to_string(), None).await;
            session_id = session.id;
            core.rename_session(session.id, "新的名称".into())
                .await
                .unwrap();
        }
        let core = AgentCore::with_store(SqliteStore::open(&db).unwrap()).unwrap();
        let session = core
            .list_sessions()
            .await
            .into_iter()
            .find(|session| session.id == session_id)
            .unwrap();
        assert_eq!(session.title.as_deref(), Some("新的名称"));
        drop(core);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn ask_mode_pauses_until_approval_is_resolved() {
        let root = std::env::temp_dir().join(format!("lan-approval-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("sample.txt"), "before\n").unwrap();
        let core = Arc::new(AgentCore::new());
        let session = core.create_session(root.display().to_string(), None).await;
        let mut events = core.subscribe();
        let worker = {
            let core = core.clone();
            tokio::spawn(async move {
                core.call_tool(
                    session.id,
                    ToolCall {
                        id: "call-write".into(),
                        name: "replace_text".into(),
                        arguments: json!({
                            "path": "sample.txt",
                            "old_text": "before",
                            "new_text": "after"
                        }),
                    },
                    ApprovalMode::Ask,
                )
                .await
            })
        };
        let request_id = loop {
            if let CoreEvent::ApprovalRequested { request, .. } = events.recv().await.unwrap() {
                break request.id;
            }
        };
        assert_eq!(
            core.list_sessions().await[0].status,
            SessionStatus::WaitingForApproval
        );
        core.resolve_approval(request_id, ApprovalDecision::AllowOnce)
            .await
            .unwrap();
        worker.await.unwrap().unwrap();
        assert_eq!(
            fs::read_to_string(root.join("sample.txt")).unwrap(),
            "after\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    struct BlockingProvider;

    #[async_trait]
    impl ModelProvider for BlockingProvider {
        fn model_name(&self) -> &str {
            "blocking"
        }

        async fn complete(&self, _request: ModelRequest) -> Result<ModelResponse> {
            future::pending().await
        }
    }

    #[tokio::test]
    async fn interrupt_cancels_active_provider_wait() {
        let core = Arc::new(AgentCore::with_provider(Arc::new(BlockingProvider)));
        let session = core.create_session(".".into(), None).await;
        let worker = {
            let core = core.clone();
            tokio::spawn(async move {
                core.start_turn(session.id, "wait".into(), ApprovalMode::ReadOnly)
                    .await
            })
        };
        tokio::task::yield_now().await;
        assert!(core.interrupt_turn(session.id).await);
        assert!(worker.await.unwrap().is_err());
        assert_eq!(
            core.list_sessions().await[0].status,
            SessionStatus::Interrupted
        );
    }
}
