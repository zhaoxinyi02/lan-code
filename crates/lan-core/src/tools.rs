use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use lan_protocol::{RiskLevel, SessionId, ToolDescriptor};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{process::Command, time::Duration};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub session_id: SessionId,
    pub cwd: String,
    pub allow_unsandboxed_commands: bool,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn descriptor(&self) -> ToolDescriptor;
    async fn execute(&self, context: ToolContext, arguments: Value) -> Result<Value>;
}

pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, tool: Arc<dyn Tool>) {
        self.tools
            .write()
            .expect("tool registry lock poisoned")
            .insert(tool.descriptor().name, tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools
            .read()
            .expect("tool registry lock poisoned")
            .get(name)
            .cloned()
    }

    pub fn list(&self) -> Vec<ToolDescriptor> {
        self.tools
            .read()
            .expect("tool registry lock poisoned")
            .values()
            .map(|tool| tool.descriptor())
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "echo".into(),
            description: "Return the supplied text. Useful for protocol smoke tests.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"text": {"type": "string"}},
                "required": ["text"]
            }),
            risk: RiskLevel::ReadOnly,
        }
    }

    async fn execute(&self, _context: ToolContext, arguments: Value) -> Result<Value> {
        if !arguments.get("text").is_some_and(Value::is_string) {
            bail!("echo.text must be a string");
        }
        Ok(arguments)
    }
}

fn workspace_path(cwd: &str, relative: &str) -> Result<PathBuf> {
    let root = Path::new(cwd).canonicalize()?;
    let candidate = root.join(relative).canonicalize()?;
    if !candidate.starts_with(&root) {
        bail!("path escapes workspace");
    }
    Ok(candidate)
}

fn workspace_new_path(cwd: &str, relative: &str) -> Result<PathBuf> {
    let root = Path::new(cwd).canonicalize()?;
    let candidate = root.join(relative);
    let parent = candidate.parent().context("path has no parent")?;
    let existing_parent = parent.canonicalize()?;
    if !existing_parent.starts_with(&root) {
        bail!("path escapes workspace");
    }
    Ok(candidate)
}

fn string_arg<'a>(arguments: &'a Value, name: &str) -> Result<&'a str> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("{name} must be a string"))
}

fn image_mime(path: &Path) -> Result<&'static str> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "webp" => Ok("image/webp"),
        "gif" => Ok("image/gif"),
        _ => bail!("unsupported image type; use PNG, JPEG, WebP, or GIF"),
    }
}

#[derive(Clone)]
pub struct VisionTool {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl VisionTool {
    pub fn new(base_url: String, api_key: String, model: String) -> Result<Self> {
        if base_url.trim().is_empty() || model.trim().is_empty() {
            bail!("vision route requires base URL and model");
        }
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()?,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
        })
    }
}

#[async_trait]
impl Tool for VisionTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "analyze_image".into(),
            description: "Analyze an image inside the workspace using the configured vision model. The image is sent to an external model provider.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Workspace-relative image path"},
                    "prompt": {"type": "string", "description": "Question or analysis instructions"}
                },
                "required": ["path", "prompt"]
            }),
            risk: RiskLevel::ExternalSideEffect,
        }
    }

    async fn execute(&self, context: ToolContext, arguments: Value) -> Result<Value> {
        let path = workspace_path(&context.cwd, string_arg(&arguments, "path")?)?;
        let prompt = string_arg(&arguments, "prompt")?;
        let bytes = fs::read(&path)?;
        if bytes.len() > 20 * 1024 * 1024 {
            bail!("image exceeds 20 MiB limit");
        }
        let data_url = format!(
            "data:{};base64,{}",
            image_mime(&path)?,
            BASE64.encode(bytes)
        );
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": self.model,
                "messages": [{"role": "user", "content": [
                    {"type": "text", "text": prompt},
                    {"type": "image_url", "image_url": {"url": data_url}}
                ]}]
            }))
            .send()
            .await
            .context("vision request failed")?
            .error_for_status()
            .context("vision provider returned an error")?
            .json::<Value>()
            .await
            .context("invalid vision response JSON")?;
        let text = response["choices"][0]["message"]["content"]
            .as_str()
            .context("vision provider returned no text")?;
        Ok(json!({"path": path.display().to_string(), "model": self.model, "text": text}))
    }
}

#[derive(Clone)]
pub struct ImageGenerationTool {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
}

#[derive(Deserialize)]
struct GeneratedImageResponse {
    data: Vec<GeneratedImage>,
}

#[derive(Deserialize)]
struct GeneratedImage {
    b64_json: Option<String>,
    url: Option<String>,
}

impl ImageGenerationTool {
    pub fn new(base_url: String, api_key: String, model: String) -> Result<Self> {
        if base_url.trim().is_empty() || model.trim().is_empty() {
            bail!("image generation route requires base URL and model");
        }
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(180))
                .build()?,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
        })
    }
}

#[async_trait]
impl Tool for ImageGenerationTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "generate_image".into(),
            description: "Generate an image with the configured image model and save it inside the workspace. This contacts an external model provider.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": {"type": "string"},
                    "output_path": {"type": "string", "description": "Workspace-relative PNG output path in an existing directory"}
                },
                "required": ["prompt", "output_path"]
            }),
            risk: RiskLevel::ExternalSideEffect,
        }
    }

    async fn execute(&self, context: ToolContext, arguments: Value) -> Result<Value> {
        let prompt = string_arg(&arguments, "prompt")?;
        let output_path = workspace_new_path(&context.cwd, string_arg(&arguments, "output_path")?)?;
        if output_path.exists() {
            bail!("refusing to overwrite existing image");
        }
        let endpoint = if self.base_url.ends_with("/images/generations") {
            self.base_url.clone()
        } else {
            format!("{}/images/generations", self.base_url)
        };
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(&self.api_key)
            .json(&json!({"model": self.model, "prompt": prompt, "size": "1024x1024"}))
            .send()
            .await
            .context("image generation request failed")?
            .error_for_status()
            .context("image generation provider returned an error")?
            .json::<GeneratedImageResponse>()
            .await
            .context("invalid image generation response JSON")?;
        let image = response
            .data
            .into_iter()
            .next()
            .context("provider returned no image")?;
        let bytes = if let Some(data) = image.b64_json {
            BASE64
                .decode(data)
                .context("invalid generated image base64")?
        } else if let Some(url) = image.url {
            self.client
                .get(url)
                .send()
                .await
                .context("generated image download failed")?
                .error_for_status()
                .context("generated image download returned an error")?
                .bytes()
                .await?
                .to_vec()
        } else {
            bail!("provider returned no image data");
        };
        fs::write(&output_path, &bytes)?;
        Ok(json!({
            "path": output_path.display().to_string(),
            "model": self.model,
            "bytesWritten": bytes.len()
        }))
    }
}

pub struct ListFilesTool;

#[async_trait]
impl Tool for ListFilesTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "list_files".into(),
            description:
                "List workspace files recursively. Use a relative path and optional limit.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Relative directory, usually ."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500}
                },
                "required": ["path"]
            }),
            risk: RiskLevel::ReadOnly,
        }
    }

    async fn execute(&self, context: ToolContext, arguments: Value) -> Result<Value> {
        let path = workspace_path(&context.cwd, string_arg(&arguments, "path")?)?;
        let root = Path::new(&context.cwd).canonicalize()?;
        let limit = arguments
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(200)
            .min(500) as usize;
        let files = WalkDir::new(path)
            .into_iter()
            .filter_entry(|entry| entry.file_name() != ".git" && entry.file_name() != "target")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .take(limit)
            .map(|entry| {
                entry
                    .path()
                    .strip_prefix(&root)
                    .unwrap_or(entry.path())
                    .display()
                    .to_string()
            })
            .collect::<Vec<_>>();
        Ok(json!({"files": files, "truncated": files.len() == limit}))
    }
}

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "read_file".into(),
            description: "Read a UTF-8 text file inside the workspace, with bounded output.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            }),
            risk: RiskLevel::ReadOnly,
        }
    }

    async fn execute(&self, context: ToolContext, arguments: Value) -> Result<Value> {
        let path = workspace_path(&context.cwd, string_arg(&arguments, "path")?)?;
        let bytes = fs::read(&path)?;
        if bytes.len() > 128 * 1024 {
            bail!("file exceeds 128 KiB read limit");
        }
        let content = String::from_utf8(bytes).context("file is not UTF-8 text")?;
        Ok(json!({"path": path.display().to_string(), "content": content}))
    }
}

pub struct SearchTextTool;

#[async_trait]
impl Tool for SearchTextTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "search_text".into(),
            description: "Search UTF-8 workspace files for a literal text pattern.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "path": {"type": "string", "description": "Relative directory, usually ."}
                },
                "required": ["query", "path"]
            }),
            risk: RiskLevel::ReadOnly,
        }
    }

    async fn execute(&self, context: ToolContext, arguments: Value) -> Result<Value> {
        let query = string_arg(&arguments, "query")?;
        let path = workspace_path(&context.cwd, string_arg(&arguments, "path")?)?;
        let root = Path::new(&context.cwd).canonicalize()?;
        let mut matches = Vec::new();
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_entry(|entry| entry.file_name() != ".git" && entry.file_name() != "target")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            if matches.len() >= 100 {
                break;
            }
            let Ok(bytes) = fs::read(entry.path()) else {
                continue;
            };
            if bytes.len() > 512 * 1024 {
                continue;
            }
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            for (index, line) in text.lines().enumerate() {
                if line.contains(query) {
                    matches.push(json!({
                        "path": entry.path().strip_prefix(&root).unwrap_or(entry.path()).display().to_string(),
                        "line": index + 1,
                        "text": line.chars().take(300).collect::<String>()
                    }));
                    if matches.len() >= 100 {
                        break;
                    }
                }
            }
        }
        Ok(json!({"matches": matches, "truncated": matches.len() == 100}))
    }
}

pub struct ReplaceTextTool;

#[async_trait]
impl Tool for ReplaceTextTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "replace_text".into(),
            description: "Replace one exact, uniquely occurring text block in an existing UTF-8 workspace file. The edit is rejected if the old text is missing or ambiguous.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "old_text": {"type": "string"},
                    "new_text": {"type": "string"}
                },
                "required": ["path", "old_text", "new_text"]
            }),
            risk: RiskLevel::WorkspaceWrite,
        }
    }

    async fn execute(&self, context: ToolContext, arguments: Value) -> Result<Value> {
        let path = workspace_path(&context.cwd, string_arg(&arguments, "path")?)?;
        let old_text = string_arg(&arguments, "old_text")?;
        let new_text = string_arg(&arguments, "new_text")?;
        if old_text.is_empty() {
            bail!("old_text cannot be empty");
        }
        let bytes = fs::read(&path)?;
        if bytes.len() > 512 * 1024 {
            bail!("file exceeds 512 KiB edit limit");
        }
        let content = String::from_utf8(bytes).context("file is not UTF-8 text")?;
        let occurrences = content.match_indices(old_text).count();
        if occurrences != 1 {
            bail!("old_text must occur exactly once, found {occurrences}");
        }
        let updated = content.replacen(old_text, new_text, 1);
        fs::write(&path, updated.as_bytes())?;
        Ok(json!({
            "path": path.display().to_string(),
            "bytesWritten": updated.len()
        }))
    }
}

pub struct CreateFileTool;

#[async_trait]
impl Tool for CreateFileTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "create_file".into(),
            description:
                "Create one new UTF-8 workspace file. Refuses to overwrite existing files.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
            risk: RiskLevel::WorkspaceWrite,
        }
    }

    async fn execute(&self, context: ToolContext, arguments: Value) -> Result<Value> {
        let path = workspace_new_path(&context.cwd, string_arg(&arguments, "path")?)?;
        let content = string_arg(&arguments, "content")?;
        if path.exists() {
            bail!("refusing to overwrite existing file");
        }
        if content.len() > 512 * 1024 {
            bail!("content exceeds 512 KiB create limit");
        }
        fs::write(&path, content.as_bytes())?;
        Ok(json!({"path": path.display().to_string(), "bytesWritten": content.len()}))
    }
}

pub struct ApplyEditsTool;

#[async_trait]
impl Tool for ApplyEditsTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "apply_edits".into(),
            description: "Apply multiple exact text replacements. Every old_text must occur exactly once; all edits are validated before any file is written.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "edits": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 50,
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": {"type": "string"},
                                "old_text": {"type": "string"},
                                "new_text": {"type": "string"}
                            },
                            "required": ["path", "old_text", "new_text"]
                        }
                    }
                },
                "required": ["edits"]
            }),
            risk: RiskLevel::WorkspaceWrite,
        }
    }

    async fn execute(&self, context: ToolContext, arguments: Value) -> Result<Value> {
        let edits = arguments
            .get("edits")
            .and_then(Value::as_array)
            .context("edits must be an array")?;
        if edits.is_empty() || edits.len() > 50 {
            bail!("edits must contain between 1 and 50 items");
        }
        let mut seen = HashSet::new();
        let mut prepared = Vec::new();
        for edit in edits {
            let relative = string_arg(edit, "path")?;
            if !seen.insert(relative.to_string()) {
                bail!("multiple edits for the same path are not allowed");
            }
            let path = workspace_path(&context.cwd, relative)?;
            let old_text = string_arg(edit, "old_text")?;
            let new_text = string_arg(edit, "new_text")?;
            if old_text.is_empty() {
                bail!("old_text cannot be empty");
            }
            let bytes = fs::read(&path)?;
            if bytes.len() > 512 * 1024 {
                bail!("file exceeds 512 KiB edit limit: {relative}");
            }
            let content = String::from_utf8(bytes).context("file is not UTF-8 text")?;
            let occurrences = content.match_indices(old_text).count();
            if occurrences != 1 {
                bail!("{relative}: old_text must occur exactly once, found {occurrences}");
            }
            prepared.push((path, content.replacen(old_text, new_text, 1)));
        }
        let mut changed = Vec::new();
        for (path, content) in prepared {
            fs::write(&path, content.as_bytes())?;
            changed.push(path.display().to_string());
        }
        Ok(json!({"changedFiles": changed}))
    }
}

async fn run_git(cwd: &str, args: &[&str]) -> Result<Value> {
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        Command::new("git")
            .args(args)
            .current_dir(Path::new(cwd).canonicalize()?)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("git command timed out")??;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
    let bounded = stdout.chars().take(128 * 1024).collect::<String>();
    Ok(json!({
        "output": bounded,
        "truncated": stdout.len() > bounded.len()
    }))
}

pub struct GitStatusTool;

#[async_trait]
impl Tool for GitStatusTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "git_status".into(),
            description: "Show concise Git working-tree and branch status.".into(),
            input_schema: json!({"type": "object", "properties": {}}),
            risk: RiskLevel::ReadOnly,
        }
    }

    async fn execute(&self, context: ToolContext, _arguments: Value) -> Result<Value> {
        run_git(&context.cwd, &["status", "--short", "--branch"]).await
    }
}

pub struct GitDiffTool;

#[async_trait]
impl Tool for GitDiffTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "git_diff".into(),
            description: "Show the current unstaged Git diff for review.".into(),
            input_schema: json!({"type": "object", "properties": {}}),
            risk: RiskLevel::ReadOnly,
        }
    }

    async fn execute(&self, context: ToolContext, _arguments: Value) -> Result<Value> {
        run_git(&context.cwd, &["diff", "--no-ext-diff", "--"]).await
    }
}

pub struct RunCommandTool;

#[async_trait]
impl Tool for RunCommandTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "run_command".into(),
            description: "Run one executable directly in the workspace without shell string parsing. This has full host-process authority and always requires explicit full-access permission.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "program": {"type": "string"},
                    "args": {"type": "array", "items": {"type": "string"}},
                    "timeout_seconds": {"type": "integer", "minimum": 1, "maximum": 120}
                },
                "required": ["program", "args"]
            }),
            risk: RiskLevel::FullAccess,
        }
    }

    async fn execute(&self, context: ToolContext, arguments: Value) -> Result<Value> {
        if !context.allow_unsandboxed_commands
            && std::env::var("LAN_ALLOW_UNSANDBOXED_COMMANDS").as_deref() != Ok("1")
        {
            bail!(
                "command execution is not sandboxed; set LAN_ALLOW_UNSANDBOXED_COMMANDS=1 to explicitly accept full host-process authority"
            );
        }
        let program = string_arg(&arguments, "program")?;
        let args = arguments
            .get("args")
            .and_then(Value::as_array)
            .context("args must be an array")?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .context("each arg must be a string")
            })
            .collect::<Result<Vec<_>>>()?;
        let timeout_seconds = arguments
            .get("timeout_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(30)
            .clamp(1, 120);
        let output = tokio::time::timeout(
            Duration::from_secs(timeout_seconds),
            Command::new(program)
                .args(args)
                .current_dir(Path::new(&context.cwd).canonicalize()?)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .context("command timed out")??;
        fn bounded(bytes: &[u8]) -> String {
            String::from_utf8_lossy(&bytes[..bytes.len().min(64 * 1024)]).into_owned()
        }
        Ok(json!({
            "success": output.status.success(),
            "exitCode": output.status.code(),
            "stdout": bounded(&output.stdout),
            "stderr": bounded(&output.stderr),
            "stdoutTruncated": output.stdout.len() > 64 * 1024,
            "stderrTruncated": output.stderr.len() > 64 * 1024
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use lan_protocol::SessionId;
    use serde_json::json;
    use uuid::Uuid;

    use super::{
        ApplyEditsTool, ImageGenerationTool, ReplaceTextTool, RunCommandTool, Tool, ToolContext,
        VisionTool,
    };

    #[test]
    fn multimodal_tools_describe_external_side_effects() {
        let vision = VisionTool::new(
            "https://example.com/v1".into(),
            "test".into(),
            "vision-model".into(),
        )
        .unwrap();
        let image = ImageGenerationTool::new(
            "https://example.com/v1".into(),
            "test".into(),
            "image-model".into(),
        )
        .unwrap();
        assert_eq!(vision.descriptor().name, "analyze_image");
        assert_eq!(
            vision.descriptor().risk,
            lan_protocol::RiskLevel::ExternalSideEffect
        );
        assert_eq!(image.descriptor().name, "generate_image");
        assert_eq!(
            image.descriptor().risk,
            lan_protocol::RiskLevel::ExternalSideEffect
        );
    }

    #[tokio::test]
    async fn replace_text_requires_one_exact_match() {
        let root = std::env::temp_dir().join(format!("lan-core-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("sample.txt");
        fs::write(&file, "before\n").unwrap();
        let tool = ReplaceTextTool;
        tool.execute(
            ToolContext {
                session_id: SessionId::new_v4(),
                cwd: root.display().to_string(),
                allow_unsandboxed_commands: false,
            },
            json!({
                "path": "sample.txt",
                "old_text": "before",
                "new_text": "after"
            }),
        )
        .await
        .unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "after\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn apply_edits_changes_multiple_files() {
        let root = std::env::temp_dir().join(format!("lan-core-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("first.txt");
        let second = root.join("second.txt");
        fs::write(&first, "hello world").unwrap();
        fs::write(&second, "goodbye world").unwrap();

        let output = ApplyEditsTool
            .execute(
                ToolContext {
                    session_id: SessionId::new_v4(),
                    cwd: root.display().to_string(),
                    allow_unsandboxed_commands: false,
                },
                json!({
                    "edits": [
                        {"path": "first.txt", "old_text": "hello", "new_text": "hi"},
                        {"path": "second.txt", "old_text": "goodbye", "new_text": "farewell"}
                    ]
                }),
            )
            .await
            .unwrap();

        assert_eq!(output["changedFiles"].as_array().unwrap().len(), 2);
        assert_eq!(fs::read_to_string(&first).unwrap(), "hi world");
        assert_eq!(fs::read_to_string(&second).unwrap(), "farewell world");
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn apply_edits_validation_failure_writes_nothing() {
        let root = std::env::temp_dir().join(format!("lan-core-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let first = root.join("first.txt");
        let second = root.join("second.txt");
        fs::write(&first, "hello world").unwrap();
        fs::write(&second, "goodbye world").unwrap();

        let result = ApplyEditsTool
            .execute(
                ToolContext {
                    session_id: SessionId::new_v4(),
                    cwd: root.display().to_string(),
                    allow_unsandboxed_commands: false,
                },
                json!({
                    "edits": [
                        {"path": "first.txt", "old_text": "hello", "new_text": "hi"},
                        {"path": "second.txt", "old_text": "missing", "new_text": "farewell"}
                    ]
                }),
            )
            .await;

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&first).unwrap(), "hello world");
        assert_eq!(fs::read_to_string(&second).unwrap(), "goodbye world");
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn run_command_requires_explicit_unsandboxed_gate() {
        let tool = RunCommandTool;
        let error = tool
            .execute(
                ToolContext {
                    session_id: SessionId::new_v4(),
                    cwd: std::env::current_dir().unwrap().display().to_string(),
                    allow_unsandboxed_commands: false,
                },
                json!({
                    "program": "rustc",
                    "args": ["--version"],
                    "timeout_seconds": 10
                }),
            )
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("LAN_ALLOW_UNSANDBOXED_COMMANDS=1")
        );
    }

    #[tokio::test]
    async fn run_command_accepts_explicit_full_access_context() {
        let output = RunCommandTool
            .execute(
                ToolContext {
                    session_id: SessionId::new_v4(),
                    cwd: std::env::current_dir().unwrap().display().to_string(),
                    allow_unsandboxed_commands: true,
                },
                json!({
                    "program": "rustc",
                    "args": ["--version"],
                    "timeout_seconds": 10
                }),
            )
            .await
            .unwrap();
        assert_eq!(output["success"], true);
    }
}
