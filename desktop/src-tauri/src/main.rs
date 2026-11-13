#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // Avoid WebKit DMA-BUF startup failures on Linux when no override is set.
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    #[cfg(target_os = "macos")]
    if mieru_desktop_lib::supervisor_requested() {
        std::process::exit(mieru_desktop_lib::run_supervisor());
    }
    #[cfg(target_os = "linux")]
    if std::env::args().any(|arg| arg == "--mieru-supervisor") {
        eprintln!("mieru: on Linux the privileged helper is /usr/lib/mieru/mieru-helper");
        std::process::exit(1);
    }
    mieru_desktop_lib::run();
}
