use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Runtime};

pub fn setup_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    #[cfg(target_os = "macos")]
    let app_name = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| app.package_info().name.clone());

    let mut submenus: Vec<Submenu<R>> = Vec::new();

    // 1. Application Menu (on macOS this appears under the application name)
    #[cfg(target_os = "macos")]
    {
        let app_menu = Submenu::with_items(
            app,
            &app_name,
            true,
            &[
                &PredefinedMenuItem::about(app, Some(&app_name), None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::services(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::hide(app, None)?,
                &PredefinedMenuItem::hide_others(app, None)?,
                &PredefinedMenuItem::show_all(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::quit(app, None)?,
            ],
        )?;
        submenus.push(app_menu);
    }

    // 2. Edit Menu (Правка) - standard system accelerators for text editing
    let edit_menu = Submenu::with_items(
        app,
        "Правка",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    submenus.push(edit_menu);

    // 3. View Menu (Вид) - navigation across the 3 main application sections
    let nav_conn = MenuItem::with_id(
        app,
        "menu_nav_connection",
        "Подключение",
        true,
        Some("CmdOrCtrl+1"),
    )?;
    let nav_profile = MenuItem::with_id(
        app,
        "menu_nav_profile",
        "Профиль",
        true,
        Some("CmdOrCtrl+2"),
    )?;
    let nav_logs = MenuItem::with_id(app, "menu_nav_logs", "Журнал", true, Some("CmdOrCtrl+3"))?;

    let view_menu = Submenu::with_items(app, "Вид", true, &[&nav_conn, &nav_profile, &nav_logs])?;
    submenus.push(view_menu);

    // 4. Profile Menu (Профиль) - profile save and JSON import
    let save_profile = MenuItem::with_id(
        app,
        "menu_profile_save",
        "Сохранить профиль",
        true,
        Some("CmdOrCtrl+S"),
    )?;
    let import_json = MenuItem::with_id(
        app,
        "menu_profile_import_json",
        "Импортировать JSON...",
        true,
        Some("CmdOrCtrl+O"),
    )?;

    let profile_menu = Submenu::with_items(app, "Профиль", true, &[&save_profile, &import_json])?;
    submenus.push(profile_menu);

    // 5. Window Menu (Окно) - minimize, zoom, close window (Cmd+W)
    let win_min = PredefinedMenuItem::minimize(app, None)?;
    let win_max = PredefinedMenuItem::maximize(app, None)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let win_close = PredefinedMenuItem::close_window(app, None)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let win_bring_all = PredefinedMenuItem::bring_all_to_front(app, None)?;

    let window_menu = Submenu::with_items(
        app,
        "Окно",
        true,
        &[&win_min, &win_max, &sep1, &win_close, &sep2, &win_bring_all],
    )?;
    submenus.push(window_menu);

    let menu_items: Vec<&dyn IsMenuItem<R>> =
        submenus.iter().map(|s| s as &dyn IsMenuItem<R>).collect();
    let menu = Menu::with_items(app, &menu_items)?;
    Ok(menu)
}
