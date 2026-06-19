#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{Emitter, Manager};
use winreg::{RegKey, enums::HKEY_CURRENT_USER};

const APP_NAME: &str = "Lan Code";
const VERSION: &str = "0.2.10";
const CREATE_NO_WINDOW: u32 = 0x08000000;
const PAYLOAD: &[u8] = include_bytes!("../../../../target/release/lan-desktop.exe");

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressPayload {
    percent: u8,
    label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallResult {
    install_dir: String,
    executable: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstalledInfo {
    installed: bool,
    version: String,
    install_dir: String,
    action: String,
}

#[derive(Serialize, Deserialize)]
struct InstallMarker {
    product: String,
    version: String,
}

const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Lan Code";
const INSTALL_MARKER: &str = ".lancode-install.json";

fn local_app_data() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "无法定位当前用户的 LocalAppData 目录。".to_string())
}

fn default_dir() -> Result<PathBuf, String> {
    Ok(local_app_data()?.join("Programs").join(APP_NAME))
}

fn desktop_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|path| path.join("Desktop"))
}

fn start_menu_dir() -> Result<PathBuf, String> {
    Ok(std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "无法定位开始菜单目录。".to_string())?
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs"))
}

fn emit_progress(app: &tauri::AppHandle, percent: u8, label: impl Into<String>) {
    let _ = app.emit(
        "install-progress",
        ProgressPayload {
            percent,
            label: label.into(),
        },
    );
}

fn powershell(script: &str) -> Result<(), String> {
    let status = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|error| format!("无法创建快捷方式：{error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("创建快捷方式时系统返回了错误。".to_string())
    }
}

fn stop_running_app() {
    let _ = Command::new("taskkill.exe")
        .args(["/F", "/IM", "Lan Code.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

fn create_shortcut(shortcut: &Path, target: &Path, working_dir: &Path) -> Result<(), String> {
    let shortcut = shortcut.to_string_lossy().replace('\'', "''");
    let target = target.to_string_lossy().replace('\'', "''");
    let working_dir = working_dir.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$w=New-Object -ComObject WScript.Shell;$s=$w.CreateShortcut('{shortcut}');$s.TargetPath='{target}';$s.WorkingDirectory='{working_dir}';$s.IconLocation='{target},0';$s.Save()"
    );
    powershell(&script)
}

fn register_uninstaller(
    install_dir: &Path,
    executable: &Path,
    uninstaller: &Path,
) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(UNINSTALL_KEY)
        .map_err(|error| error.to_string())?;
    key.set_value("DisplayName", &APP_NAME)
        .map_err(|error| error.to_string())?;
    key.set_value("DisplayVersion", &VERSION)
        .map_err(|error| error.to_string())?;
    key.set_value("Publisher", &APP_NAME)
        .map_err(|error| error.to_string())?;
    key.set_value(
        "InstallLocation",
        &install_dir.to_string_lossy().to_string(),
    )
    .map_err(|error| error.to_string())?;
    key.set_value("DisplayIcon", &executable.to_string_lossy().to_string())
        .map_err(|error| error.to_string())?;
    key.set_value(
        "UninstallString",
        &format!("\"{}\" --uninstall", uninstaller.display()),
    )
    .map_err(|error| error.to_string())?;
    key.set_value("NoModify", &1u32)
        .map_err(|error| error.to_string())?;
    key.set_value("NoRepair", &1u32)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn remove_context_menus() {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for path in [
        r"Software\Classes\*\shell\LanCode",
        r"Software\Classes\Directory\shell\LanCode",
        r"Software\Classes\Directory\Background\shell\LanCode",
    ] {
        let _ = hkcu.delete_subkey_all(path);
    }
}

fn register_context_menus(executable: &Path) -> Result<(), String> {
    remove_context_menus();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let executable = executable.to_string_lossy();
    for (path, argument) in [
        (r"Software\Classes\*\shell\LanCode", "%1"),
        (r"Software\Classes\Directory\shell\LanCode", "%1"),
        (
            r"Software\Classes\Directory\Background\shell\LanCode",
            "%V",
        ),
    ] {
        let (key, _) = hkcu.create_subkey(path).map_err(|error| error.to_string())?;
        key.set_value("", &"用 Lan Code 打开")
            .map_err(|error| error.to_string())?;
        key.set_value("Icon", &format!("{executable},0"))
            .map_err(|error| error.to_string())?;
        let (command, _) = key
            .create_subkey("command")
            .map_err(|error| error.to_string())?;
        command
            .set_value("", &format!("\"{executable}\" --open \"{argument}\""))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn installed_info_value() -> InstalledInfo {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey(UNINSTALL_KEY) else {
        return InstalledInfo {
            installed: false,
            version: String::new(),
            install_dir: String::new(),
            action: "install".into(),
        };
    };
    let version: String = key.get_value("DisplayVersion").unwrap_or_default();
    let install_dir: String = key.get_value("InstallLocation").unwrap_or_default();
    let installed =
        !install_dir.trim().is_empty() && Path::new(&install_dir).join("Lan Code.exe").is_file();
    let action = if !installed {
        "install"
    } else if version == VERSION {
        "repair"
    } else {
        "update"
    };
    InstalledInfo {
        installed,
        version,
        install_dir,
        action: action.into(),
    }
}

fn is_uninstaller() -> bool {
    if std::env::args().any(|arg| arg == "--uninstall") {
        return true;
    }
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
        .is_some_and(|name| name.to_ascii_lowercase().contains("uninstall"))
}

#[tauri::command]
fn installer_mode() -> &'static str {
    if is_uninstaller() {
        "uninstall"
    } else {
        "install"
    }
}

#[tauri::command]
fn installed_info() -> InstalledInfo {
    installed_info_value()
}

#[tauri::command]
fn default_install_dir() -> Result<String, String> {
    let installed = installed_info_value();
    if installed.installed {
        return Ok(installed.install_dir);
    }
    Ok(default_dir()?.to_string_lossy().to_string())
}

#[tauri::command]
fn choose_install_dir(current: String) -> Option<String> {
    let mut dialog = rfd::FileDialog::new();
    if !current.trim().is_empty() {
        dialog = dialog.set_directory(current);
    }
    dialog
        .pick_folder()
        .map(|path| path.to_string_lossy().to_string())
}

#[tauri::command]
async fn install_app(
    app: tauri::AppHandle,
    install_dir: String,
    desktop_shortcut: bool,
    start_menu_shortcut: bool,
    context_menus: bool,
) -> Result<InstallResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let install_dir = PathBuf::from(install_dir);
        if install_dir.as_os_str().is_empty() {
            return Err("请选择有效的安装目录。".to_string());
        }

        let previous = installed_info_value();
        emit_progress(&app, 4, "正在准备安装目录...");
        fs::create_dir_all(&install_dir).map_err(|error| format!("无法创建安装目录：{error}"))?;
        let executable = install_dir.join("Lan Code.exe");
        let temporary = install_dir.join("Lan Code.exe.installing");
        let uninstaller = install_dir.join("Uninstall Lan Code.exe");

        stop_running_app();
        let mut file =
            fs::File::create(&temporary).map_err(|error| format!("无法写入程序文件：{error}"))?;
        let chunks = PAYLOAD.chunks(256 * 1024);
        let total_chunks = chunks.len().max(1);
        for (index, chunk) in chunks.enumerate() {
            file.write_all(chunk)
                .map_err(|error| format!("写入程序文件失败：{error}"))?;
            let percent = 8 + ((index + 1) * 68 / total_chunks) as u8;
            emit_progress(&app, percent, "正在写入 Lan Code 核心文件...");
        }
        file.flush().map_err(|error| error.to_string())?;
        drop(file);

        if executable.exists() {
            let _ = fs::remove_file(&executable);
        }
        fs::rename(&temporary, &executable)
            .map_err(|error| format!("无法完成程序文件安装：{error}"))?;

        emit_progress(&app, 80, "正在配置卸载程序...");
        let current_exe = std::env::current_exe().map_err(|error| error.to_string())?;
        fs::copy(&current_exe, &uninstaller)
            .map_err(|error| format!("无法创建卸载程序：{error}"))?;
        let marker = serde_json::to_vec_pretty(&InstallMarker {
            product: APP_NAME.into(),
            version: VERSION.into(),
        })
        .map_err(|error| error.to_string())?;
        fs::write(install_dir.join(INSTALL_MARKER), marker)
            .map_err(|error| format!("无法写入安装标记：{error}"))?;

        emit_progress(&app, 86, "正在创建快捷方式...");
        if desktop_shortcut {
            if let Some(desktop) = desktop_dir() {
                fs::create_dir_all(&desktop).map_err(|error| error.to_string())?;
                create_shortcut(&desktop.join("Lan Code.lnk"), &executable, &install_dir)?;
            }
        }
        if start_menu_shortcut {
            let start_menu = start_menu_dir()?.join(APP_NAME);
            fs::create_dir_all(&start_menu).map_err(|error| error.to_string())?;
            create_shortcut(&start_menu.join("Lan Code.lnk"), &executable, &install_dir)?;
            create_shortcut(
                &start_menu.join("卸载 Lan Code.lnk"),
                &uninstaller,
                &install_dir,
            )?;
        }

        emit_progress(&app, 94, "正在注册应用信息...");
        register_uninstaller(&install_dir, &executable, &uninstaller)?;
        if context_menus {
            register_context_menus(&executable)?;
        } else {
            remove_context_menus();
        }
        if previous.installed {
            let old_dir = PathBuf::from(previous.install_dir);
            if old_dir != install_dir {
                let _ = fs::remove_file(old_dir.join("Lan Code.exe"));
                let _ = fs::remove_file(old_dir.join("Uninstall Lan Code.exe"));
                let _ = fs::remove_file(old_dir.join(INSTALL_MARKER));
                let _ = fs::remove_dir(&old_dir);
            }
        }
        emit_progress(&app, 100, "Lan Code 已准备就绪");

        Ok(InstallResult {
            install_dir: install_dir.to_string_lossy().to_string(),
            executable: executable.to_string_lossy().to_string(),
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn launch_installed(executable: String) -> Result<(), String> {
    Command::new(executable)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法启动 Lan Code：{error}"))
}

#[tauri::command]
fn uninstall_app() -> Result<(), String> {
    let install_dir = std::env::current_exe()
        .map_err(|error| error.to_string())?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "无法定位 Lan Code 安装目录。".to_string())?;

    if let Some(desktop) = desktop_dir() {
        let _ = fs::remove_file(desktop.join("Lan Code.lnk"));
    }
    if let Ok(start_menu) = start_menu_dir() {
        let _ = fs::remove_dir_all(start_menu.join(APP_NAME));
    }
    stop_running_app();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let _ = hkcu.delete_subkey_all(UNINSTALL_KEY);
    remove_context_menus();
    let _ = fs::remove_file(install_dir.join("Lan Code.exe"));
    let _ = fs::remove_file(install_dir.join(INSTALL_MARKER));

    let directory = install_dir.to_string_lossy().replace('\'', "''");
    let uninstaller = install_dir
        .join("Uninstall Lan Code.exe")
        .to_string_lossy()
        .replace('\'', "''");
    let process_id = std::process::id();
    let script = format!(
        "while (Get-Process -Id {process_id} -ErrorAction SilentlyContinue) {{ Start-Sleep -Milliseconds 250 }}; Remove-Item -LiteralPath '{uninstaller}' -Force -ErrorAction SilentlyContinue; Remove-Item -LiteralPath '{directory}' -ErrorAction SilentlyContinue"
    );
    Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            installer_mode,
            installed_info,
            default_install_dir,
            choose_install_dir,
            install_app,
            launch_installed,
            uninstall_app
        ])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title(if installer_mode() == "uninstall" {
                    "卸载 Lan Code"
                } else {
                    "安装 Lan Code"
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Lan Code installer");
}
