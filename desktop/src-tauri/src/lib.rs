#[cfg(target_os = "macos")]
mod escape;
mod health;
#[cfg(target_os = "macos")]
mod identity;
mod menu;
#[cfg(target_os = "macos")]
mod privilege;
#[cfg(target_os = "macos")]
mod supervisor;
mod tray;
#[cfg(target_os = "linux")]
#[path = "vpn_linux.rs"]
mod vpn;
#[cfg(target_os = "macos")]
mod vpn;

pub use mieru_shared::{core_config, fsutil, profile, redact, status};

use tauri::{Emitter, Manager};

use profile::{Profile, ProfileView};
use status::VpnStatus;
use vpn::{AppState, LogView};

#[cfg(target_os = "macos")]
pub fn supervisor_requested() -> bool {
    supervisor::requested()
}

#[cfg(target_os = "macos")]
pub fn run_supervisor() -> i32 {
    supervisor::run()
}

#[tauri::command]
fn profile_get(state: tauri::State<'_, AppState>) -> Result<Option<ProfileView>, String> {
    vpn::profile_get(&state)
}

#[tauri::command]
fn profile_set(state: tauri::State<'_, AppState>, profile: Profile) -> Result<ProfileView, String> {
    vpn::profile_set(&state, profile)
}

#[tauri::command]
async fn vpn_connect(state: tauri::State<'_, AppState>) -> Result<VpnStatus, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || vpn::vpn_connect(&state))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn vpn_disconnect(state: tauri::State<'_, AppState>) -> Result<VpnStatus, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || vpn::vpn_disconnect(&state))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn vpn_status_get(state: tauri::State<'_, AppState>) -> Result<VpnStatus, String> {
    vpn::vpn_status_get(&state)
}

#[tauri::command]
fn vpn_log_get(state: tauri::State<'_, AppState>) -> Result<LogView, String> {
    vpn::vpn_log_get(&state)
}

#[tauri::command]
async fn server_health_check(
    state: tauri::State<'_, AppState>,
) -> Result<health::ServerHealth, String> {
    health::check(state.inner().clone()).await
}

pub fn run() {
    let state = AppState::new();
    tauri::Builder::default()
        .manage(state)
        .menu(|app| menu::setup_menu(app))
        .on_menu_event(|app, event| {
            let focus_main_window = || {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            };

            match event.id.as_ref() {
                "menu_nav_connection" => {
                    let _ = app.emit("menu-nav", "connection");
                    focus_main_window();
                }
                "menu_nav_profile" => {
                    let _ = app.emit("menu-nav", "profile");
                    focus_main_window();
                }
                "menu_nav_logs" => {
                    let _ = app.emit("menu-nav", "logs");
                    focus_main_window();
                }
                "menu_profile_save" => {
                    let _ = app.emit("menu-profile-save", ());
                    focus_main_window();
                }
                "menu_profile_import_json" => {
                    let _ = app.emit("menu-profile-import-json", ());
                    focus_main_window();
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            profile_get,
            profile_set,
            vpn_connect,
            vpn_disconnect,
            vpn_status_get,
            vpn_log_get,
            server_health_check
        ])
        .setup(|app| {
            let state = app.state::<AppState>();
            vpn::attach(app.handle(), &state);
            match tray::setup_tray(app.handle()) {
                Ok(_) => tray::set_present(true),
                Err(_) => tray::set_present(false),
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if tray::is_present() {
                    let _ = window.hide();
                } else {
                    let app = window.app_handle();
                    if let Some(state) = app.try_state::<AppState>() {
                        vpn::shutdown_for_exit(&state);
                    }
                    app.exit(0);
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to start Mieru")
        .run(|app, event| match event {
            tauri::RunEvent::Exit => {
                if let Some(state) = app.try_state::<AppState>() {
                    vpn::shutdown_for_exit(&state);
                }
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
            _ => {}
        });
}
