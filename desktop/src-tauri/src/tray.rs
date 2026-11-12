use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Listener, Manager, Runtime};

use crate::status::{VpnState, VpnStatus};
use crate::vpn::{self, AppState};

static PRESENT: AtomicBool = AtomicBool::new(false);

pub fn set_present(value: bool) {
    PRESENT.store(value, Ordering::SeqCst);
}

pub fn is_present() -> bool {
    PRESENT.load(Ordering::SeqCst)
}

fn state_title(state: VpnState) -> &'static str {
    match state {
        VpnState::Disconnected => "Отключено",
        VpnState::Connecting => "Подключение...",
        VpnState::Connected => "Подключено",
        VpnState::Disconnecting => "Отключение...",
        VpnState::Error => "Ошибка",
    }
}

fn create_state_icon(state: VpnState) -> Image<'static> {
    const SIZE: u32 = 32;
    let (r, g, b) = match state {
        VpnState::Disconnected => (148, 163, 184), // Slate 400
        VpnState::Connecting => (245, 158, 11),    // Amber 500
        VpnState::Connected => (16, 185, 129),     // Emerald 500
        VpnState::Disconnecting => (249, 115, 22), // Orange 500
        VpnState::Error => (239, 68, 68),          // Red 500
    };

    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let cx = 15.5f32;
    let cy = 15.5f32;
    let radius = 10.0f32;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = ((y * SIZE + x) * 4) as usize;

            if dist <= radius - 0.5 {
                rgba[idx] = r;
                rgba[idx + 1] = g;
                rgba[idx + 2] = b;
                rgba[idx + 3] = 255;
            } else if dist < radius + 0.5 {
                let alpha = ((radius + 0.5 - dist) * 255.0).clamp(0.0, 255.0) as u8;
                rgba[idx] = r;
                rgba[idx + 1] = g;
                rgba[idx + 2] = b;
                rgba[idx + 3] = alpha;
            }
        }
    }

    Image::new_owned(rgba, SIZE, SIZE)
}

pub fn setup_tray<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<TrayIcon<R>, Box<dyn std::error::Error>> {
    let initial_status = if let Some(state) = app.try_state::<AppState>() {
        vpn::vpn_status_get(&state).unwrap_or_else(|_| VpnStatus::new(VpnState::Disconnected, ""))
    } else {
        VpnStatus::new(VpnState::Disconnected, "")
    };

    let status_info = MenuItem::with_id(
        app,
        "status_info",
        format!("Статус: {}", state_title(initial_status.state)),
        false,
        None::<&str>,
    )?;
    let show = MenuItem::with_id(app, "show", "Показать окно", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Подключить", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&status_info, &show, &toggle, &separator, &quit])?;

    let tray = TrayIconBuilder::with_id("main-tray")
        .icon(create_state_icon(initial_status.state))
        .tooltip(format!("Mieru: {}", state_title(initial_status.state)))
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event({
            let toggle = toggle.clone();
            move |app, event| match event.id.as_ref() {
                "show" => {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.unminimize();
                        let _ = w.set_focus();
                    }
                }
                "toggle" => {
                    if let Some(state) = app.try_state::<AppState>() {
                        let state = state.inner().clone();
                        let current = vpn::vpn_status_get(&state)
                            .unwrap_or_else(|_| VpnStatus::new(VpnState::Disconnected, ""));
                        match current.state {
                            VpnState::Connected | VpnState::Connecting => {
                                let _ = toggle.set_text("Отключение...");
                                let _ = toggle.set_enabled(false);
                                tauri::async_runtime::spawn_blocking(move || {
                                    let _ = vpn::vpn_disconnect(&state);
                                });
                            }
                            VpnState::Disconnected | VpnState::Disconnecting | VpnState::Error => {
                                let _ = toggle.set_text("Подключение...");
                                let _ = toggle.set_enabled(true);
                                tauri::async_runtime::spawn_blocking(move || {
                                    let _ = vpn::vpn_connect(&state);
                                });
                            }
                        }
                    }
                }
                "quit" => {
                    if let Some(state) = app.try_state::<AppState>() {
                        let state = state.inner().clone();
                        vpn::shutdown_for_exit(&state);
                    }
                    app.exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;

    // Reactive update via vpn-status event
    let status_info_item = status_info.clone();
    let toggle_item = toggle.clone();
    let tray_handle = Arc::new(tray.clone());

    app.listen("vpn-status", move |event| {
        if let Ok(status) = serde_json::from_str::<VpnStatus>(event.payload()) {
            let _ = status_info_item.set_text(format!("Статус: {}", state_title(status.state)));
            let _ = tray_handle.set_icon(Some(create_state_icon(status.state)));
            let _ = tray_handle.set_tooltip(Some(format!("Mieru: {}", state_title(status.state))));

            match status.state {
                VpnState::Disconnected | VpnState::Error => {
                    let _ = toggle_item.set_text("Подключить");
                    let _ = toggle_item.set_enabled(true);
                }
                VpnState::Connecting => {
                    let _ = toggle_item.set_text("Отмена подключения");
                    let _ = toggle_item.set_enabled(true);
                }
                VpnState::Connected => {
                    let _ = toggle_item.set_text("Отключить");
                    let _ = toggle_item.set_enabled(true);
                }
                VpnState::Disconnecting => {
                    let _ = toggle_item.set_text("Отключение...");
                    let _ = toggle_item.set_enabled(false);
                }
            }
        }
    });

    Ok(tray)
}
