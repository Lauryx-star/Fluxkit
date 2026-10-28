use super::{
    model::{Attachment, Collection},
    store::{CollectionView, ProjectStore, ProjectView},
};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncFailure {
    handle: String,
    message: String,
}

/// Server workspaces publish every saved change automatically; local projects are unaffected.
/// The local save already succeeded, so a failed publish is reported through the
/// `server-sync-failed` event instead of failing the save. The change stays in the workspace
/// and the next save (or Sync saved changes) publishes it.
async fn publish(app: &tauri::AppHandle, state: &State<'_, ProjectState>, handle: &str) {
    let check = handle.to_string();
    let server = run(state, move |store| {
        store.with_environment(&check, |root, _| {
            Ok(super::store::server_owner(root)?.is_some())
        })
    })
    .await;
    if !matches!(server, Ok(true)) {
        return;
    }
    let synced =
        crate::server::server_sync_project(app.clone(), state.clone(), handle.to_string()).await;
    if synced.is_ok() {
        // A sync raises the project revision that value writes must name; reload it now.
        if let Ok(Some(context)) = crate::server::values::context(state, handle, false).await {
            let _ = crate::server::values::refresh(app, &context).await;
        }
    }
    if let Err(error) = synced {
        let _ = app.emit(
            "server-sync-failed",
            SyncFailure {
                handle: handle.to_string(),
                message: format!("Saved in the workspace, but not yet on the server: {error}"),
            },
        );
    }
}

#[tauri::command]
pub async fn environment_action(
    app: tauri::AppHandle,
    state: State<'_, ProjectState>,
    handle: String,
    action: super::environments::Action,
) -> Result<super::environments::Reply, String> {
    let settings = app
        .path()
        .app_data_dir()
        .map_err(|_| "Could not locate local application settings.")?
        .join("environment-selection");
    let remote_context = crate::server::values::context(
        &state,
        &handle,
        matches!(
            action,
            super::environments::Action::Save { .. } | super::environments::Action::SetValue { .. }
        ),
    )
    .await?;
    let value_state = app
        .state::<crate::server::values::ValueState>()
        .inner()
        .clone();
    let publishes = matches!(action, super::environments::Action::Save { .. });
    let published = handle.clone();
    let action = if let Some(context) = remote_context {
        if let super::environments::Action::SetValue {
            variable,
            environment,
            value,
            expected,
            expected_value_revision,
        } = action
        {
            let saved = crate::server::values::set_value(
                &app,
                &context,
                &variable,
                environment.as_deref(),
                value.clone(),
                &expected,
                expected_value_revision,
            )
            .await;
            match saved {
                // The server rejected the slot (400, nothing written) because the variable or
                // environment only exists locally. Publish the saved workspace, then save once
                // more. Definitions on disk are unchanged, so `expected` stays valid.
                Err(error) if error == crate::server::values::UNSYNCED_DEFINITIONS => {
                    crate::server::server_sync_project(app.clone(), state.clone(), handle.clone())
                        .await
                        .map_err(|error| {
                            format!("The value was not saved: publishing the new variable definitions to the server failed. {error}")
                        })?;
                    let context = crate::server::values::context(&state, &handle, true)
                        .await?
                        .ok_or("This workspace is no longer a server workspace.")?;
                    crate::server::values::refresh(&app, &context).await?;
                    crate::server::values::set_value(
                        &app,
                        &context,
                        &variable,
                        environment.as_deref(),
                        value,
                        &expected,
                        expected_value_revision,
                    )
                    .await?;
                }
                other => other?,
            }
            super::environments::Action::Load
        } else {
            crate::server::values::refresh(&app, &context).await?;
            action
        }
    } else {
        action
    };
    let reply = run(&state, move |store| {
        if matches!(
            &action,
            super::environments::Action::Save { .. } | super::environments::Action::SetValue { .. }
        ) {
            store.ensure_writable(&handle)?;
        }
        store.with_environment(&handle, |root, project| {
            let mut reply = super::environments::apply(
                root,
                &settings,
                project,
                action,
                &crate::server::values::adapter(&value_state, root)?,
            )?;
            if let Some(identity) = super::store::server_owner(root)? {
                reply.view.server_values = Some(value_state.revisions(&identity, project)?);
            }
            Ok(reply)
        })
    })
    .await?;
    if publishes {
        publish(&app, &state, &published).await;
    }
    Ok(reply)
}

#[derive(Default)]
pub struct ProjectState(Arc<Mutex<ProjectStore>>);

#[tauri::command]
pub async fn restore_last_project(
    app: tauri::AppHandle,
    state: State<'_, ProjectState>,
) -> Result<Option<ProjectView>, String> {
    let settings = app
        .path()
        .app_data_dir()
        .map_err(|_| "Could not locate local workspace settings.")?;
    run(&state, move |store| {
        super::workspace::restore(&settings, store)
    })
    .await
}

#[tauri::command]
pub async fn remember_project(
    app: tauri::AppHandle,
    state: State<'_, ProjectState>,
    handle: Option<String>,
) -> Result<(), String> {
    let settings = app
        .path()
        .app_data_dir()
        .map_err(|_| "Could not locate local workspace settings.")?;
    run(&state, move |store| {
        super::workspace::remember(&settings, store, handle.as_deref())
    })
    .await
}

pub(crate) async fn run<T: Send + 'static>(
    state: &State<'_, ProjectState>,
    operation: impl FnOnce(&mut ProjectStore) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let store = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut store = store
            .lock()
            .map_err(|_| "Project state is unavailable. Restart Fluxkit.")?;
        operation(&mut store)
    })
    .await
    .map_err(|_| "Project operation could not finish. Your edits are still open.".to_string())?
}

#[tauri::command]
pub async fn choose_project(
    state: State<'_, ProjectState>,
    name: Option<String>,
) -> Result<Option<ProjectView>, String> {
    if let Some(name) = &name {
        super::model::validate_name(name)?;
    }
    let title = if name.is_some() {
        "Choose an empty folder for your new Fluxkit project"
    } else {
        "Open a folder containing project.fluxkit.json"
    };
    let Some(selected) = rfd::AsyncFileDialog::new()
        .set_title(title)
        .pick_folder()
        .await
    else {
        return Ok(None);
    };
    let path = selected.path().to_path_buf();
    run(&state, move |store| {
        if let Some(name) = name {
            store.create(&path, name)
        } else {
            store.open(&path)
        }
    })
    .await
    .map(Some)
}

#[tauri::command]
pub async fn reload_project(
    state: State<'_, ProjectState>,
    handle: String,
) -> Result<ProjectView, String> {
    run(&state, move |store| store.reload(&handle)).await
}

#[tauri::command]
pub async fn rename_project(
    app: tauri::AppHandle,
    state: State<'_, ProjectState>,
    handle: String,
    name: String,
    expected: String,
) -> Result<ProjectView, String> {
    let published = handle.clone();
    let view = run(&state, move |store| store.rename(&handle, name, &expected)).await?;
    publish(&app, &state, &published).await;
    Ok(view)
}

#[tauri::command]
pub async fn save_collection(
    app: tauri::AppHandle,
    state: State<'_, ProjectState>,
    handle: String,
    collection: Collection,
    expected: Option<String>,
) -> Result<CollectionView, String> {
    let published = handle.clone();
    let view = run(&state, move |store| {
        store.save_collection(&handle, collection, expected)
    })
    .await?;
    publish(&app, &state, &published).await;
    Ok(view)
}

#[tauri::command]
pub async fn remove_collection(
    app: tauri::AppHandle,
    state: State<'_, ProjectState>,
    handle: String,
    id: String,
    expected: String,
) -> Result<(), String> {
    let published = handle.clone();
    run(&state, move |store| {
        store.remove_collection(&handle, &id, &expected)
    })
    .await?;
    publish(&app, &state, &published).await;
    Ok(())
}

#[tauri::command]
pub async fn choose_attachment(
    state: State<'_, ProjectState>,
    handle: String,
) -> Result<Option<Attachment>, String> {
    // Reject unauthorized handles before displaying a file picker.
    let check = handle.clone();
    run(&state, move |store| store.reload(&check).map(|_| ())).await?;
    let Some(selected) = rfd::AsyncFileDialog::new()
        .set_title("Choose a request body file to copy into this project")
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let path = selected.path().to_path_buf();
    run(&state, move |store| store.attachment(&handle, &path))
        .await
        .map(Some)
}

#[tauri::command]
pub async fn close_project(
    app: tauri::AppHandle,
    state: State<'_, ProjectState>,
    handle: String,
) -> Result<(), String> {
    match crate::server::values::context(&state, &handle, false).await {
        Ok(Some(context)) => app
            .state::<crate::server::values::ValueState>()
            .lock(&context.identity)?,
        Ok(None) => {}
        // Closing a revoked handle must still work; it grants no access to its files.
        Err(_) => app.state::<crate::server::values::ValueState>().clear(),
    }
    run(&state, move |store| {
        store.close(&handle);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn save_project_auth(
    app: tauri::AppHandle,
    state: State<'_, ProjectState>,
    handle: String,
    auth: Option<super::model::Auth>,
    expected: String,
) -> Result<ProjectView, String> {
    let published = handle.clone();
    let view = run(&state, move |store| {
        store.save_auth(&handle, auth, &expected)
    })
    .await?;
    publish(&app, &state, &published).await;
    Ok(view)
}

#[tauri::command]
pub async fn move_request(
    app: tauri::AppHandle,
    state: State<'_, ProjectState>,
    handle: String,
    input: super::moving::MoveInput,
) -> Result<Vec<CollectionView>, String> {
    let owner = handle.clone();
    let published = handle.clone();
    let views = run(&state, move |store| {
        store.ensure_writable(&handle)?;
        store.with_environment(&handle, |root, _| super::moving::apply(root, &owner, input))
    })
    .await?;
    publish(&app, &state, &published).await;
    Ok(views)
}
