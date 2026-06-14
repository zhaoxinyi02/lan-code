#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use lan_core::{
    AgentCore, ModelMessage, ModelProvider, ModelRequest, OpenAiCompatibleProvider, SqliteStore,
};
use lan_protocol::{
    ApprovalDecision, ApprovalMode, ApprovalRequest, CoreEvent, Session, SessionId, TurnResult,
};
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopProject {
    name: String,
    path: String,
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
    projects: Vec<DesktopProject>,
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
            projects: Vec::new(),
        }
    }
}

struct AppState {
    core: RwLock<Arc<AgentCore>>,
    settings: RwLock<DesktopSettings>,
    data_dir: RwLock<PathBuf>,
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

fn build_core(settings: &DesktopSettings, data_dir: &Path) -> Result<AgentCore, String> {
    fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    let store =
        SqliteStore::open(data_dir.join("lan-code.sqlite")).map_err(|error| error.to_string())?;
    if settings.api_key.trim().is_empty() && settings.provider != "ollama" {
        return AgentCore::with_store(store)
            .map(|core| core.with_max_provider_rounds(settings.max_provider_rounds))
            .map_err(|error| error.to_string());
    }
    let api_key = if settings.api_key.trim().is_empty() {
        "ollama".into()
    } else {
        settings.api_key.clone()
    };
    let provider =
        OpenAiCompatibleProvider::new(settings.base_url.clone(), api_key, settings.model.clone())
            .map_err(|error| error.to_string())?;
    AgentCore::with_provider_and_store(Arc::new(provider), store)
        .map(|core| core.with_max_provider_rounds(settings.max_provider_rounds))
        .map_err(|error| error.to_string())
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
async fn test_provider(settings: DesktopSettings) -> Result<String, String> {
    if settings.api_key.trim().is_empty() && settings.provider != "ollama" {
        return Err("请先填写 API Key".into());
    }
    let api_key = if settings.api_key.trim().is_empty() {
        "ollama".into()
    } else {
        settings.api_key
    };
    let provider = OpenAiCompatibleProvider::new(settings.base_url, api_key, settings.model)
        .map_err(|error| error.to_string())?;
    let response = provider
        .complete(ModelRequest {
            messages: vec![ModelMessage::text(
                lan_core::ModelRole::User,
                "Reply with exactly: connected",
            )],
            tools: Vec::new(),
        })
        .await
        .map_err(|error| error.to_string())?;
    Ok(response.text)
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
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            test_provider,
            pick_workspace,
            pick_data_dir,
            list_sessions,
            create_session,
            delete_session,
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
