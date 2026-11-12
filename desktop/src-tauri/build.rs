fn main() {
    // Only these invoke handlers are exposed to the webview.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "profile_get",
            "profile_set",
            "vpn_connect",
            "vpn_disconnect",
            "vpn_status_get",
            "vpn_log_get",
            "server_health_check",
        ]),
    ))
    .expect("failed to build the Tauri context");
}
