#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::Instant,
};

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use lan_core::{
    AgentCore, AnthropicProvider, ImageGenerationTool, ModelMessage, ModelProvider, ModelRequest,
    OpenAiCompatibleProvider, SqliteStore, VisionTool,
};
use lan_protocol::{
    ApprovalDecision, ApprovalMode, ApprovalRequest, CoreEvent, RiskLevel, Session, SessionId,
    ToolDescriptor, TurnResult,
};
use portable_pty::{Child, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::RwLock;
use uuid::Uuid;
use walkdir::WalkDir;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

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
    #[serde(default = "default_true")]
    enabled: bool,
    provider: String,
    base_url: String,
    model: String,
    api_key: String,
    input_price_per_million: f64,
    output_price_per_million: f64,
    #[serde(default = "default_context_window")]
    context_window: u64,
    #[serde(default = "default_max_output_tokens")]
    max_output_tokens: u64,
}

fn default_true() -> bool {
    true
}

fn default_context_window() -> u64 {
    128_000
}

fn default_max_output_tokens() -> u64 {
    8_192
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
    terminals: Mutex<HashMap<String, TerminalSession>>,
}

struct TerminalSession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalPayload {
    id: String,
    data: String,
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

#[derive(Deserialize)]
struct ProviderModelsResponse {
    #[serde(default)]
    data: Vec<ProviderModel>,
    #[serde(default)]
    models: Vec<ProviderModel>,
}

#[derive(Deserialize)]
struct ProviderModel {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
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

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OfficeFile {
    path: String,
    name: String,
    kind: String,
    size: u64,
    modified: u64,
    dirty: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OfficeSection {
    id: String,
    title: String,
    kind: String,
    index: usize,
    text: String,
    children: Vec<OfficeSection>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OfficeDocument {
    path: String,
    name: String,
    kind: String,
    text: String,
    sections: Vec<OfficeSection>,
    word_count: usize,
    object_count: usize,
    warnings: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OfficeBinary {
    path: String,
    mime: String,
    base64: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OfficeTextStyleRequest {
    path: String,
    text: String,
    font_family: Option<String>,
    font_size_pt: Option<f64>,
    bold: bool,
    italic: bool,
    underline: bool,
}

struct DocxTextStyle<'a> {
    font_family: Option<&'a str>,
    font_size_pt: Option<f64>,
    bold: bool,
    italic: bool,
    underline: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OfficeAction {
    id: String,
    action_type: String,
    path: String,
    target_id: Option<String>,
    old_text: Option<String>,
    new_text: Option<String>,
    note: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OfficeDiffItem {
    id: String,
    title: String,
    description: String,
    change_type: String,
    before: String,
    after: String,
    status: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OfficePatchPreview {
    patch_id: String,
    path: String,
    backup_path: String,
    summary: String,
    actions: Vec<OfficeAction>,
    diff: Vec<OfficeDiffItem>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OfficeQualityIssue {
    severity: String,
    title: String,
    detail: String,
    target: String,
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
    changed_files: usize,
    staged_files: usize,
    unstaged_files: usize,
    untracked_files: usize,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartupTarget {
    workspace: String,
    file: Option<String>,
}

#[tauri::command]
fn startup_target() -> Option<StartupTarget> {
    let args = std::env::args().collect::<Vec<_>>();
    let index = args.iter().position(|arg| arg == "--open")?;
    let target = PathBuf::from(args.get(index + 1)?);
    let target = target.canonicalize().unwrap_or(target);
    if target.is_file() {
        Some(StartupTarget {
            workspace: target.parent()?.to_string_lossy().to_string(),
            file: Some(target.to_string_lossy().to_string()),
        })
    } else {
        Some(StartupTarget {
            workspace: target.to_string_lossy().to_string(),
            file: None,
        })
    }
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://github.com/zhaoxinyi02/lan-code") {
        return Err("只允许打开 Lan Code 官方 GitHub 链接。".to_string());
    }
    hidden_command("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", &url])
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法调用系统浏览器：{error}"))
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

fn office_kind(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "docx" => Some("docx"),
        "xlsx" => Some("xlsx"),
        "pptx" => Some("pptx"),
        "pdf" => Some("pdf"),
        "md" => Some("markdown"),
        "csv" => Some("csv"),
        _ => None,
    }
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn plain_text_from_xml(xml: &str) -> String {
    let xml = xml
        .replace("</w:p>", "\n")
        .replace("</a:p>", "\n")
        .replace("</row>", "\n")
        .replace("</si>", "\n")
        .replace("</c>", "\t");
    let mut out = String::with_capacity(xml.len() / 2);
    let mut inside_tag = false;
    for ch in xml.chars() {
        match ch {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => out.push(ch),
            _ => {}
        }
    }
    xml_unescape(&out)
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn zip_text_entries(
    path: &Path,
    predicate: impl Fn(&str) -> bool,
) -> Result<Vec<(String, String)>, String> {
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("Office 文件包损坏：{error}"))?;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry.name().to_string();
        if predicate(&name) {
            let mut text = String::new();
            entry
                .read_to_string(&mut text)
                .map_err(|error| error.to_string())?;
            entries.push((name, text));
        }
    }
    Ok(entries)
}

fn build_office_section(
    id: String,
    title: String,
    kind: String,
    index: usize,
    text: String,
) -> OfficeSection {
    OfficeSection {
        id,
        title,
        kind,
        index,
        text,
        children: Vec::new(),
    }
}

fn read_ooxml_document(path: &Path, relative: &str, kind: &str) -> Result<OfficeDocument, String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(relative)
        .to_string();
    let mut sections = Vec::new();
    let mut warnings = Vec::new();
    match kind {
        "docx" => {
            let entries = zip_text_entries(path, |name| name == "word/document.xml")?;
            let text = entries
                .first()
                .map(|(_, xml)| plain_text_from_xml(xml))
                .unwrap_or_default();
            for (index, paragraph) in text.lines().enumerate() {
                let trimmed = paragraph.trim();
                if trimmed.chars().count() < 4 {
                    continue;
                }
                let title = if trimmed.chars().count() > 42 {
                    format!(
                        "第 {} 段：{}...",
                        index + 1,
                        trimmed.chars().take(42).collect::<String>()
                    )
                } else {
                    format!("第 {} 段：{}", index + 1, trimmed)
                };
                sections.push(build_office_section(
                    format!("paragraph-{index}"),
                    title,
                    "paragraph".into(),
                    index,
                    trimmed.to_string(),
                ));
                if sections.len() >= 80 {
                    warnings.push("文档段落较多，当前只显示前 80 个大纲节点。".into());
                    break;
                }
            }
            Ok(OfficeDocument {
                path: relative.into(),
                name,
                kind: kind.into(),
                word_count: text.split_whitespace().count(),
                object_count: sections.len(),
                text,
                sections,
                warnings,
            })
        }
        "pptx" => {
            let mut entries = zip_text_entries(path, |name| {
                name.starts_with("ppt/slides/slide") && name.ends_with(".xml")
            })?;
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut all = Vec::new();
            for (index, (entry, xml)) in entries.iter().enumerate() {
                let text = plain_text_from_xml(xml);
                let title = text
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("空白页");
                sections.push(build_office_section(
                    entry.clone(),
                    format!(
                        "第 {} 页：{}",
                        index + 1,
                        title.chars().take(34).collect::<String>()
                    ),
                    "slide".into(),
                    index,
                    text.clone(),
                ));
                all.push(format!("## 第 {} 页\n{}", index + 1, text));
            }
            let text = all.join("\n\n");
            Ok(OfficeDocument {
                path: relative.into(),
                name,
                kind: kind.into(),
                word_count: text.split_whitespace().count(),
                object_count: sections.len(),
                text,
                sections,
                warnings,
            })
        }
        "xlsx" => {
            let mut entries = zip_text_entries(path, |name| {
                name == "xl/sharedStrings.xml"
                    || (name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml"))
            })?;
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut all = Vec::new();
            for (index, (entry, xml)) in entries.iter().enumerate() {
                let text = plain_text_from_xml(xml);
                let title = if entry == "xl/sharedStrings.xml" {
                    "共享文本".into()
                } else {
                    format!("Sheet {}", index)
                };
                sections.push(build_office_section(
                    entry.clone(),
                    title,
                    "sheet".into(),
                    index,
                    text.clone(),
                ));
                all.push(format!("## {}\n{}", entry, text));
            }
            let text = all.join("\n\n");
            Ok(OfficeDocument {
                path: relative.into(),
                name,
                kind: kind.into(),
                word_count: text.split_whitespace().count(),
                object_count: sections.len(),
                text,
                sections,
                warnings,
            })
        }
        _ => Err("暂不支持该 Office 类型的结构化读取".into()),
    }
}

fn read_plain_office_document(
    path: &Path,
    relative: &str,
    kind: &str,
) -> Result<OfficeDocument, String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(relative)
        .to_string();
    let text = if kind == "pdf" {
        let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
        format!(
            "PDF 文件已识别。\n大小：{} KB\n\n当前版本先支持 PDF 作为上下文文件加入 Office Mode，后续会接入 PDF 文本抽取、页面渲染和标注编辑。",
            metadata.len() / 1024
        )
    } else {
        fs::read_to_string(path).map_err(|_| "该文件无法作为文本读取")?
    };
    Ok(OfficeDocument {
        path: relative.into(),
        name,
        kind: kind.into(),
        word_count: text.split_whitespace().count(),
        object_count: 1,
        sections: vec![build_office_section(
            "body".into(),
            "正文".into(),
            "text".into(),
            0,
            text.clone(),
        )],
        text,
        warnings: if kind == "pdf" {
            vec!["PDF 编辑引擎尚未启用，本版本先作为只读上下文。".into()]
        } else {
            Vec::new()
        },
    })
}

fn target_xml_for_kind(kind: &str, entry: &str) -> bool {
    match kind {
        "docx" => entry == "word/document.xml",
        "pptx" => entry.starts_with("ppt/slides/slide") && entry.ends_with(".xml"),
        "xlsx" => {
            entry == "xl/sharedStrings.xml"
                || (entry.starts_with("xl/worksheets/sheet") && entry.ends_with(".xml"))
        }
        _ => false,
    }
}

fn replace_in_office_package(
    source: &Path,
    target: &Path,
    kind: &str,
    old_text: &str,
    new_text: &str,
) -> Result<usize, String> {
    let bytes = fs::read(source).map_err(|error| error.to_string())?;
    let reader = Cursor::new(bytes);
    let mut archive = ZipArchive::new(reader).map_err(|error| error.to_string())?;
    let output = fs::File::create(target).map_err(|error| error.to_string())?;
    let mut writer = ZipWriter::new(output);
    let mut replacements = 0;
    let escaped_old = xml_escape(old_text);
    let escaped_new = xml_escape(new_text);
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = file.name().to_string();
        let options = SimpleFileOptions::default().compression_method(file.compression());
        writer
            .start_file(name.clone(), options)
            .map_err(|error| error.to_string())?;
        if target_xml_for_kind(kind, &name) {
            let mut text = String::new();
            file.read_to_string(&mut text)
                .map_err(|error| error.to_string())?;
            let before = text.clone();
            text = text.replace(&escaped_old, &escaped_new);
            if text == before {
                text = text.replace(old_text, new_text);
            }
            if text != before {
                replacements += before
                    .matches(&escaped_old)
                    .count()
                    .max(before.matches(old_text).count())
                    .max(1);
            }
            writer
                .write_all(text.as_bytes())
                .map_err(|error| error.to_string())?;
        } else {
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)
                .map_err(|error| error.to_string())?;
            writer
                .write_all(&buffer)
                .map_err(|error| error.to_string())?;
        }
    }
    writer.finish().map_err(|error| error.to_string())?;
    Ok(replacements)
}

fn style_docx_text(
    source: &Path,
    target: &Path,
    selected_text: &str,
    style: DocxTextStyle<'_>,
) -> Result<usize, String> {
    let bytes = fs::read(source).map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|error| error.to_string())?;
    let output = fs::File::create(target).map_err(|error| error.to_string())?;
    let mut writer = ZipWriter::new(output);
    let needle = xml_escape(selected_text);
    let mut applied = 0;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = file.name().to_string();
        let options = SimpleFileOptions::default().compression_method(file.compression());
        writer
            .start_file(name.clone(), options)
            .map_err(|error| error.to_string())?;
        if name == "word/document.xml" {
            let mut xml = String::new();
            file.read_to_string(&mut xml)
                .map_err(|error| error.to_string())?;
            if let Some(text_pos) = xml.find(&needle)
                && let Some(run_start) = xml[..text_pos].rfind("<w:r")
                && let Some(run_end_offset) = xml[text_pos..].find("</w:r>")
            {
                let run_end = text_pos + run_end_offset + "</w:r>".len();
                let run = &xml[run_start..run_end];
                let mut properties = String::new();
                if let Some(family) = style.font_family {
                    let family = xml_escape(family);
                    properties.push_str(&format!(
                        r#"<w:rFonts w:ascii="{family}" w:hAnsi="{family}" w:eastAsia="{family}"/>"#
                    ));
                }
                if let Some(size) = style.font_size_pt {
                    let half_points = (size * 2.0).round().clamp(2.0, 400.0) as u32;
                    properties.push_str(&format!(
                        r#"<w:sz w:val="{half_points}"/><w:szCs w:val="{half_points}"/>"#
                    ));
                }
                if style.bold {
                    properties.push_str("<w:b/>");
                }
                if style.italic {
                    properties.push_str("<w:i/>");
                }
                if style.underline {
                    properties.push_str(r#"<w:u w:val="single"/>"#);
                }
                let styled = if let Some(close) = run.find("</w:rPr>") {
                    let insert = close;
                    format!("{}{}{}", &run[..insert], properties, &run[insert..])
                } else if let Some(open_end) = run.find('>') {
                    format!(
                        "{}<w:rPr>{}</w:rPr>{}",
                        &run[..=open_end],
                        properties,
                        &run[open_end + 1..]
                    )
                } else {
                    run.to_string()
                };
                xml.replace_range(run_start..run_end, &styled);
                applied = 1;
            }
            writer
                .write_all(xml.as_bytes())
                .map_err(|error| error.to_string())?;
        } else {
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)
                .map_err(|error| error.to_string())?;
            writer
                .write_all(&buffer)
                .map_err(|error| error.to_string())?;
        }
    }
    writer.finish().map_err(|error| error.to_string())?;
    Ok(applied)
}

fn append_to_office_package(
    source: &Path,
    target: &Path,
    kind: &str,
    new_text: &str,
) -> Result<usize, String> {
    let bytes = fs::read(source).map_err(|error| error.to_string())?;
    let reader = Cursor::new(bytes);
    let mut archive = ZipArchive::new(reader).map_err(|error| error.to_string())?;
    let output = fs::File::create(target).map_err(|error| error.to_string())?;
    let mut writer = ZipWriter::new(output);
    let mut applied = 0;
    let escaped = xml_escape(new_text);
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = file.name().to_string();
        let options = SimpleFileOptions::default().compression_method(file.compression());
        writer
            .start_file(name.clone(), options)
            .map_err(|error| error.to_string())?;
        if applied == 0 && target_xml_for_kind(kind, &name) {
            let mut text = String::new();
            file.read_to_string(&mut text)
                .map_err(|error| error.to_string())?;
            let patched = match kind {
                "docx" => text.replace(
                    "</w:body>",
                    &format!("<w:p><w:r><w:t>{escaped}</w:t></w:r></w:p></w:body>"),
                ),
                "pptx" => text.replace(
                    "</p:spTree>",
                    &format!(r#"<p:sp><p:nvSpPr/><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>{escaped}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree>"#),
                ),
                "xlsx" => text.replace(
                    "</sheetData>",
                    &format!(r#"<row><c t="inlineStr"><is><t>{escaped}</t></is></c></row></sheetData>"#),
                ),
                _ => text.clone(),
            };
            if patched != text {
                applied = 1;
            }
            writer
                .write_all(patched.as_bytes())
                .map_err(|error| error.to_string())?;
        } else {
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)
                .map_err(|error| error.to_string())?;
            writer
                .write_all(&buffer)
                .map_err(|error| error.to_string())?;
        }
    }
    writer.finish().map_err(|error| error.to_string())?;
    Ok(applied)
}

fn office_backup_path(path: &Path) -> PathBuf {
    let stamp = chrono_like_stamp();
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("office-file");
    path.with_file_name(format!("{name}.lancode-backup-{stamp}"))
}

fn chrono_like_stamp() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default();
    millis.to_string()
}

fn write_zip_text(writer: &mut ZipWriter<fs::File>, name: &str, text: &str) -> Result<(), String> {
    writer
        .start_file(
            name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .map_err(|error| error.to_string())?;
    writer
        .write_all(text.as_bytes())
        .map_err(|error| error.to_string())
}

fn create_minimal_docx(path: &Path, title: &str) -> Result<(), String> {
    let file = fs::File::create(path).map_err(|error| error.to_string())?;
    let mut writer = ZipWriter::new(file);
    write_zip_text(
        &mut writer,
        "[Content_Types].xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
    )?;
    write_zip_text(
        &mut writer,
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
    )?;
    write_zip_text(
        &mut writer,
        "word/document.xml",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:spacing w:after="240"/></w:pPr><w:r><w:rPr><w:rFonts w:ascii="Microsoft YaHei" w:eastAsia="Microsoft YaHei"/><w:b/><w:color w:val="1E4F91"/><w:sz w:val="36"/></w:rPr><w:t>{}</w:t></w:r></w:p><w:p><w:pPr><w:spacing w:after="160"/></w:pPr><w:r><w:rPr><w:rFonts w:ascii="Microsoft YaHei" w:eastAsia="Microsoft YaHei"/><w:color w:val="5F6B7A"/><w:sz w:val="22"/></w:rPr><w:t>从 Lan Code Office Mode 开始编辑。</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#,
            xml_escape(title)
        ),
    )?;
    writer.finish().map_err(|error| error.to_string())?;
    Ok(())
}

fn create_minimal_pptx(path: &Path, title: &str) -> Result<(), String> {
    let file = fs::File::create(path).map_err(|error| error.to_string())?;
    let mut writer = ZipWriter::new(file);
    write_zip_text(
        &mut writer,
        "[Content_Types].xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>"#,
    )?;
    write_zip_text(
        &mut writer,
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#,
    )?;
    write_zip_text(
        &mut writer,
        "ppt/presentation.xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/></p:presentation>"#,
    )?;
    write_zip_text(
        &mut writer,
        "ppt/_rels/presentation.xml.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"#,
    )?;
    write_zip_text(
        &mut writer,
        "ppt/slides/slide1.xml",
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld name="Lan Code Slide"><p:bg><p:bgPr><a:solidFill><a:srgbClr val="F7F9FC"/></a:solidFill><a:effectLst/></p:bgPr></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id="2" name="Accent"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="1371600" y="1097280"/><a:ext cx="1219200" cy="91440"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="2F80ED"/></a:solidFill><a:ln><a:noFill/></a:ln></p:spPr></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Lan Code Content"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="1371600" y="1371600"/><a:ext cx="9144000" cy="3657600"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="square" anchor="t"><a:spAutoFit/></a:bodyPr><a:lstStyle/><a:p><a:pPr algn="l"/><a:r><a:rPr lang="zh-CN" sz="3200" b="1"/><a:t>{}</a:t></a:r><a:endParaRPr lang="zh-CN" sz="3200"/></a:p><a:p><a:pPr marT="360000"/><a:r><a:rPr lang="zh-CN" sz="1800"/><a:t>从 Lan Code Office Mode 开始编辑。</a:t></a:r><a:endParaRPr lang="zh-CN" sz="1800"/></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#,
            xml_escape(title)
        ),
    )?;
    writer.finish().map_err(|error| error.to_string())?;
    Ok(())
}

fn create_minimal_xlsx(path: &Path) -> Result<(), String> {
    let file = fs::File::create(path).map_err(|error| error.to_string())?;
    let mut writer = ZipWriter::new(file);
    write_zip_text(
        &mut writer,
        "[Content_Types].xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
    )?;
    write_zip_text(
        &mut writer,
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
    )?;
    write_zip_text(
        &mut writer,
        "xl/workbook.xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
    )?;
    write_zip_text(
        &mut writer,
        "xl/_rels/workbook.xml.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
    )?;
    write_zip_text(
        &mut writer,
        "xl/worksheets/sheet1.xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Lan Code Office Mode</t></is></c></row></sheetData></worksheet>"#,
    )?;
    writer.finish().map_err(|error| error.to_string())?;
    Ok(())
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
    let context_window = active_context_window(settings) as usize;
    let core = if settings.api_key.trim().is_empty() && !is_local_provider(&settings.provider) {
        AgentCore::with_store(store)
            .map(|core| core.with_max_provider_rounds(settings.max_provider_rounds))
            .map(|core| core.with_model_context_tokens(context_window))
            .map_err(|error| error.to_string())?
    } else {
        let api_key = if settings.api_key.trim().is_empty() {
            "ollama".into()
        } else {
            settings.api_key.clone()
        };
        let provider = build_model_provider(
            &settings.provider,
            settings.base_url.clone(),
            api_key,
            settings.model.clone(),
            Some(active_max_output_tokens(settings)),
        )?;
        AgentCore::with_provider_and_store(Arc::from(provider), store)
            .map(|core| core.with_max_provider_rounds(settings.max_provider_rounds))
            .map(|core| core.with_model_context_tokens(context_window))
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

fn build_model_provider(
    provider: &str,
    base_url: String,
    api_key: String,
    model: String,
    max_output_tokens: Option<u64>,
) -> Result<Box<dyn ModelProvider>, String> {
    if provider == "anthropic" {
        return AnthropicProvider::new_with_limits(base_url, api_key, model, max_output_tokens)
            .map(|provider| Box::new(provider) as Box<dyn ModelProvider>)
            .map_err(|error| error.to_string());
    }
    OpenAiCompatibleProvider::new_with_limits(base_url, api_key, model, max_output_tokens)
        .map(|provider| Box::new(provider) as Box<dyn ModelProvider>)
        .map_err(|error| error.to_string())
}

fn active_context_window(settings: &DesktopSettings) -> u64 {
    settings
        .provider_profiles
        .iter()
        .find(|profile| {
            profile.enabled
                && profile.provider == settings.provider
                && profile.base_url == settings.base_url
                && profile.model == settings.model
        })
        .map(|profile| profile.context_window)
        .unwrap_or_else(default_context_window)
}

fn active_max_output_tokens(settings: &DesktopSettings) -> u64 {
    settings
        .provider_profiles
        .iter()
        .find(|profile| {
            profile.enabled
                && profile.provider == settings.provider
                && profile.base_url == settings.base_url
                && profile.model == settings.model
        })
        .map(|profile| profile.max_output_tokens)
        .unwrap_or_else(default_max_output_tokens)
}

async fn core(state: &State<'_, AppState>) -> Arc<AgentCore> {
    state.core.read().await.clone()
}

fn spawn_core_event_forwarder(app: AppHandle, core: Arc<AgentCore>) {
    tauri::async_runtime::spawn(async move {
        let mut events = core.subscribe();
        loop {
            match events.recv().await {
                Ok(event) => {
                    let _ = app.emit("core-event", event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<DesktopSettings, String> {
    Ok(state.settings.read().await.clone())
}

#[tauri::command]
async fn save_settings(
    mut settings: DesktopSettings,
    state: State<'_, AppState>,
    app: AppHandle,
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
    let next_core = Arc::new(build_core(&settings, &next_data_dir)?);
    spawn_core_event_forwarder(app, next_core.clone());
    *state.core.write().await = next_core;
    *state.settings.write().await = settings;
    *state.data_dir.write().await = next_data_dir;
    Ok(())
}

#[tauri::command]
async fn test_provider(settings: DesktopSettings) -> Result<ProviderTestResult, String> {
    if settings.api_key.trim().is_empty() && !is_local_provider(&settings.provider) {
        return Err("请先填写 API Key".into());
    }
    let max_output_tokens = active_max_output_tokens(&settings);
    let api_key = if settings.api_key.trim().is_empty() {
        "ollama".into()
    } else {
        settings.api_key.clone()
    };
    let provider = build_model_provider(
        &settings.provider,
        settings.base_url.clone(),
        api_key,
        settings.model.clone(),
        Some(max_output_tokens),
    )?;
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
async fn list_provider_models(
    provider: String,
    base_url: String,
    api_key: String,
) -> Result<Vec<String>, String> {
    let base_url = base_url.trim_end_matches('/');
    if base_url.is_empty() {
        return Err("请先填写 API 地址".into());
    }
    if api_key.trim().is_empty() && !is_local_provider(&provider) {
        return Err("请先填写 API Key".into());
    }

    let mut request = reqwest::Client::new().get(format!("{base_url}/models"));
    if !api_key.trim().is_empty() {
        request = request.bearer_auth(api_key.trim());
    }
    if provider == "anthropic" {
        request = request
            .header("x-api-key", api_key.trim())
            .header("anthropic-version", "2023-06-01");
    }
    let response = request
        .header("User-Agent", "Lan-Code-Desktop")
        .send()
        .await
        .map_err(|error| format!("获取模型失败：{error}"))?
        .error_for_status()
        .map_err(|error| format!("模型接口返回错误：{error}"))?
        .json::<ProviderModelsResponse>()
        .await
        .map_err(|error| format!("解析模型列表失败：{error}"))?;

    let mut models = response
        .data
        .into_iter()
        .chain(response.models)
        .map(|model| {
            if model.id.is_empty() {
                model.name
            } else {
                model.id
            }
        })
        .filter(|model| !model.is_empty())
        .collect::<Vec<_>>();
    models.sort_by_key(|model| model.to_ascii_lowercase());
    models.dedup();
    if models.is_empty() {
        return Err("服务端没有返回可用模型 ID".into());
    }
    Ok(models)
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
async fn office_list_files(state: State<'_, AppState>) -> Result<Vec<OfficeFile>, String> {
    let settings = state.settings.read().await;
    let root = PathBuf::from(&settings.workspace)
        .canonicalize()
        .map_err(|error| format!("工作区不可用：{error}"))?;
    let ignored = [".git", "node_modules", "target", "dist", ".idea", ".vscode"];
    let git_status = hidden_command("git")
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .current_dir(&root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|line| line.len() >= 4)
                .map(|line| {
                    line[3..]
                        .rsplit_once(" -> ")
                        .map(|(_, target)| target)
                        .unwrap_or(&line[3..])
                        .trim_matches('"')
                        .to_string()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let mut files = Vec::new();
    for entry in WalkDir::new(&root)
        .max_depth(8)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0 || !ignored.contains(&entry.file_name().to_string_lossy().as_ref())
        })
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let Some(kind) = office_kind(entry.path()) else {
            continue;
        };
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        let relative = entry
            .path()
            .strip_prefix(&root)
            .map_err(|error| error.to_string())?
            .display()
            .to_string();
        files.push(OfficeFile {
            name: entry.file_name().to_string_lossy().to_string(),
            dirty: git_status.contains(&relative),
            path: relative,
            kind: kind.into(),
            size: metadata.len(),
            modified: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs())
                .unwrap_or_default(),
        });
    }
    files.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
    Ok(files)
}

#[tauri::command]
async fn office_read_file(
    path: String,
    state: State<'_, AppState>,
) -> Result<OfficeDocument, String> {
    let settings = state.settings.read().await;
    let resolved = workspace_path(&settings, &path)?;
    let kind = office_kind(&resolved).ok_or("不是 Office Mode 支持的文件类型")?;
    if matches!(kind, "docx" | "pptx" | "xlsx") {
        read_ooxml_document(&resolved, &path, kind)
    } else {
        read_plain_office_document(&resolved, &path, kind)
    }
}

#[tauri::command]
async fn office_read_binary(
    path: String,
    state: State<'_, AppState>,
) -> Result<OfficeBinary, String> {
    let settings = state.settings.read().await;
    let resolved = workspace_path(&settings, &path)?;
    let kind = office_kind(&resolved).ok_or("不是 Office Mode 支持的文件类型")?;
    if !matches!(kind, "docx" | "xlsx" | "pptx") {
        return Err("该文件类型不支持二进制 Office 编辑器".into());
    }
    let bytes = fs::read(&resolved).map_err(|error| format!("读取 Office 文件失败：{error}"))?;
    if bytes.len() > 128 * 1024 * 1024 {
        return Err("Office 文件超过 128MB，暂不在内置编辑器中打开".into());
    }
    let mime = match kind {
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    };
    Ok(OfficeBinary {
        path,
        mime: mime.into(),
        base64: BASE64.encode(bytes),
    })
}

#[tauri::command]
async fn office_write_binary(
    path: String,
    base64: String,
    state: State<'_, AppState>,
) -> Result<OfficeDocument, String> {
    let settings = state.settings.read().await;
    let resolved = mutable_workspace_path(&settings, &path)?;
    let kind = office_kind(&resolved).ok_or("不是 Office Mode 支持的文件类型")?;
    if !matches!(kind, "docx" | "xlsx" | "pptx") {
        return Err("该文件类型不支持二进制写回".into());
    }
    let bytes = BASE64
        .decode(base64.as_bytes())
        .map_err(|error| format!("Office 文件数据无效：{error}"))?;
    if bytes.len() > 128 * 1024 * 1024 {
        return Err("Office 文件超过 128MB，拒绝写入".into());
    }
    let backup = office_backup_path(&resolved);
    if resolved.exists() {
        fs::copy(&resolved, &backup).map_err(|error| format!("创建 Office 备份失败：{error}"))?;
    }
    let temp = resolved.with_extension(format!("{kind}.lancode-write"));
    fs::write(&temp, bytes).map_err(|error| format!("写入 Office 临时文件失败：{error}"))?;
    fs::rename(&temp, &resolved)
        .or_else(|_| {
            fs::copy(&temp, &resolved)?;
            fs::remove_file(&temp)
        })
        .map_err(|error| format!("替换 Office 文件失败：{error}"))?;
    read_ooxml_document(&resolved, &path, kind)
}

#[tauri::command]
async fn office_style_text(
    request: OfficeTextStyleRequest,
    state: State<'_, AppState>,
) -> Result<OfficeDocument, String> {
    if request.text.trim().is_empty() {
        return Err("请先在文档中选中要调整格式的文字".into());
    }
    let settings = state.settings.read().await;
    let resolved = mutable_workspace_path(&settings, &request.path)?;
    let kind = office_kind(&resolved).ok_or("不是 Office Mode 支持的文件类型")?;
    if kind != "docx" {
        return Err("当前格式工具仅对 DOCX 原文件写回；表格请使用内嵌编辑器".into());
    }
    let backup = office_backup_path(&resolved);
    fs::copy(&resolved, &backup).map_err(|error| format!("创建 Office 备份失败：{error}"))?;
    let temp = resolved.with_extension("docx.lancode-style");
    let applied = style_docx_text(
        &resolved,
        &temp,
        &request.text,
        DocxTextStyle {
            font_family: request.font_family.as_deref(),
            font_size_pt: request.font_size_pt,
            bold: request.bold,
            italic: request.italic,
            underline: request.underline,
        },
    )?;
    if applied == 0 {
        let _ = fs::remove_file(&temp);
        return Err("没有在 DOCX 文本节点中找到该选区，请缩小选区后重试".into());
    }
    fs::copy(&temp, &resolved).map_err(|error| format!("写回 DOCX 样式失败：{error}"))?;
    let _ = fs::remove_file(temp);
    read_ooxml_document(&resolved, &request.path, kind)
}

#[tauri::command]
async fn office_create_file(
    path: String,
    kind: String,
    state: State<'_, AppState>,
) -> Result<OfficeDocument, String> {
    let settings = state.settings.read().await;
    let resolved = mutable_workspace_path(&settings, &path)?;
    if resolved.exists() {
        return Err("同名 Office 文件已存在".into());
    }
    if let Some(parent) = resolved.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    match kind.as_str() {
        "docx" => create_minimal_docx(&resolved, "Lan Code 新文档")?,
        "pptx" => create_minimal_pptx(&resolved, "Lan Code 新演示")?,
        "xlsx" => create_minimal_xlsx(&resolved)?,
        "markdown" | "md" => {
            fs::write(&resolved, "# 新文档\n").map_err(|error| error.to_string())?
        }
        _ => return Err("暂不支持创建该类型".into()),
    }
    let actual_kind = office_kind(&resolved).unwrap_or(kind.as_str());
    if matches!(actual_kind, "docx" | "pptx" | "xlsx") {
        read_ooxml_document(&resolved, &path, actual_kind)
    } else {
        read_plain_office_document(&resolved, &path, actual_kind)
    }
}

#[tauri::command]
async fn office_preview_patch(
    actions: Vec<OfficeAction>,
    state: State<'_, AppState>,
) -> Result<OfficePatchPreview, String> {
    let settings = state.settings.read().await;
    let first = actions.first().ok_or("没有可预览的 Office 操作")?;
    let resolved = mutable_workspace_path(&settings, &first.path)?;
    let kind = office_kind(&resolved).ok_or("不是 Office Mode 支持的文件类型")?;
    let backup = office_backup_path(&resolved);
    let mut diff = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        let before = action.old_text.clone().unwrap_or_default();
        let after = action.new_text.clone().unwrap_or_default();
        diff.push(OfficeDiffItem {
            id: action.id.clone(),
            title: match action.action_type.as_str() {
                "replace_text" => "替换文字".into(),
                "insert_text" => "插入文字".into(),
                "set_style" => "调整样式".into(),
                other => format!("Office 操作：{other}"),
            },
            description: action.note.clone().unwrap_or_else(|| {
                format!(
                    "目标：{}",
                    action
                        .target_id
                        .clone()
                        .unwrap_or_else(|| "当前文档".into())
                )
            }),
            change_type: if before.is_empty() {
                "insert".into()
            } else {
                "content".into()
            },
            before,
            after,
            status: if matches!(kind, "docx" | "pptx" | "xlsx") {
                "ready".into()
            } else {
                "readonly".into()
            },
        });
        if index > 50 {
            break;
        }
    }
    Ok(OfficePatchPreview {
        patch_id: Uuid::new_v4().to_string(),
        path: first.path.clone(),
        backup_path: backup.display().to_string(),
        summary: format!(
            "准备对 {} 执行 {} 个结构化 Office 操作",
            first.path,
            actions.len()
        ),
        actions,
        diff,
    })
}

#[tauri::command]
async fn office_apply_patch(
    actions: Vec<OfficeAction>,
    state: State<'_, AppState>,
) -> Result<OfficePatchPreview, String> {
    let settings = state.settings.read().await;
    let first = actions.first().ok_or("没有可应用的 Office 操作")?;
    let resolved = mutable_workspace_path(&settings, &first.path)?;
    let kind = office_kind(&resolved).ok_or("不是 Office Mode 支持的文件类型")?;
    if !matches!(kind, "docx" | "pptx" | "xlsx" | "markdown" | "csv") {
        return Err("该文件类型当前只支持只读上下文，不支持写回".into());
    }
    let backup = office_backup_path(&resolved);
    fs::copy(&resolved, &backup).map_err(|error| error.to_string())?;
    let mut diff = Vec::new();
    if matches!(kind, "docx" | "pptx" | "xlsx") {
        let mut current = resolved.clone();
        let mut temp_paths = Vec::new();
        for action in actions.iter() {
            let old_text = action.old_text.as_deref().unwrap_or_default();
            let new_text = action.new_text.as_deref().unwrap_or_default();
            if old_text.is_empty() && action.action_type != "insert_text" {
                continue;
            }
            let temp = resolved.with_extension(format!("{}.lancode-tmp", Uuid::new_v4()));
            let replacements = if action.action_type == "insert_text" && old_text.is_empty() {
                append_to_office_package(&current, &temp, kind, new_text)?
            } else {
                replace_in_office_package(&current, &temp, kind, old_text, new_text)?
            };
            temp_paths.push(temp.clone());
            current = temp;
            diff.push(OfficeDiffItem {
                id: action.id.clone(),
                title: if replacements == 0 {
                    "未找到目标文本".into()
                } else {
                    "已应用文本替换".into()
                },
                description: action
                    .note
                    .clone()
                    .unwrap_or_else(|| format!("命中 {replacements} 处")),
                change_type: "content".into(),
                before: old_text.into(),
                after: new_text.into(),
                status: if replacements == 0 {
                    "skipped".into()
                } else {
                    "applied".into()
                },
            });
        }
        if current != resolved {
            fs::copy(&current, &resolved).map_err(|error| error.to_string())?;
        }
        for temp in temp_paths {
            let _ = fs::remove_file(temp);
        }
    } else {
        let mut content = fs::read_to_string(&resolved).map_err(|_| "文本文件读取失败")?;
        for action in actions.iter() {
            let old_text = action.old_text.as_deref().unwrap_or_default();
            let new_text = action.new_text.as_deref().unwrap_or_default();
            if old_text.is_empty() {
                content.push_str(new_text);
            } else {
                content = content.replace(old_text, new_text);
            }
            diff.push(OfficeDiffItem {
                id: action.id.clone(),
                title: "已应用文本操作".into(),
                description: action.note.clone().unwrap_or_default(),
                change_type: "content".into(),
                before: old_text.into(),
                after: new_text.into(),
                status: "applied".into(),
            });
        }
        fs::write(&resolved, content).map_err(|error| error.to_string())?;
    }
    Ok(OfficePatchPreview {
        patch_id: Uuid::new_v4().to_string(),
        path: first.path.clone(),
        backup_path: backup.display().to_string(),
        summary: format!("已写入 {}，原文件备份为 {}", first.path, backup.display()),
        actions,
        diff,
    })
}

#[tauri::command]
async fn office_rollback(
    backup_path: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<OfficeDocument, String> {
    let settings = state.settings.read().await;
    let target = mutable_workspace_path(&settings, &path)?;
    let backup = PathBuf::from(&backup_path);
    if !backup.exists() {
        return Err("备份文件不存在".into());
    }
    fs::copy(&backup, &target).map_err(|error| error.to_string())?;
    let kind = office_kind(&target).ok_or("不是 Office Mode 支持的文件类型")?;
    if matches!(kind, "docx" | "pptx" | "xlsx") {
        read_ooxml_document(&target, &path, kind)
    } else {
        read_plain_office_document(&target, &path, kind)
    }
}

#[tauri::command]
async fn office_check_file(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<OfficeQualityIssue>, String> {
    let document = office_read_file(path, state).await?;
    let mut issues = Vec::new();
    if document.text.trim().is_empty() {
        issues.push(OfficeQualityIssue {
            severity: "warning".into(),
            title: "内容为空".into(),
            detail: "当前文件没有可读取的文本内容，可能是图片型文档或复杂对象。".into(),
            target: document.name.clone(),
        });
    }
    if document.text.lines().any(|line| line.chars().count() > 120) {
        issues.push(OfficeQualityIssue {
            severity: "info".into(),
            title: "存在较长段落".into(),
            detail: "建议检查页面换行、表格宽度或幻灯片文本溢出。".into(),
            target: document.name.clone(),
        });
    }
    if document.warnings.is_empty() && issues.is_empty() {
        issues.push(OfficeQualityIssue {
            severity: "ok".into(),
            title: "基础结构检查通过".into(),
            detail: "文件可读取，未发现明显空内容或长行风险。".into(),
            target: document.name,
        });
    }
    Ok(issues)
}

#[tauri::command]
async fn office_export_markdown(
    path: String,
    state: State<'_, AppState>,
) -> Result<WorkspaceFile, String> {
    let document = office_read_file(path.clone(), state.clone()).await?;
    let settings = state.settings.read().await;
    let source = workspace_path(&settings, &path)?;
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("office-export");
    let export_name = format!("{stem}.office.md");
    let export_path = source.with_file_name(&export_name);
    let relative = export_path
        .strip_prefix(
            PathBuf::from(&settings.workspace)
                .canonicalize()
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?
        .display()
        .to_string();
    let mut markdown = format!(
        "# {}\n\n- 来源文件：`{}`\n- 类型：{}\n- 对象数量：{}\n\n",
        document.name,
        document.path,
        document.kind.to_uppercase(),
        document.object_count
    );
    if document.sections.is_empty() {
        markdown.push_str(&document.text);
    } else {
        for section in document.sections {
            markdown.push_str(&format!("## {}\n\n{}\n\n", section.title, section.text));
        }
    }
    fs::write(&export_path, markdown.as_bytes()).map_err(|error| error.to_string())?;
    Ok(WorkspaceFile {
        path: relative,
        content: markdown,
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
    let inside = hidden_command("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(&settings.workspace)
        .output()
        .map_err(|error| error.to_string())?;
    if !inside.status.success() {
        return Ok(GitOverview {
            is_repository: false,
            branch: String::new(),
            additions: 0,
            deletions: 0,
            changed_files: 0,
            staged_files: 0,
            unstaged_files: 0,
            untracked_files: 0,
            commits: Vec::new(),
        });
    }
    let branch = hidden_command("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(&settings.workspace)
        .output()
        .map_err(|error| error.to_string())?;
    let numstat = hidden_command("git")
        .args(["diff", "--numstat", "HEAD", "--", "."])
        .current_dir(&settings.workspace)
        .output()
        .map_err(|error| error.to_string())?;
    let (mut additions, deletions) = String::from_utf8_lossy(&numstat.stdout).lines().fold(
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
    let untracked = hidden_command("git")
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .current_dir(&settings.workspace)
        .output()
        .map_err(|error| error.to_string())?;
    for relative in untracked
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let Ok(relative) = std::str::from_utf8(relative) else {
            continue;
        };
        let path = Path::new(&settings.workspace).join(relative);
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        if bytes.len() <= 2 * 1024 * 1024 && !bytes.contains(&0) {
            additions += bytes.iter().filter(|byte| **byte == b'\n').count() as u64
                + u64::from(!bytes.is_empty() && bytes.last() != Some(&b'\n'));
        }
    }
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
    let status = hidden_command("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(&settings.workspace)
        .output()
        .map_err(|error| error.to_string())?;
    let mut changed_files = 0;
    let mut staged_files = 0;
    let mut unstaged_files = 0;
    let mut untracked_files = 0;
    for line in String::from_utf8_lossy(&status.stdout).lines() {
        if line.len() < 2 {
            continue;
        }
        changed_files += 1;
        let bytes = line.as_bytes();
        if bytes[0] == b'?' && bytes[1] == b'?' {
            untracked_files += 1;
            continue;
        }
        if bytes[0] != b' ' {
            staged_files += 1;
        }
        if bytes[1] != b' ' {
            unstaged_files += 1;
        }
    }
    let branch_name = String::from_utf8_lossy(&branch.stdout).trim().to_string();
    Ok(GitOverview {
        is_repository: true,
        branch: if branch_name.is_empty() {
            "未提交".into()
        } else {
            branch_name
        },
        additions,
        deletions,
        changed_files,
        staged_files,
        unstaged_files,
        untracked_files,
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
    id: String,
    shell: String,
    cols: u16,
    rows: u16,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let settings = state.settings.read().await.clone();
    if settings.approval_mode != ApprovalMode::FullAccess {
        return Err("集成终端要求权限模式为 fullAccess".into());
    }
    let mut guard = state.terminals.lock().map_err(|error| error.to_string())?;
    if guard.contains_key(&id) {
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
    let mut command = match shell.as_str() {
        "cmd" => {
            let mut command = CommandBuilder::new("cmd.exe");
            command.args(["/K"]);
            command
        }
        "wsl" => CommandBuilder::new("wsl.exe"),
        _ => {
            let mut command = CommandBuilder::new("powershell.exe");
            command.args(["-NoLogo"]);
            command
        }
    };
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
    let terminal_id = id.clone();
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    let text = String::from_utf8_lossy(&buffer[..size]).into_owned();
                    let _ = app.emit(
                        "terminal-output",
                        TerminalPayload {
                            id: terminal_id.clone(),
                            data: text,
                        },
                    );
                }
                Err(_) => break,
            }
        }
        let _ = app.emit(
            "terminal-exit",
            TerminalPayload {
                id: terminal_id,
                data: String::new(),
            },
        );
    });
    guard.insert(
        id,
        TerminalSession {
            master: pair.master,
            writer,
            child,
        },
    );
    Ok(())
}

#[tauri::command]
fn terminal_write(id: String, data: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.terminals.lock().map_err(|error| error.to_string())?;
    let terminal = guard.get_mut(&id).ok_or("终端尚未启动")?;
    terminal
        .writer
        .write_all(data.as_bytes())
        .and_then(|_| terminal.writer.flush())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn terminal_resize(
    id: String,
    cols: u16,
    rows: u16,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let guard = state.terminals.lock().map_err(|error| error.to_string())?;
    let terminal = guard.get(&id).ok_or("终端尚未启动")?;
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
    let mut guard = state.terminals.lock().map_err(|error| error.to_string())?;
    for (_, mut terminal) in guard.drain() {
        terminal.child.kill().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn terminal_stop_one(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.terminals.lock().map_err(|error| error.to_string())?;
    if let Some(mut terminal) = guard.remove(&id) {
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
    let provider = build_model_provider(
        &settings.provider,
        settings.base_url.clone(),
        if settings.api_key.trim().is_empty() {
            "ollama".into()
        } else {
            settings.api_key.clone()
        },
        settings.model.clone(),
        Some(active_max_output_tokens(&settings)),
    )?;
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
    let client = reqwest::Client::new();
    let checksum_url = format!("{installer_url}.sha256");
    let checksum_text = client
        .get(&checksum_url)
        .header("User-Agent", "Lan-Code-Desktop")
        .send()
        .await
        .map_err(|error| format!("下载更新校验文件失败：{error}"))?
        .error_for_status()
        .map_err(|_| "更新包缺少 SHA-256 校验文件，已拒绝下载。".to_string())?
        .text()
        .await
        .map_err(|error| format!("读取更新校验文件失败：{error}"))?;
    let expected_hash = checksum_text
        .split_whitespace()
        .find(|part| part.len() == 64 && part.chars().all(|ch| ch.is_ascii_hexdigit()))
        .ok_or_else(|| "更新校验文件格式无效，未找到 SHA-256。".to_string())?
        .to_ascii_lowercase();
    let bytes = client
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
    let actual_hash = format!("{:x}", Sha256::digest(&bytes));
    if actual_hash != expected_hash {
        return Err("更新包 SHA-256 校验失败，已拒绝安装。".into());
    }
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
    let core = Arc::new(build_core(&settings, &data_dir).unwrap_or_else(|_| AgentCore::new()));
    let event_core = core.clone();
    tauri::Builder::default()
        .setup(move |app| {
            if let Some(window) = app.get_webview_window("main") {
                let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/taskbar.png"))?;
                window.set_icon(icon)?;
            }
            spawn_core_event_forwarder(app.handle().clone(), event_core.clone());
            Ok(())
        })
        .manage(AppState {
            core: RwLock::new(core),
            settings: RwLock::new(settings),
            data_dir: RwLock::new(data_dir),
            terminals: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            test_provider,
            list_provider_models,
            list_workspace_files,
            search_workspace,
            read_workspace_file,
            office_list_files,
            office_read_file,
            office_read_binary,
            office_write_binary,
            office_style_text,
            office_create_file,
            office_preview_patch,
            office_apply_patch,
            office_rollback,
            office_check_file,
            office_export_markdown,
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
            terminal_stop_one,
            inline_completion,
            check_for_updates,
            startup_target,
            open_external_url,
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
    use super::{
        DesktopSettings, DocxTextStyle, build_core, create_minimal_docx, create_minimal_pptx,
        create_minimal_xlsx, infer_model_capabilities, plain_text_from_xml, read_ooxml_document,
        style_docx_text, zip_text_entries,
    };

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

    #[test]
    fn office_xml_text_extraction_preserves_paragraphs() {
        let text = plain_text_from_xml(
            r#"<w:p><w:r><w:t>第一段</w:t></w:r></w:p><w:p><w:r><w:t>第二段 &amp; 符号</w:t></w:r></w:p>"#,
        );
        assert!(text.contains("第一段"));
        assert!(text.contains("第二段 & 符号"));
    }

    #[test]
    fn office_minimal_docx_can_be_created_and_read() {
        let path = std::env::temp_dir().join(format!("lan-office-{}.docx", uuid::Uuid::new_v4()));
        create_minimal_docx(&path, "测试文档").unwrap();
        let document = read_ooxml_document(&path, "测试文档.docx", "docx").unwrap();
        assert_eq!(document.kind, "docx");
        assert!(document.text.contains("测试文档"));
        let xml = zip_text_entries(&path, |name| name == "word/document.xml")
            .unwrap()
            .remove(0)
            .1;
        assert!(xml.contains(r#"<w:pgSz w:w="11906" w:h="16838"/>"#));
        assert!(xml.contains(r#"<w:color w:val="1E4F91"/>"#));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn office_docx_style_write_keeps_package_readable() {
        let source =
            std::env::temp_dir().join(format!("lan-office-source-{}.docx", uuid::Uuid::new_v4()));
        let styled =
            std::env::temp_dir().join(format!("lan-office-styled-{}.docx", uuid::Uuid::new_v4()));
        create_minimal_docx(&source, "格式测试").unwrap();
        let applied = style_docx_text(
            &source,
            &styled,
            "格式测试",
            DocxTextStyle {
                font_family: Some("Microsoft YaHei"),
                font_size_pt: Some(18.0),
                bold: true,
                italic: false,
                underline: true,
            },
        )
        .unwrap();
        assert_eq!(applied, 1);
        let document = read_ooxml_document(&styled, "格式测试.docx", "docx").unwrap();
        assert!(document.text.contains("格式测试"));
        let xml = zip_text_entries(&styled, |name| name == "word/document.xml")
            .unwrap()
            .remove(0)
            .1;
        assert!(xml.contains("<w:b/>"));
        assert!(xml.contains(r#"<w:u w:val="single"/>"#));
        assert!(xml.contains("Microsoft YaHei"));
        let _ = std::fs::remove_file(source);
        let _ = std::fs::remove_file(styled);
    }

    #[test]
    fn office_minimal_spreadsheet_and_presentation_are_readable() {
        let xlsx = std::env::temp_dir().join(format!("lan-office-{}.xlsx", uuid::Uuid::new_v4()));
        let pptx = std::env::temp_dir().join(format!("lan-office-{}.pptx", uuid::Uuid::new_v4()));
        create_minimal_xlsx(&xlsx).unwrap();
        create_minimal_pptx(&pptx, "演示测试").unwrap();
        let sheet = read_ooxml_document(&xlsx, "测试.xlsx", "xlsx").unwrap();
        let slides = read_ooxml_document(&pptx, "测试.pptx", "pptx").unwrap();
        assert_eq!(sheet.kind, "xlsx");
        assert_eq!(slides.kind, "pptx");
        assert!(slides.text.contains("演示测试"));
        let slide_xml = zip_text_entries(&pptx, |name| name == "ppt/slides/slide1.xml")
            .unwrap()
            .remove(0)
            .1;
        assert!(slide_xml.contains(r#"<a:xfrm>"#));
        assert!(slide_xml.contains(r#"<a:srgbClr val="2F80ED"/>"#));
        assert!(slide_xml.contains(r#"sz="3200""#));
        let _ = std::fs::remove_file(xlsx);
        let _ = std::fs::remove_file(pptx);
    }
}
