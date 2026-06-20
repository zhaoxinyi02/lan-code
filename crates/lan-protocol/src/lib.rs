use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub type SessionId = Uuid;
pub type TurnId = Uuid;
pub type EventId = Uuid;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cached_input_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RpcRequest {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RpcResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RpcNotification {
    pub method: String,
    pub params: Value,
}

impl RpcResponse {
    pub fn success(id: String, result: impl Serialize) -> Self {
        Self {
            id,
            result: Some(serde_json::to_value(result).expect("serializable RPC result")),
            error: None,
        }
    }

    pub fn error(id: String, code: i32, message: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: SessionId,
    pub cwd: String,
    pub title: Option<String>,
    pub status: SessionStatus,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    Idle,
    Running,
    WaitingForApproval,
    Interrupted,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CoreEvent {
    SessionCreated {
        event_id: EventId,
        session: Session,
    },
    TurnStarted {
        event_id: EventId,
        session_id: SessionId,
        turn_id: TurnId,
    },
    TextDelta {
        event_id: EventId,
        session_id: SessionId,
        turn_id: TurnId,
        text: String,
    },
    UsageRecorded {
        event_id: EventId,
        session_id: SessionId,
        turn_id: TurnId,
        model: String,
        usage: TokenUsage,
    },
    ContextUsageUpdated {
        event_id: EventId,
        session_id: SessionId,
        turn_id: TurnId,
        used_tokens: u64,
        context_window: u64,
    },
    ContextCompactionStarted {
        event_id: EventId,
        session_id: SessionId,
        turn_id: TurnId,
        before_tokens: u64,
        context_window: u64,
    },
    ContextCompactionCompleted {
        event_id: EventId,
        session_id: SessionId,
        turn_id: TurnId,
        before_tokens: u64,
        after_tokens: u64,
        compacted_messages: usize,
    },
    ApprovalRequested {
        event_id: EventId,
        session_id: SessionId,
        request: ApprovalRequest,
    },
    ToolStarted {
        event_id: EventId,
        session_id: SessionId,
        tool_call_id: String,
        tool_name: String,
        arguments: Value,
    },
    ToolCompleted {
        event_id: EventId,
        session_id: SessionId,
        tool_call_id: String,
        tool_name: String,
        output: Value,
    },
    ToolFailed {
        event_id: EventId,
        session_id: SessionId,
        tool_call_id: String,
        tool_name: String,
        error: String,
    },
    TurnCompleted {
        event_id: EventId,
        session_id: SessionId,
        turn_id: TurnId,
    },
    TurnInterrupted {
        event_id: EventId,
        session_id: SessionId,
        turn_id: TurnId,
    },
    TurnFailed {
        event_id: EventId,
        session_id: SessionId,
        turn_id: TurnId,
        error: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum RiskLevel {
    ReadOnly,
    WorkspaceWrite,
    ExternalSideEffect,
    FullAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub risk: RiskLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnResult {
    pub session_id: SessionId,
    pub turn_id: TurnId,
    pub text: String,
    pub provider_rounds: u32,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: Uuid,
    pub tool_name: String,
    pub risk: RiskLevel,
    pub reason: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalMode {
    ReadOnly,
    Ask,
    Workspace,
    FullAccess,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalDecision {
    AllowOnce,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "decision", rename_all = "camelCase")]
pub enum PolicyDecision {
    Allow,
    Ask { request: ApprovalRequest },
    Deny { reason: String },
}
