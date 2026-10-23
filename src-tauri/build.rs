fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "app_info",
            "choose_project",
            "reload_project",
            "rename_project",
            "save_project_auth",
            "save_collection",
            "remove_collection",
            "choose_attachment",
            "close_project",
            "environment_action",
            "begin_http_request",
            "execute_http_request",
            "cancel_http_request",
            "authentication_action",
            "history_action",
        ]),
    ))
    .expect("failed to generate Tauri build metadata");
}
