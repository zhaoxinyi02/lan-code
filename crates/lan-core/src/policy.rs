use lan_protocol::{ApprovalMode, ApprovalRequest, PolicyDecision, RiskLevel, ToolDescriptor};
use serde_json::Value;
use uuid::Uuid;

pub struct PermissionPolicy;

impl PermissionPolicy {
    pub fn evaluate(mode: ApprovalMode, tool: &ToolDescriptor, arguments: Value) -> PolicyDecision {
        let allowed = match mode {
            ApprovalMode::ReadOnly => tool.risk <= RiskLevel::ReadOnly,
            ApprovalMode::Workspace => tool.risk <= RiskLevel::WorkspaceWrite,
            ApprovalMode::FullAccess => true,
            ApprovalMode::Ask => tool.risk <= RiskLevel::ReadOnly,
        };
        if allowed {
            return PolicyDecision::Allow;
        }
        if mode == ApprovalMode::ReadOnly {
            return PolicyDecision::Deny {
                reason: format!("{} is not allowed in read-only mode", tool.name),
            };
        }
        PolicyDecision::Ask {
            request: ApprovalRequest {
                id: Uuid::new_v4(),
                tool_name: tool.name.clone(),
                risk: tool.risk,
                reason: format!("{} requires {:?} access", tool.name, tool.risk),
                arguments,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use lan_protocol::{ApprovalMode, PolicyDecision, RiskLevel, ToolDescriptor};
    use serde_json::json;

    use super::PermissionPolicy;

    fn tool(risk: RiskLevel) -> ToolDescriptor {
        ToolDescriptor {
            name: "write".into(),
            description: String::new(),
            input_schema: json!({}),
            risk,
        }
    }

    #[test]
    fn ask_mode_requests_approval_for_writes() {
        assert!(matches!(
            PermissionPolicy::evaluate(
                ApprovalMode::Ask,
                &tool(RiskLevel::WorkspaceWrite),
                json!({})
            ),
            PolicyDecision::Ask { .. }
        ));
    }
}
