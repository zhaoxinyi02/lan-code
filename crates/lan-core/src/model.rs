use std::{collections::BTreeMap, sync::Arc};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use lan_protocol::{TokenUsage, ToolCall, ToolDescriptor};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelMessage {
    pub role: ModelRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<WireToolCall>>,
}

impl ModelMessage {
    pub fn text(role: ModelRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: Some(content.into()),
            reasoning_content: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WireToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: WireFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WireFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub messages: Vec<ModelMessage>,
    pub tools: Vec<ToolDescriptor>,
}

#[derive(Debug, Clone)]
pub struct ModelResponse {
    pub message: ModelMessage,
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: TokenUsage,
}

#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn model_name(&self) -> &str;
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse>;
    async fn complete_stream(
        &self,
        request: ModelRequest,
        on_text: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<ModelResponse> {
        let response = self.complete(request).await?;
        if !response.text.is_empty() {
            on_text(response.text.clone());
        }
        Ok(response)
    }
}

pub struct OpenAiCompatibleProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    max_output_tokens: Option<u64>,
}

impl OpenAiCompatibleProvider {
    pub fn new(base_url: String, api_key: String, model: String) -> Result<Self> {
        Self::new_with_limits(base_url, api_key, model, None)
    }

    pub fn new_with_limits(
        base_url: String,
        api_key: String,
        model: String,
        max_output_tokens: Option<u64>,
    ) -> Result<Self> {
        if api_key.trim().is_empty() {
            bail!("API key cannot be empty");
        }
        Ok(Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()?,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            max_output_tokens,
        })
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ModelMessage],
    tools: Vec<ChatTool<'a>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
}

#[derive(Serialize)]
struct ChatTool<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    function: ChatFunction<'a>,
}

#[derive(Serialize)]
struct ChatFunction<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a Value,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

#[derive(Default, Deserialize)]
struct ChatUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
    #[serde(default)]
    total_tokens: u64,
    #[serde(default)]
    prompt_tokens_details: ChatPromptTokenDetails,
}

#[derive(Default, Deserialize)]
struct ChatPromptTokenDetails {
    #[serde(default)]
    cached_tokens: u64,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ModelMessage,
}

#[async_trait]
impl ModelProvider for OpenAiCompatibleProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse> {
        let tools = request
            .tools
            .iter()
            .map(|tool| ChatTool {
                kind: "function",
                function: ChatFunction {
                    name: &tool.name,
                    description: &tool.description,
                    parameters: &tool.input_schema,
                },
            })
            .collect();
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&ChatRequest {
                model: &self.model,
                messages: &request.messages,
                tools,
                stream: false,
                max_tokens: self.max_output_tokens,
            })
            .send()
            .await
            .context("model request failed")?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            bail!("{}", provider_error_hint(status.as_u16(), &body));
        }
        let response: ChatResponse =
            serde_json::from_str(&body).context("invalid model response JSON")?;
        let usage = token_usage(response.usage.unwrap_or_default());
        let message = response
            .choices
            .into_iter()
            .next()
            .context("model response contained no choices")?
            .message;
        let text = message.content.clone().unwrap_or_default();
        let tool_calls = message
            .tool_calls
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|call| {
                Ok(ToolCall {
                    id: call.id,
                    name: call.function.name,
                    arguments: serde_json::from_str(&call.function.arguments)
                        .context("tool arguments were not valid JSON")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ModelResponse {
            message,
            text,
            tool_calls,
            usage,
        })
    }

    async fn complete_stream(
        &self,
        request: ModelRequest,
        on_text: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<ModelResponse> {
        let tools = request
            .tools
            .iter()
            .map(|tool| ChatTool {
                kind: "function",
                function: ChatFunction {
                    name: &tool.name,
                    description: &tool.description,
                    parameters: &tool.input_schema,
                },
            })
            .collect();
        let mut response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&ChatRequest {
                model: &self.model,
                messages: &request.messages,
                tools,
                stream: true,
                max_tokens: self.max_output_tokens,
            })
            .send()
            .await
            .context("streaming model request failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!("{}", provider_error_hint(status.as_u16(), &body));
        }

        let mut buffer = String::new();
        let mut text = String::new();
        let mut reasoning = String::new();
        let mut calls = BTreeMap::<usize, WireToolCall>::new();
        let mut usage = TokenUsage::default();
        while let Some(chunk) = response
            .chunk()
            .await
            .context("failed reading model stream")?
        {
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(end) = buffer.find('\n') {
                let line = buffer[..end].trim_end_matches('\r').to_string();
                buffer.drain(..=end);
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                let value: Value =
                    serde_json::from_str(data).context("invalid streaming response JSON")?;
                if let Some(raw_usage) = value.get("usage").filter(|usage| !usage.is_null()) {
                    let parsed: ChatUsage = serde_json::from_value(raw_usage.clone())?;
                    usage = token_usage(parsed);
                }
                let Some(delta) = value["choices"][0].get("delta") else {
                    continue;
                };
                if let Some(part) = delta.get("content").and_then(Value::as_str) {
                    text.push_str(part);
                    on_text(part.to_string());
                }
                if let Some(part) = delta.get("reasoning_content").and_then(Value::as_str) {
                    reasoning.push_str(part);
                }
                if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for call in tool_calls {
                        let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let entry = calls.entry(index).or_insert_with(|| WireToolCall {
                            id: String::new(),
                            kind: "function".into(),
                            function: WireFunctionCall {
                                name: String::new(),
                                arguments: String::new(),
                            },
                        });
                        if let Some(id) = call.get("id").and_then(Value::as_str) {
                            entry.id.push_str(id);
                        }
                        if let Some(kind) = call.get("type").and_then(Value::as_str) {
                            entry.kind = kind.to_string();
                        }
                        if let Some(name) = call["function"].get("name").and_then(Value::as_str) {
                            entry.function.name.push_str(name);
                        }
                        if let Some(arguments) =
                            call["function"].get("arguments").and_then(Value::as_str)
                        {
                            entry.function.arguments.push_str(arguments);
                        }
                    }
                }
            }
        }
        let wire_calls = calls.into_values().collect::<Vec<_>>();
        let tool_calls = wire_calls
            .iter()
            .map(|call| {
                Ok(ToolCall {
                    id: call.id.clone(),
                    name: call.function.name.clone(),
                    arguments: serde_json::from_str(&call.function.arguments)
                        .context("streamed tool arguments were not valid JSON")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ModelResponse {
            message: ModelMessage {
                role: ModelRole::Assistant,
                content: (!text.is_empty()).then_some(text.clone()),
                reasoning_content: (!reasoning.is_empty()).then_some(reasoning),
                tool_call_id: None,
                tool_calls: (!wire_calls.is_empty()).then_some(wire_calls),
            },
            text,
            tool_calls,
            usage,
        })
    }
}

fn token_usage(value: ChatUsage) -> TokenUsage {
    TokenUsage {
        input_tokens: value.prompt_tokens,
        output_tokens: value.completion_tokens,
        total_tokens: if value.total_tokens == 0 {
            value.prompt_tokens + value.completion_tokens
        } else {
            value.total_tokens
        },
        cached_input_tokens: value.prompt_tokens_details.cached_tokens,
    }
}

fn provider_error_hint(status: u16, body: &str) -> String {
    let lower = body.to_lowercase();
    let hint = if lower.contains("context")
        || lower.contains("maximum")
        || lower.contains("token")
        || lower.contains("too long")
        || lower.contains("length")
    {
        "模型上下文可能已超限。建议切回更大上下文模型，或让 Lan Code 压缩旧上下文后继续。"
    } else if status == 401
        || status == 403
        || lower.contains("api key")
        || lower.contains("unauthorized")
    {
        "模型鉴权失败。请检查 API Key、Base URL、供应商区域和账号额度。"
    } else if status == 404 || lower.contains("model") && lower.contains("not found") {
        "模型 ID 可能不可用。请在模型设置里重新获取模型列表，或手动确认模型名称。"
    } else if status == 429 || lower.contains("rate limit") {
        "模型服务限流或额度不足。可以稍后重试，或切换到其他已启用模型。"
    } else {
        "模型服务返回错误。请检查供应商配置和网络状态。"
    };
    format!("model request returned {status}: {body}\n\n{hint}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_null_usage_from_openai_compatible_providers() {
        let response: ChatResponse = serde_json::from_value(serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "ok"
                }
            }],
            "usage": null
        }))
        .expect("null usage should be accepted");

        assert!(response.usage.is_none());
    }

    #[test]
    fn provider_error_mentions_context_overflow() {
        let error = provider_error_hint(400, "maximum context length exceeded");
        assert!(error.contains("上下文"));
        assert!(error.contains("压缩"));
    }
}
