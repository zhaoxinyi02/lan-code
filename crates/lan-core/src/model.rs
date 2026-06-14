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
}

pub struct OpenAiCompatibleProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(base_url: String, api_key: String, model: String) -> Result<Self> {
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
        })
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ModelMessage],
    tools: Vec<ChatTool<'a>>,
    stream: bool,
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
    usage: ChatUsage,
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
            })
            .send()
            .await
            .context("model request failed")?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            bail!("model request returned {status}: {body}");
        }
        let response: ChatResponse =
            serde_json::from_str(&body).context("invalid model response JSON")?;
        let usage = TokenUsage {
            input_tokens: response.usage.prompt_tokens,
            output_tokens: response.usage.completion_tokens,
            total_tokens: if response.usage.total_tokens == 0 {
                response.usage.prompt_tokens + response.usage.completion_tokens
            } else {
                response.usage.total_tokens
            },
            cached_input_tokens: response.usage.prompt_tokens_details.cached_tokens,
        };
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
}
