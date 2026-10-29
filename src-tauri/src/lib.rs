mod exports;
mod history;
mod http;
mod imports;
mod projects;
mod server;

use http::commands::*;
use projects::commands::*;
use serde::Serialize;
use tauri::Manager;

/// Read-only application metadata exposed to the local user interface.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    name: &'static str,
    version: &'static str,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: "Fluxkit",
        version: env!("CARGO_PKG_VERSION"),
    }
}

/// Starts the desktop application with project-scoped native operations.
pub fn run() {
    let application = tauri::Builder::default()
        .manage(ProjectState::default())
        .manage(server::oidc::OidcState::default())
        .manage(server::values::ValueState::default())
        .manage(server::workspace::SyncState::default())
        .manage(server::transfer::TransferState::default())
        .manage(HttpState::default())
        .manage(imports::commands::ImportState::default())
        .manage(exports::commands::ExportState::default())
        .setup(|app| {
            let state = history::HistoryState::new(
                app.path()
                    .app_data_dir()
                    .ok()
                    .map(|root| root.join("history")),
            );
            state.start_cleanup();
            app.manage(state);
            Ok(())
        })
        .menu(|app| {
            let menu = tauri::menu::Menu::default(app)?;
            #[cfg(target_os = "macos")]
            if let Some(tauri::menu::MenuItemKind::Submenu(application)) = menu.items()?.first() {
                // AppKit's predefined Quit terminates directly, bypassing cleanup and draft guards.
                let count = application.items()?.len();
                if count > 0 {
                    application.remove_at(count - 1)?;
                }
                application.append(&tauri::menu::MenuItem::with_id(
                    app,
                    "fluxkit-quit",
                    "Quit Fluxkit",
                    true,
                    Some("CmdOrCtrl+Q"),
                )?)?;
            }
            Ok(menu)
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "fluxkit-quit" {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            server::values::server_vault_action,
            server::server_connection_get,
            server::server_connection_set,
            server::server_register,
            server::server_login,
            server::oidc::server_auth_mode,
            server::oidc::server_oidc_login,
            server::oidc::server_oidc_cancel,
            server::server_logout,
            server::server_projects,
            server::server_open_project,
            server::workspace::server_refresh_workspace,
            server::server_delete_project,
            server::transfer::server_preview_transfer,
            server::transfer::server_prepare_transfer,
            server::transfer::server_commit_transfer,
            server::transfer::server_discard_transfer,
            server::server_project_members,
            server::server_add_project_member,
            server::server_update_project_member,
            server::server_remove_project_member,
            server::server_account,
            server::server_admin_users,
            server::server_admin_approve_user,
            server::server_admin_set_user_status,
            server::server_admin_delete_user,
            server::server_create_team_project,
            exports::commands::preview_openapi_export,
            exports::commands::save_openapi_export,
            exports::commands::discard_openapi_export,
            imports::commands::choose_openapi_import,
            imports::commands::rename_import_variable,
            imports::commands::choose_postman_import,
            imports::commands::commit_openapi_import,
            imports::commands::discard_openapi_import,
            choose_project,
            restore_last_project,
            remember_project,
            save_session,
            restore_session,
            reload_project,
            rename_project,
            save_project_auth,
            save_collection,
            move_request,
            remove_collection,
            choose_attachment,
            close_project,
            environment_action,
            begin_http_request,
            execute_http_request,
            preview_http_headers,
            test_authentication,
            cancel_http_request,
            authentication_action,
            history::commands::history_action
        ])
        .build(tauri::generate_context!());
    match application {
        Ok(application) => application.run(|handle, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                handle
                    .state::<HttpState>()
                    .shutdown(handle, &api, code.unwrap_or(0));
            }
        }),
        Err(error) => {
            eprintln!("Fluxkit could not start: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::app_info;

    #[test]
    fn metadata_matches_the_frontend_contract() {
        let value = serde_json::to_value(app_info()).expect("metadata must serialize");
        assert_eq!(value["name"], "Fluxkit");
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(value.as_object().expect("object").len(), 2);
    }
}
