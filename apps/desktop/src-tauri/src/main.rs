#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::Instant,
};

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use lan_core::{
    AgentCore, ImageGenerationTool, ModelMessage, ModelProvider, ModelRequest,
    OpenAiCompatibleProvider, SqliteStore, VisionTool,
};
use lan_protocol::{
    ApprovalDecision, ApprovalMode, ApprovalRequest, CoreEvent, RiskLevel, Session, SessionId,
    ToolDescriptor, TurnResult,
};
use portable_pty::{Child, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::RwLock;
use uuid::Uuid;
use walkdir::WalkDir;

fn hidden_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn is_local_provider(provider: &str) -> bool {
    matches!(provider, "ollama" | "lmstudio")
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopProject {
    name: String,
    path: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderProfile {
    id: String,
    name: String,
    provider: String,
    base_url: String,
    model: String,
    api_key: String,
    input_price_per_million: f64,
    output_price_per_million: f64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ModelCapabilities {
    image_input: bool,
    image_output: bool,
    audio_input: bool,
    audio_output: bool,
    tool_calling: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct CapabilityRoute {
    enabled: bool,
    inherit_main_model: bool,
    provider: String,
    base_url: String,
    model: String,
    api_key: String,
}

impl Default for CapabilityRoute {
    fn default() -> Self {
        Self {
            enabled: false,
            inherit_main_model: true,
            provider: "custom".into(),
            base_url: String::new(),
            model: String::new(),
            api_key: String::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct DesktopSettings {
    provider: String,
    base_url: String,
    model: String,
    api_key: String,
    workspace: String,
    data_dir: String,
    approval_mode: ApprovalMode,
    max_provider_rounds: usize,
    input_price_per_million: f64,
    output_price_per_million: f64,
    projects: Vec<DesktopProject>,
    provider_profiles: Vec<ProviderProfile>,
    model_capabilities: ModelCapabilities,
    vision_route: CapabilityRoute,
    image_generation_route: CapabilityRoute,
    speech_to_text_route: CapabilityRoute,
    text_to_speech_route: CapabilityRoute,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            provider: "deepseek".into(),
            base_url: "https://api.deepseek.com".into(),
            model: "deepseek-v4-pro".into(),
            api_key: String::new(),
            workspace: std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .display()
                .to_string(),
            data_dir: default_data_dir().display().to_string(),
            approval_mode: ApprovalMode::ReadOnly,
            max_provider_rounds: 48,
            input_price_per_million: 0.0,
            output_price_per_million: 0.0,
            projects: Vec::new(),
            provider_profiles: Vec::new(),
            model_capabilities: ModelCapabilities::default(),
            vision_route: CapabilityRoute::default(),
            image_generation_route: CapabilityRoute::default(),
            speech_to_text_route: CapabilityRoute::default(),
            text_to_speech_route: CapabilityRoute::default(),
        }
    }
}

struct AppState {
    core: RwLock<Arc<AgentCore>>,
    settings: RwLock<DesktopSettings>,
    data_dir: RwLock<PathBuf>,
    terminal: Mutex<Option<TerminalSession>>,
}

struct TerminalSession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderTestResult {
    model: String,
    latency_ms: u128,
    text_response: String,
    tool_call_supported: bool,
    capabilities: ModelCapabilities,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceEntry {
    path: String,
    name: String,
    is_dir: bool,
    depth: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceFile {
    path: String,
    content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceSearchMatch {
    path: String,
    line: usize,
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GitChange {
    status: String,
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GitCommit {
    hash: String,
    subject: String,
    author: String,
    relative_time: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GitOverview {
    is_repository: bool,
    branch: String,
    additions: u64,
    deletions: u64,
    commits: Vec<GitCommit>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateInfo {
    current_version: String,
    latest_version: String,
    available: bool,
    release_url: String,
    installer_url: Option<String>,
    installer_name: Option<String>,
    published_at: Option<String>,
    notes: Option<String>,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    published_at: Option<String>,
    body: Option<String>,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct ImageGenerationResponse {
    data: Vec<GeneratedImage>,
}

#[derive(Deserialize)]
struct GeneratedImage {
    b64_json: Option<String>,
    url: Option<String>,
}

fn user_home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir)
}

fn default_data_dir() -> PathBuf {
    user_home().join(".lancode")
}

fn location_file() -> PathBuf {
    user_home().join(".lancode-location")
}

fn configured_data_dir() -> PathBuf {
    fs::read_to_string(location_file())
        .ok()
        .map(|value| PathBuf::from(value.trim()))
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(default_data_dir)
}

fn migrate_legacy_data_dir(target: &Path) {
    let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) else {
        return;
    };
    let legacy = app_data.join("Lan Code");
    if target.join("settings.json").exists() || !legacy.join("settings.json").exists() {
        return;
    }
    let _ = fs::create_dir_all(target);
    for name in [
        "settings.json",
        "lan-code.sqlite",
        "lan-code.sqlite-wal",
        "lan-code.sqlite-shm",
    ] {
        let source = legacy.join(name);
        if source.exists() {
            let _ = fs::copy(source, target.join(name));
        }
    }
}

fn load_settings(data_dir: &Path) -> DesktopSettings {
    let mut settings: DesktopSettings = fs::read_to_string(data_dir.join("settings.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    if settings.api_key.is_empty() {
        settings.api_key = keyring::Entry::new("Lan Code", "provider-api-key")
            .ok()
            .and_then(|entry| entry.get_password().ok())
            .unwrap_or_default();
    }
    settings.data_dir = data_dir.display().to_string();
    if settings.projects.is_empty() && Path::new(&settings.workspace).is_dir() {
        let path = PathBuf::from(&settings.workspace);
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("当前项目")
            .to_string();
        settings.projects.push(DesktopProject {
            name,
            path: settings.workspace.clone(),
        });
    }
    settings
}

fn persist_bootstrap_settings(data_dir: &Path, settings: &DesktopSettings) {
    let _ = fs::create_dir_all(data_dir);
    if let Ok(bytes) = serde_json::to_vec_pretty(settings) {
        let _ = fs::write(data_dir.join("settings.json"), bytes);
    }
}

fn infer_model_capabilities(model: &str, tool_calling: bool) -> ModelCapabilities {
    let model = model.to_ascii_lowercase();
    let image_input = [
        "vision", "gpt-4o", "gpt-4.1", "gemini", "claude-3", "claude-4", "qwen-vl", "qvq",
        "pixtral", "llava",
    ]
    .iter()
    .any(|hint| model.contains(hint));
    let image_output = ["gpt-image", "dall-e", "imagen", "flux", "sdxl", "seedream"]
        .iter()
        .any(|hint| model.contains(hint));
    let audio_input = ["audio", "whisper", "transcribe"]
        .iter()
        .any(|hint| model.contains(hint));
    let audio_output = ["audio", "tts", "speech"]
        .iter()
        .any(|hint| model.contains(hint));
    ModelCapabilities {
        image_input,
        image_output,
        audio_input,
        audio_output,
        tool_calling,
    }
}

fn workspace_path(settings: &DesktopSettings, relative: &str) -> Result<PathBuf, String> {
    let root = PathBuf::from(&settings.workspace)
        .canonicalize()
        .map_err(|error| format!("工作区不可用：{error}"))?;
    let candidate = root.join(relative);
    let resolved = if candidate.exists() {
        candidate
            .canonicalize()
            .map_err(|error| error.to_string())?
    } else {
        let parent = candidate.parent().ok_or("文件路径无效")?;
        parent
            .canonicalize()
            .map_err(|error| error.to_string())?
            .join(candidate.file_name().ok_or("文件路径无效")?)
    };
    if !resolved.starts_with(&root) {
        return Err("拒绝访问工作区之外的路径".into());
    }
    Ok(resolved)
}

fn mutable_workspace_path(settings: &DesktopSettings, relative: &str) -> Result<PathBuf, String> {
    let resolved = workspace_path(settings, relative)?;
    let root = PathBuf::from(&settings.workspace)
        .canonicalize()
        .map_err(|error| format!("工作区不可用：{error}"))?;
    if resolved == root {
        return Err("拒绝修改工作区根目录".into());
    }
    Ok(resolved)
}

fn resolved_route(
    settings: &DesktopSettings,
    route: &CapabilityRoute,
    main_supported: bool,
) -> Result<(String, String, String), String> {
    if route.inherit_main_model && main_supported {
        return Ok((
            settings.base_url.clone(),
            settings.api_key.clone(),
            settings.model.clone(),
        ));
    }
    if !route.enabled || route.model.trim().is_empty() {
        return Err("当前主模型不支持该能力，请在设置中配置专用模型".into());
    }
    Ok((
        if route.base_url.trim().is_empty() {
            settings.base_url.clone()
        } else {
            route.base_url.clone()
        },
        if route.api_key.trim().is_empty() {
            settings.api_key.clone()
        } else {
            route.api_key.clone()
        },
        route.model.clone(),
    ))
}

fn image_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/png",
    }
}

fn build_core(settings: &DesktopSettings, data_dir: &Path) -> Result<AgentCore, String> {
    fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    let store =
        SqliteStore::open(data_dir.join("lan-code.sqlite")).map_err(|error| error.to_string())?;
    let core = if settings.api_key.trim().is_empty() && !is_local_provider(&settings.provider) {
        AgentCore::with_store(store)
            .map(|core| core.with_max_provider_rounds(settings.max_provider_rounds))
            .map_err(|error| error.to_string())?
    } else {
        let api_key = if settings.api_key.trim().is_empty() {
            "ollama".into()
        } else {
            settings.api_key.clone()
        };
        let provider = OpenAiCompatibleProvider::new(
            settings.base_url.clone(),
            api_key,
            settings.model.clone(),
        )
        .map_err(|error| error.to_string())?;
        AgentCore::with_provider_and_store(Arc::new(provider), store)
            .map(|core| core.with_max_provider_rounds(settings.max_provider_rounds))
            .map_err(|error| error.to_string())?
    };
    if let Ok((base_url, api_key, model)) = resolved_route(
        settings,
        &settings.vision_route,
        settings.model_capabilities.image_input,
    ) {
        core.register_tool(Arc::new(
            VisionTool::new(base_url, api_key, model).map_err(|error| error.to_string())?,
        ));
    }
    if let Ok((base_url, api_key, model)) = resolved_route(
        settings,
        &settings.image_generation_route,
        settings.model_capabilities.image_output,
    ) {
        core.register_tool(Arc::new(
            ImageGenerationTool::new(base_url, api_key, model)
                .map_err(|error| error.to_string())?,
        ));
    }
    Ok(core)
}

async fn core(state: &State<'_, AppState>) -> Arc<AgentCore> {
    state.core.read().await.clone()
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<DesktopSettings, String> {
    Ok(state.settings.read().await.clone())
}

#[tauri::command]
async fn save_settings(
    mut settings: DesktopSettings,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if settings.base_url.trim().is_empty() || settings.model.trim().is_empty() {
        return Err("API 地址和模型名称不能为空".into());
    }
    if !Path::new(&settings.workspace).is_dir() {
        return Err("当前工作区目录不存在".into());
    }
    settings.max_provider_rounds = settings.max_provider_rounds.clamp(4, 256);
    settings
        .projects
        .retain(|project| Path::new(&project.path).is_dir());
    let next_data_dir = PathBuf::from(settings.data_dir.trim());
    if next_data_dir.as_os_str().is_empty() {
        return Err("数据目录不能为空".into());
    }
    fs::create_dir_all(&next_data_dir).map_err(|error| error.to_string())?;
    fs::write(
        next_data_dir.join("settings.json"),
        serde_json::to_vec_pretty(&settings).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::write(location_file(), next_data_dir.display().to_string())
        .map_err(|error| error.to_string())?;
    let next_core = build_core(&settings, &next_data_dir)?;
    *state.core.write().await = Arc::new(next_core);
    *state.settings.write().await = settings;
    *state.data_dir.write().await = next_data_dir;
    Ok(())
}

#[tauri::command]
async fn test_provider(settings: DesktopSettings) -> Result<ProviderTestResult, String> {
    if settings.api_key.trim().is_empty() && !is_local_provider(&settings.provider) {
        return Err("请先填写 API Key".into());
    }
    let api_key = if settings.api_key.trim().is_empty() {
        "ollama".into()
    } else {
        settings.api_key
    };
    let provider = OpenAiCompatibleProvider::new(settings.base_url, api_key, settings.model)
        .map_err(|error| error.to_string())?;
    let started = Instant::now();
    let response = provider
        .complete(ModelRequest {
            messages: vec![ModelMessage::text(
                lan_core::ModelRole::User,
                "请简短回复连接成功，然后调用 capability_probe 工具。",
            )],
            tools: vec![ToolDescriptor {
                name: "capability_probe".into(),
                description: "用于检测模型是否支持工具调用".into(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {"ok": {"type": "boolean"}},
                    "required": ["ok"]
                }),
                risk: RiskLevel::ReadOnly,
            }],
        })
        .await
        .map_err(|error| error.to_string())?;
    Ok(ProviderTestResult {
        model: provider.model_name().to_string(),
        latency_ms: started.elapsed().as_millis(),
        text_response: response.text,
        tool_call_supported: response
            .tool_calls
            .iter()
            .any(|call| call.name == "capability_probe"),
        capabilities: infer_model_capabilities(
            provider.model_name(),
            response
                .tool_calls
                .iter()
                .any(|call| call.name == "capability_probe"),
        ),
    })
}

#[tauri::command]
async fn list_workspace_files(state: State<'_, AppState>) -> Result<Vec<WorkspaceEntry>, String> {
    let settings = state.settings.read().await;
    let root = PathBuf::from(&settings.workspace);
    let ignored = [".git", "node_modules", "target", "dist", ".idea", ".vscode"];
    let mut entries = Vec::new();
    for entry in WalkDir::new(&root)
        .max_depth(8)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0 || !ignored.contains(&entry.file_name().to_string_lossy().as_ref())
        })
        .filter_map(Result::ok)
        .skip(1)
        .take(4000)
    {
        let relative = entry
            .path()
            .strip_prefix(&root)
            .map_err(|error| error.to_string())?
            .display()
            .to_string();
        entries.push(WorkspaceEntry {
            path: relative,
            name: entry.file_name().to_string_lossy().to_string(),
            is_dir: entry.file_type().is_dir(),
            depth: entry.depth() - 1,
        });
    }
    Ok(entries)
}

#[tauri::command]
async fn search_workspace(
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceSearchMatch>, String> {
    let query = query.trim().to_lowercase();
    if query.len() < 2 {
        return Err("搜索内容至少需要 2 个字符".into());
    }
    let settings = state.settings.read().await;
    let root = PathBuf::from(&settings.workspace);
    let ignored = [".git", "node_modules", "target", "dist", ".idea", ".vscode"];
    let mut matches = Vec::new();
    for entry in WalkDir::new(&root)
        .max_depth(12)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0 || !ignored.contains(&entry.file_name().to_string_lossy().as_ref())
        })
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        if entry
            .metadata()
            .map(|metadata| metadata.len())
            .unwrap_or(u64::MAX)
            > 2 * 1024 * 1024
        {
            continue;
        }
        let Ok(content) = fs::read_to_string(entry.path()) else {
            continue;
        };
        for (index, line) in content.lines().enumerate() {
            if line.to_lowercase().contains(&query) {
                matches.push(WorkspaceSearchMatch {
                    path: entry
                        .path()
                        .strip_prefix(&root)
                        .map_err(|error| error.to_string())?
                        .display()
                        .to_string(),
                    line: index + 1,
                    text: line.trim().chars().take(180).collect(),
                });
                if matches.len() >= 200 {
                    return Ok(matches);
                }
            }
        }
    }
    Ok(matches)
}

#[tauri::command]
async fn read_workspace_file(
    path: String,
    state: State<'_, AppState>,
) -> Result<WorkspaceFile, String> {
    let settings = state.settings.read().await;
    let resolved = mutable_workspace_path(&settings, &path)?;
    let metadata = fs::metadata(&resolved).map_err(|error| error.to_string())?;
    if metadata.len() > 2 * 1024 * 1024 {
        return Err("文件超过 2MB，暂不在内置编辑器中打开".into());
    }
    Ok(WorkspaceFile {
        path,
        content: fs::read_to_string(resolved).map_err(|_| "该文件不是可编辑文本文件")?,
    })
}

#[tauri::command]
async fn write_workspace_file(
    path: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let settings = state.settings.read().await;
    let resolved = mutable_workspace_path(&settings, &path)?;
    fs::write(resolved, content).map_err(|error| error.to_string())
}

#[tauri::command]
async fn create_workspace_entry(
    path: String,
    is_dir: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let settings = state.settings.read().await;
    let resolved = mutable_workspace_path(&settings, &path)?;
    if resolved.exists() {
        return Err("同名文件或文件夹已存在".into());
    }
    if is_dir {
        fs::create_dir_all(resolved).map_err(|error| error.to_string())
    } else {
        let parent = resolved.parent().ok_or("文件路径无效")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        fs::write(resolved, "").map_err(|error| error.to_string())
    }
}

#[tauri::command]
async fn rename_workspace_entry(
    path: String,
    new_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let settings = state.settings.read().await;
    let source = mutable_workspace_path(&settings, &path)?;
    let target = mutable_workspace_path(&settings, &new_path)?;
    if !source.exists() {
        return Err("原文件或文件夹不存在".into());
    }
    if target.exists() {
        return Err("目标名称已存在".into());
    }
    fs::rename(source, target).map_err(|error| error.to_string())
}

#[tauri::command]
async fn delete_workspace_entry(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let settings = state.settings.read().await;
    let resolved = mutable_workspace_path(&settings, &path)?;
    if resolved.is_dir() {
        fs::remove_dir_all(resolved).map_err(|error| error.to_string())
    } else {
        fs::remove_file(resolved).map_err(|error| error.to_string())
    }
}

#[tauri::command]
async fn workspace_git_diff(state: State<'_, AppState>) -> Result<String, String> {
    let settings = state.settings.read().await;
    let output = hidden_command("git")
        .args(["diff", "--", "."])
        .current_dir(&settings.workspace)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[tauri::command]
async fn workspace_git_changes(state: State<'_, AppState>) -> Result<Vec<GitChange>, String> {
    let settings = state.settings.read().await;
    let output = hidden_command("git")
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .current_dir(&settings.workspace)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.len() >= 4)
        .map(|line| GitChange {
            status: line[..2].to_string(),
            path: line[3..]
                .rsplit_once(" -> ")
                .map(|(_, target)| target)
                .unwrap_or(&line[3..])
                .trim_matches('"')
                .to_string(),
        })
        .collect())
}

#[tauri::command]
async fn workspace_git_overview(state: State<'_, AppState>) -> Result<GitOverview, String> {
    let settings = state.settings.read().await;
    let branch = hidden_command("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&settings.workspace)
        .output()
        .map_err(|error| error.to_string())?;
    if !branch.status.success() {
        return Ok(GitOverview {
            is_repository: false,
            branch: String::new(),
            additions: 0,
            deletions: 0,
            commits: Vec::new(),
        });
    }
    let numstat = hidden_command("git")
        .args(["diff", "--numstat", "HEAD", "--", "."])
        .current_dir(&settings.workspace)
        .output()
        .map_err(|error| error.to_string())?;
    let (additions, deletions) = String::from_utf8_lossy(&numstat.stdout).lines().fold(
        (0, 0),
        |(additions, deletions), line| {
            let mut fields = line.split('\t');
            let added = fields
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            let removed = fields
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            (additions + added, deletions + removed)
        },
    );
    let history = hidden_command("git")
        .args(["log", "-12", "--pretty=format:%h%x1f%s%x1f%an%x1f%ar"])
        .current_dir(&settings.workspace)
        .output()
        .map_err(|error| error.to_string())?;
    let commits = String::from_utf8_lossy(&history.stdout)
        .lines()
        .filter_map(|line| {
            let fields = line.split('\u{1f}').collect::<Vec<_>>();
            (fields.len() == 4).then(|| GitCommit {
                hash: fields[0].to_string(),
                subject: fields[1].to_string(),
                author: fields[2].to_string(),
                relative_time: fields[3].to_string(),
            })
        })
        .collect();
    Ok(GitOverview {
        is_repository: true,
        branch: String::from_utf8_lossy(&branch.stdout).trim().to_string(),
        additions,
        deletions,
        commits,
    })
}

#[tauri::command]
async fn workspace_file_diff(path: String, state: State<'_, AppState>) -> Result<String, String> {
    let settings = state.settings.read().await;
    workspace_path(&settings, &path)?;
    let output = hidden_command("git")
        .args(["diff", "--no-ext-diff", "HEAD", "--", &path])
        .current_dir(&settings.workspace)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[tauri::command]
async fn discard_workspace_changes(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let settings = state.settings.read().await;
    mutable_workspace_path(&settings, &path)?;
    let status = hidden_command("git")
        .args(["status", "--porcelain=v1", "--", &path])
        .current_dir(&settings.workspace)
        .output()
        .map_err(|error| error.to_string())?;
    let summary = String::from_utf8_lossy(&status.stdout);
    if summary.starts_with("??") {
        return Err("未跟踪文件不会被自动删除，请在文件树中手动确认删除".into());
    }
    let output = hidden_command("git")
        .args(["restore", "--worktree", "--", &path])
        .current_dir(&settings.workspace)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}

#[tauri::command]
async fn terminal_start(
    cols: u16,
    rows: u16,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let settings = state.settings.read().await.clone();
    if settings.approval_mode != ApprovalMode::FullAccess {
        return Err("集成终端要求权限模式为 fullAccess".into());
    }
    let mut guard = state.terminal.lock().map_err(|error| error.to_string())?;
    if guard.is_some() {
        return Ok(());
    }
    let pair = NativePtySystem::default()
        .openpty(PtySize {
            rows: rows.max(2),
            cols: cols.max(2),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| error.to_string())?;
    let mut command = CommandBuilder::new("powershell.exe");
    command.args(["-NoLogo"]);
    command.cwd(&settings.workspace);
    command.env("TERM", "xterm-256color");
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| error.to_string())?;
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| error.to_string())?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| error.to_string())?;
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    let text = String::from_utf8_lossy(&buffer[..size]).into_owned();
                    let _ = app.emit("terminal-output", text);
                }
                Err(_) => break,
            }
        }
        let _ = app.emit("terminal-exit", ());
    });
    *guard = Some(TerminalSession {
        master: pair.master,
        writer,
        child,
    });
    Ok(())
}

#[tauri::command]
fn terminal_write(data: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.terminal.lock().map_err(|error| error.to_string())?;
    let terminal = guard.as_mut().ok_or("终端尚未启动")?;
    terminal
        .writer
        .write_all(data.as_bytes())
        .and_then(|_| terminal.writer.flush())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn terminal_resize(cols: u16, rows: u16, state: State<'_, AppState>) -> Result<(), String> {
    let guard = state.terminal.lock().map_err(|error| error.to_string())?;
    let terminal = guard.as_ref().ok_or("终端尚未启动")?;
    terminal
        .master
        .resize(PtySize {
            rows: rows.max(2),
            cols: cols.max(2),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn terminal_stop(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.terminal.lock().map_err(|error| error.to_string())?;
    if let Some(mut terminal) = guard.take() {
        terminal.child.kill().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn inline_completion(
    path: String,
    prefix: String,
    suffix: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let settings = state.settings.read().await.clone();
    if settings.api_key.trim().is_empty() && !is_local_provider(&settings.provider) {
        return Err("请先配置 API Key".into());
    }
    let provider = OpenAiCompatibleProvider::new(
        settings.base_url,
        if settings.api_key.trim().is_empty() {
            "ollama".into()
        } else {
            settings.api_key
        },
        settings.model,
    )
    .map_err(|error| error.to_string())?;
    let response = provider
        .complete(ModelRequest {
            messages: vec![ModelMessage::text(
                lan_core::ModelRole::User,
                format!(
                    "你是代码补全引擎。只输出应插入光标处的代码，不要解释，不要 Markdown。\n文件：{path}\n<PREFIX>\n{}\n</PREFIX>\n<SUFFIX>\n{}\n</SUFFIX>",
                    prefix.chars().rev().take(8000).collect::<String>().chars().rev().collect::<String>(),
                    suffix.chars().take(4000).collect::<String>()
                ),
            )],
            tools: Vec::new(),
        })
        .await
        .map_err(|error| error.to_string())?;
    Ok(response.text.trim_matches('`').trim().to_string())
}

#[tauri::command]
async fn check_for_updates() -> Result<UpdateInfo, String> {
    let release = reqwest::Client::new()
        .get("https://api.github.com/repos/zhaoxinyi02/lan-code/releases/latest")
        .header("User-Agent", "Lan-Code-Desktop")
        .send()
        .await
        .map_err(|error| format!("检查更新失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("GitHub 返回错误：{error}"))?
        .json::<GithubRelease>()
        .await
        .map_err(|error| format!("解析更新信息失败：{error}"))?;
    let latest_text = release.tag_name.trim_start_matches('v');
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())?;
    let latest = semver::Version::parse(latest_text).map_err(|e| e.to_string())?;
    let installer = release
        .assets
        .iter()
        .find(|asset| asset.name.to_ascii_lowercase().ends_with("-setup.exe"));
    Ok(UpdateInfo {
        current_version: current.to_string(),
        latest_version: latest.to_string(),
        available: latest > current,
        release_url: release.html_url,
        installer_url: installer.map(|asset| asset.browser_download_url.clone()),
        installer_name: installer.map(|asset| asset.name.clone()),
        published_at: release.published_at,
        notes: release.body,
    })
}

#[tauri::command]
async fn download_update(
    installer_url: String,
    installer_name: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    if !installer_url.starts_with("https://github.com/zhaoxinyi02/lan-code/releases/download/")
        || !installer_name.to_ascii_lowercase().ends_with("-setup.exe")
        || installer_name.contains(['/', '\\'])
    {
        return Err("拒绝下载未经验证的更新地址".into());
    }
    let bytes = reqwest::Client::new()
        .get(installer_url)
        .header("User-Agent", "Lan-Code-Desktop")
        .send()
        .await
        .map_err(|error| format!("下载更新失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("下载更新失败：{error}"))?
        .bytes()
        .await
        .map_err(|error| format!("读取更新包失败：{error}"))?;
    let updates_dir = state.data_dir.read().await.join("updates");
    fs::create_dir_all(&updates_dir).map_err(|error| error.to_string())?;
    let path = updates_dir.join(installer_name);
    fs::write(&path, bytes).map_err(|error| format!("保存更新包失败：{error}"))?;
    Ok(path.display().to_string())
}

#[tauri::command]
async fn install_downloaded_update(
    path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let update = PathBuf::from(path)
        .canonicalize()
        .map_err(|error| format!("更新包不存在：{error}"))?;
    let updates_dir = state.data_dir.read().await.join("updates");
    let updates_dir = updates_dir
        .canonicalize()
        .map_err(|error| format!("更新目录不存在：{error}"))?;
    if !update.starts_with(&updates_dir)
        || update.extension().and_then(|value| value.to_str()) != Some("exe")
    {
        return Err("拒绝运行数据目录之外的安装包".into());
    }
    Command::new(update)
        .spawn()
        .map_err(|error| format!("启动安装程序失败：{error}"))?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
async fn pick_workspace() -> Result<Option<String>, String> {
    Ok(rfd::FileDialog::new()
        .pick_folder()
        .map(|path| path.display().to_string()))
}

#[tauri::command]
async fn pick_data_dir() -> Result<Option<String>, String> {
    pick_workspace().await
}

#[tauri::command]
async fn analyze_image(prompt: String, state: State<'_, AppState>) -> Result<String, String> {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("图片", &["png", "jpg", "jpeg", "webp", "gif"])
        .pick_file()
    else {
        return Err("未选择图片".into());
    };
    let settings = state.settings.read().await.clone();
    let (base_url, api_key, model) = resolved_route(
        &settings,
        &settings.vision_route,
        settings.model_capabilities.image_input,
    )?;
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    if bytes.len() > 20 * 1024 * 1024 {
        return Err("图片超过 20MB".into());
    }
    let data_url = format!("data:{};base64,{}", image_mime(&path), BASE64.encode(bytes));
    let response = reqwest::Client::new()
        .post(format!("{}/chat/completions", base_url.trim_end_matches('/')))
        .bearer_auth(api_key)
        .json(&serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": if prompt.trim().is_empty() { "请详细描述这张图片。" } else { &prompt }},
                {"type": "image_url", "image_url": {"url": data_url}}
            ]}]
        }))
        .send()
        .await
        .map_err(|error| format!("图片理解请求失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("图片理解服务返回错误：{error}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|error| error.to_string())?;
    response["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "模型没有返回图片理解文本".into())
}

#[tauri::command]
async fn generate_image(prompt: String, state: State<'_, AppState>) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("图片描述不能为空".into());
    }
    let settings = state.settings.read().await.clone();
    let (base_url, api_key, model) = resolved_route(
        &settings,
        &settings.image_generation_route,
        settings.model_capabilities.image_output,
    )?;
    let endpoint = if base_url.ends_with("/images/generations") {
        base_url
    } else {
        format!("{}/images/generations", base_url.trim_end_matches('/'))
    };
    let response = reqwest::Client::new()
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&serde_json::json!({"model": model, "prompt": prompt, "size": "1024x1024"}))
        .send()
        .await
        .map_err(|error| format!("图片生成请求失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("图片生成服务返回错误：{error}"))?
        .json::<ImageGenerationResponse>()
        .await
        .map_err(|error| error.to_string())?;
    let image = response.data.into_iter().next().ok_or("服务没有返回图片")?;
    let bytes = if let Some(data) = image.b64_json {
        BASE64.decode(data).map_err(|error| error.to_string())?
    } else if let Some(url) = image.url {
        reqwest::get(url)
            .await
            .map_err(|error| error.to_string())?
            .bytes()
            .await
            .map_err(|error| error.to_string())?
            .to_vec()
    } else {
        return Err("图片响应没有数据".into());
    };
    let output_dir = state.data_dir.read().await.join("generated");
    fs::create_dir_all(&output_dir).map_err(|error| error.to_string())?;
    let path = output_dir.join(format!("{}.png", Uuid::new_v4()));
    fs::write(&path, bytes).map_err(|error| error.to_string())?;
    Ok(path.display().to_string())
}

#[tauri::command]
async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<Session>, String> {
    Ok(core(&state).await.list_sessions().await)
}

#[tauri::command]
async fn create_session(
    cwd: String,
    title: Option<String>,
    state: State<'_, AppState>,
) -> Result<Session, String> {
    if !Path::new(&cwd).is_dir() {
        return Err("工作区目录不存在".into());
    }
    Ok(core(&state).await.create_session(cwd, title).await)
}

#[tauri::command]
async fn delete_session(session_id: SessionId, state: State<'_, AppState>) -> Result<(), String> {
    core(&state)
        .await
        .delete_session(session_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn rename_session(
    session_id: SessionId,
    title: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    core(&state)
        .await
        .rename_session(session_id, title)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn session_messages(
    session_id: SessionId,
    state: State<'_, AppState>,
) -> Result<Vec<ModelMessage>, String> {
    core(&state)
        .await
        .messages_for_session(session_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn session_events(
    session_id: SessionId,
    state: State<'_, AppState>,
) -> Result<Vec<CoreEvent>, String> {
    core(&state)
        .await
        .events_for_session(session_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn pending_approvals(state: State<'_, AppState>) -> Result<Vec<ApprovalRequest>, String> {
    Ok(core(&state).await.pending_approvals().await)
}

#[tauri::command]
async fn resolve_approval(
    request_id: Uuid,
    decision: ApprovalDecision,
    state: State<'_, AppState>,
) -> Result<(), String> {
    core(&state)
        .await
        .resolve_approval(request_id, decision)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_turn(
    session_id: SessionId,
    prompt: String,
    mode: ApprovalMode,
    state: State<'_, AppState>,
) -> Result<TurnResult, String> {
    core(&state)
        .await
        .start_turn(session_id, prompt, mode)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn interrupt_turn(session_id: SessionId, state: State<'_, AppState>) -> Result<bool, String> {
    Ok(core(&state).await.interrupt_turn(session_id).await)
}

fn main() {
    let data_dir = configured_data_dir();
    migrate_legacy_data_dir(&data_dir);
    let settings = load_settings(&data_dir);
    persist_bootstrap_settings(&data_dir, &settings);
    let core = build_core(&settings, &data_dir).unwrap_or_else(|_| AgentCore::new());
    tauri::Builder::default()
        .manage(AppState {
            core: RwLock::new(Arc::new(core)),
            settings: RwLock::new(settings),
            data_dir: RwLock::new(data_dir),
            terminal: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            test_provider,
            list_workspace_files,
            search_workspace,
            read_workspace_file,
            write_workspace_file,
            create_workspace_entry,
            rename_workspace_entry,
            delete_workspace_entry,
            workspace_git_diff,
            workspace_git_changes,
            workspace_git_overview,
            workspace_file_diff,
            discard_workspace_changes,
            terminal_start,
            terminal_write,
            terminal_resize,
            terminal_stop,
            inline_completion,
            check_for_updates,
            download_update,
            install_downloaded_update,
            pick_workspace,
            pick_data_dir,
            analyze_image,
            generate_image,
            list_sessions,
            create_session,
            delete_session,
            rename_session,
            session_messages,
            session_events,
            pending_approvals,
            resolve_approval,
            start_turn,
            interrupt_turn
        ])
        .run(tauri::generate_context!())
        .expect("run Lan Code desktop");
}

#[cfg(test)]
mod tests {
    use super::{DesktopSettings, build_core, infer_model_capabilities};

    #[test]
    fn infers_common_multimodal_model_capabilities() {
        let vision = infer_model_capabilities("gpt-4o", true);
        assert!(vision.image_input);
        assert!(vision.tool_calling);
        assert!(!vision.image_output);

        let image = infer_model_capabilities("gpt-image-1", false);
        assert!(image.image_output);
    }

    #[test]
    fn desktop_routes_register_multimodal_core_tools() {
        let mut settings = DesktopSettings::default();
        settings.model_capabilities.image_input = true;
        settings.model_capabilities.image_output = true;
        let data_dir =
            std::env::temp_dir().join(format!("lan-desktop-tools-{}", uuid::Uuid::new_v4()));
        let core = build_core(&settings, &data_dir).unwrap();
        let tools = core
            .list_tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert!(tools.contains(&"analyze_image".to_string()));
        assert!(tools.contains(&"generate_image".to_string()));
        let _ = std::fs::remove_dir_all(data_dir);
    }
}
