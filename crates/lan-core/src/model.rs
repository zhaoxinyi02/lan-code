use std::{collections::BTreeMap, sync::Arc};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use lan_protocol::{TokenUsage, ToolCall, ToolDescriptor};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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

pub struct AnthropicProvider {
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

impl AnthropicProvider {
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
    stream_options: Option<ChatStreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
}

#[derive(Serialize)]
struct ChatStreamOptions {
    include_usage: bool,
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

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    messages: Vec<AnthropicMessage>,
    tools: Vec<AnthropicTool<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    max_tokens: u64,
}

#[derive(Serialize)]
struct AnthropicTool<'a> {
    name: &'a str,
    description: &'a str,
    input_schema: &'a Value,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: &'static str,
    content: Vec<AnthropicContent>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<AnthropicResponseContent>,
    #[serde(default)]
    usage: AnthropicUsage,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicResponseContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Default, Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
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
                stream_options: None,
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
        let input_tokens = estimate_messages_tokens(&request.messages);
        let mut usage = token_usage(response.usage.unwrap_or_default());
        let message = response
            .choices
            .into_iter()
            .next()
            .context("model response contained no choices")?
            .message;
        let text = message.content.clone().unwrap_or_default();
        let mut message = message;
        let mut tool_calls = message
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
        if tool_calls.is_empty()
            && let Some(parsed) = parse_raw_tool_call_text(&text)
        {
            message.content = None;
            message.tool_calls = Some(tool_calls_to_wire(&parsed));
            tool_calls = parsed;
        }
        let text = if tool_calls.is_empty() {
            text
        } else {
            String::new()
        };
        fill_missing_usage(&mut usage, input_tokens, &message);
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
        let input_tokens = estimate_messages_tokens(&request.messages);
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
                stream_options: Some(ChatStreamOptions {
                    include_usage: true,
                }),
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
        let mut emitted_text_len = 0usize;
        let mut suppress_raw_tool_text = false;
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
                    let trimmed = text.trim_start();
                    if trimmed.starts_with("<tool_call") {
                        suppress_raw_tool_text = true;
                    }
                    if !suppress_raw_tool_text && !"<tool_call".starts_with(trimmed) {
                        on_text(text[emitted_text_len..].to_string());
                        emitted_text_len = text.len();
                    }
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
        let mut tool_calls = wire_calls
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
        let mut message_tool_calls = (!wire_calls.is_empty()).then_some(wire_calls);
        if tool_calls.is_empty()
            && let Some(parsed) = parse_raw_tool_call_text(&text)
        {
            message_tool_calls = Some(tool_calls_to_wire(&parsed));
            text.clear();
            tool_calls = parsed;
        }
        let message = ModelMessage {
            role: ModelRole::Assistant,
            content: (!text.is_empty()).then_some(text.clone()),
            reasoning_content: (!reasoning.is_empty()).then_some(reasoning),
            tool_call_id: None,
            tool_calls: message_tool_calls,
        };
        fill_missing_usage(&mut usage, input_tokens, &message);
        Ok(ModelResponse {
            message,
            text,
            tool_calls,
            usage,
        })
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn model_name(&self) -> &str {
        &self.model
    }

    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse> {
        let (system, messages) = anthropic_messages(&request.messages)?;
        let tools = request
            .tools
            .iter()
            .map(|tool| AnthropicTool {
                name: &tool.name,
                description: &tool.description,
                input_schema: &tool.input_schema,
            })
            .collect::<Vec<_>>();
        let response = self
            .client
            .post(format!("{}/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&AnthropicRequest {
                model: &self.model,
                messages,
                tools,
                system,
                max_tokens: self.max_output_tokens.unwrap_or(8192),
            })
            .send()
            .await
            .context("anthropic model request failed")?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            bail!("{}", provider_error_hint(status.as_u16(), &body));
        }
        let response: AnthropicResponse =
            serde_json::from_str(&body).context("invalid Anthropic model response JSON")?;
        let mut text = String::new();
        let mut tool_calls = Vec::new();
        for item in response.content {
            match item {
                AnthropicResponseContent::Text { text: part } => {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&part);
                }
                AnthropicResponseContent::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCall {
                        id,
                        name,
                        arguments: input,
                    });
                }
                AnthropicResponseContent::Other => {}
            }
        }
        let message = ModelMessage {
            role: ModelRole::Assistant,
            content: (!text.is_empty()).then_some(text.clone()),
            reasoning_content: None,
            tool_call_id: None,
            tool_calls: (!tool_calls.is_empty()).then_some(tool_calls_to_wire(&tool_calls)),
        };
        Ok(ModelResponse {
            message,
            text,
            tool_calls,
            usage: TokenUsage {
                input_tokens: response.usage.input_tokens,
                output_tokens: response.usage.output_tokens,
                total_tokens: response.usage.input_tokens + response.usage.output_tokens,
                cached_input_tokens: response.usage.cache_read_input_tokens,
            },
        })
    }
}

fn tool_calls_to_wire(calls: &[ToolCall]) -> Vec<WireToolCall> {
    calls
        .iter()
        .map(|call| WireToolCall {
            id: call.id.clone(),
            kind: "function".into(),
            function: WireFunctionCall {
                name: call.name.clone(),
                arguments: call.arguments.to_string(),
            },
        })
        .collect()
}

fn anthropic_messages(
    messages: &[ModelMessage],
) -> Result<(Option<String>, Vec<AnthropicMessage>)> {
    let mut system = Vec::new();
    let mut result = Vec::new();
    for message in messages {
        match message.role {
            ModelRole::System => {
                if let Some(content) = &message.content {
                    system.push(content.clone());
                }
            }
            ModelRole::User => {
                result.push(AnthropicMessage {
                    role: "user",
                    content: vec![AnthropicContent::Text {
                        text: message.content.clone().unwrap_or_default(),
                    }],
                });
            }
            ModelRole::Assistant => {
                let mut content = Vec::new();
                if let Some(text) = &message.content
                    && !text.is_empty()
                {
                    content.push(AnthropicContent::Text { text: text.clone() });
                }
                for call in message.tool_calls.clone().unwrap_or_default() {
                    content.push(AnthropicContent::ToolUse {
                        id: call.id,
                        name: call.function.name,
                        input: serde_json::from_str(&call.function.arguments)
                            .context("assistant tool call arguments were not valid JSON")?,
                    });
                }
                if !content.is_empty() {
                    result.push(AnthropicMessage {
                        role: "assistant",
                        content,
                    });
                }
            }
            ModelRole::Tool => {
                result.push(AnthropicMessage {
                    role: "user",
                    content: vec![AnthropicContent::ToolResult {
                        tool_use_id: message.tool_call_id.clone().unwrap_or_default(),
                        content: message.content.clone().unwrap_or_default(),
                    }],
                });
            }
        }
    }
    Ok(((!system.is_empty()).then(|| system.join("\n\n")), result))
}

fn parse_raw_tool_call_text(text: &str) -> Option<Vec<ToolCall>> {
    if !text.contains("<tool_call") || !text.contains("<function=") {
        return None;
    }
    let mut calls = Vec::new();
    let mut rest = text;
    while let Some(function_start) = rest.find("<function=") {
        rest = &rest[function_start + "<function=".len()..];
        let name_end = rest.find('>')?;
        let name = rest[..name_end]
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
        if name.is_empty() {
            return None;
        }
        rest = &rest[name_end + 1..];
        let function_end = rest.find("</function>").unwrap_or(rest.len());
        let function_body = &rest[..function_end];
        let mut arguments = Map::new();
        let mut body_rest = function_body;
        while let Some(parameter_start) = body_rest.find("<parameter=") {
            body_rest = &body_rest[parameter_start + "<parameter=".len()..];
            let key_end = body_rest.find('>')?;
            let key = body_rest[..key_end]
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            body_rest = &body_rest[key_end + 1..];
            let value_end = body_rest.find("</parameter>")?;
            let raw_value = body_rest[..value_end].trim();
            body_rest = &body_rest[value_end + "</parameter>".len()..];
            let value = serde_json::from_str::<Value>(raw_value)
                .unwrap_or_else(|_| Value::String(strip_outer_quotes(raw_value).to_string()));
            arguments.insert(key, value);
        }
        calls.push(ToolCall {
            id: format!("raw-tool-call-{}", calls.len() + 1),
            name,
            arguments: Value::Object(arguments),
        });
        rest = if function_end < rest.len() {
            &rest[function_end + "</function>".len()..]
        } else {
            ""
        };
    }
    (!calls.is_empty()).then_some(calls)
}

fn strip_outer_quotes(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        &value[1..value.len() - 1]
    } else {
        value
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

fn fill_missing_usage(usage: &mut TokenUsage, input_tokens: u64, message: &ModelMessage) {
    if usage.input_tokens == 0 {
        usage.input_tokens = input_tokens;
    }
    if usage.output_tokens == 0 {
        usage.output_tokens = estimate_message_tokens(message);
    }
    if usage.total_tokens == 0 {
        usage.total_tokens = usage.input_tokens + usage.output_tokens;
    }
}

fn estimate_messages_tokens(messages: &[ModelMessage]) -> u64 {
    messages.iter().map(estimate_message_tokens).sum()
}

fn estimate_message_tokens(message: &ModelMessage) -> u64 {
    let content = estimate_text_tokens(message.content.as_deref().unwrap_or_default());
    let reasoning = estimate_text_tokens(message.reasoning_content.as_deref().unwrap_or_default());
    let tools = message
        .tool_calls
        .as_ref()
        .map(|calls| {
            calls
                .iter()
                .map(|call| {
                    estimate_text_tokens(&call.function.name)
                        + estimate_text_tokens(&call.function.arguments)
                })
                .sum::<u64>()
        })
        .unwrap_or_default();
    12 + content + reasoning + tools
}

fn estimate_text_tokens(text: &str) -> u64 {
    if text.is_empty() {
        return 0;
    }
    let mut ascii_run = 0u64;
    let mut tokens = 0u64;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch.is_ascii_punctuation() || ch == ' ' || ch == '\n' {
            ascii_run += 1;
        } else {
            if ascii_run > 0 {
                tokens += ascii_run.div_ceil(4);
                ascii_run = 0;
            }
            if !ch.is_whitespace() {
                tokens += 1;
            }
        }
    }
    tokens + ascii_run.div_ceil(4)
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
    fn estimates_usage_when_compatible_provider_omits_it() {
        let message = ModelMessage::text(ModelRole::Assistant, "这是一个 GLM 返回的中文回复。");
        let mut usage = TokenUsage::default();
        fill_missing_usage(&mut usage, 128, &message);

        assert_eq!(usage.input_tokens, 128);
        assert!(usage.output_tokens > 0);
        assert_eq!(usage.total_tokens, usage.input_tokens + usage.output_tokens);
    }

    #[test]
    fn provider_error_mentions_context_overflow() {
        let error = provider_error_hint(400, "maximum context length exceeded");
        assert!(error.contains("上下文"));
        assert!(error.contains("压缩"));
    }

    #[test]
    fn parses_raw_xml_style_tool_call_text() {
        let calls = parse_raw_tool_call_text(
            r#"<tool_call>
<function=replace_text>
<parameter=path>crates/lan-core/src/lib.rs</parameter>
<parameter=old_text>"before"</parameter>
<parameter=new_text>"after"</parameter>
</function>
</tool_call>"#,
        )
        .unwrap();
        assert_eq!(calls[0].name, "replace_text");
        assert_eq!(calls[0].arguments["path"], "crates/lan-core/src/lib.rs");
        assert_eq!(calls[0].arguments["old_text"], "before");
        assert_eq!(calls[0].arguments["new_text"], "after");
    }
}
