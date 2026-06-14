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
struct DesktopSettings {
    base_url: String,
    model: String,
    api_key: String,
    workspace: String,
    approval_mode: ApprovalMode,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            base_url: "https://api.deepseek.com".into(),
            model: "deepseek-v4-pro".into(),
            api_key: String::new(),
            workspace: std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .display()
                .to_string(),
            approval_mode: ApprovalMode::ReadOnly,
        }
    }
}

struct AppState {
    core: RwLock<Arc<AgentCore>>,
    settings: RwLock<DesktopSettings>,
    data_dir: PathBuf,
}

fn app_data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Lan Code")
}

fn load_settings(data_dir: &Path) -> DesktopSettings {
    let mut settings: DesktopSettings = fs::read_to_string(data_dir.join("settings.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    settings.api_key = keyring::Entry::new("Lan Code", "provider-api-key")
        .ok()
        .and_then(|entry| entry.get_password().ok())
        .unwrap_or_default();
    settings
}

fn build_core(settings: &DesktopSettings, data_dir: &Path) -> Result<AgentCore, String> {
    fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    let store =
        SqliteStore::open(data_dir.join("lan-code.sqlite")).map_err(|error| error.to_string())?;
    if settings.api_key.trim().is_empty() {
        return AgentCore::with_store(store).map_err(|error| error.to_string());
    }
    let provider = OpenAiCompatibleProvider::new(
        settings.base_url.clone(),
        settings.api_key.clone(),
        settings.model.clone(),
    )
    .map_err(|error| error.to_string())?;
    AgentCore::with_provider_and_store(Arc::new(provider), store).map_err(|error| error.to_string())
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
    settings: DesktopSettings,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if settings.base_url.trim().is_empty() || settings.model.trim().is_empty() {
        return Err("API 地址和模型名称不能为空".into());
    }
    if !Path::new(&settings.workspace).is_dir() {
        return Err("工作区目录不存在".into());
    }
    fs::create_dir_all(&state.data_dir).map_err(|error| error.to_string())?;
    let credential =
        keyring::Entry::new("Lan Code", "provider-api-key").map_err(|error| error.to_string())?;
    if settings.api_key.trim().is_empty() {
        let _ = credential.delete_credential();
    } else {
        credential
            .set_password(&settings.api_key)
            .map_err(|error| error.to_string())?;
    }
    let mut persisted = settings.clone();
    persisted.api_key.clear();
    fs::write(
        state.data_dir.join("settings.json"),
        serde_json::to_vec_pretty(&persisted).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let next_core = build_core(&settings, &state.data_dir)?;
    *state.core.write().await = Arc::new(next_core);
    *state.settings.write().await = settings;
    Ok(())
}

#[tauri::command]
async fn test_provider(settings: DesktopSettings) -> Result<String, String> {
    if settings.api_key.trim().is_empty() {
        return Err("请先填写 API Key".into());
    }
    let provider =
        OpenAiCompatibleProvider::new(settings.base_url, settings.api_key, settings.model)
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
    let data_dir = app_data_dir();
    let settings = load_settings(&data_dir);
    let core = build_core(&settings, &data_dir).unwrap_or_else(|_| AgentCore::new());
    tauri::Builder::default()
        .manage(AppState {
            core: RwLock::new(Arc::new(core)),
            settings: RwLock::new(settings),
            data_dir,
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            test_provider,
            pick_workspace,
            list_sessions,
            create_session,
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
