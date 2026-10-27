use super::{
    model::{Attachment, Collection},
    store::{CollectionView, ProjectStore, ProjectView},
};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

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
    let action = if let Some(context) = remote_context {
        if let super::environments::Action::SetValue {
            variable,
            environment,
            value,
            expected,
            expected_value_revision,
        } = action
        {
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
            super::environments::Action::Load
        } else {
            crate::server::values::refresh(&app, &context).await?;
            action
        }
    } else {
        action
    };
    run(&state, move |store| {
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
    .await
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
    state: State<'_, ProjectState>,
    handle: String,
    name: String,
    expected: String,
) -> Result<ProjectView, String> {
    run(&state, move |store| store.rename(&handle, name, &expected)).await
}

#[tauri::command]
pub async fn save_collection(
    state: State<'_, ProjectState>,
    handle: String,
    collection: Collection,
    expected: Option<String>,
) -> Result<CollectionView, String> {
    run(&state, move |store| {
        store.save_collection(&handle, collection, expected)
    })
    .await
}

#[tauri::command]
pub async fn remove_collection(
    state: State<'_, ProjectState>,
    handle: String,
    id: String,
    expected: String,
) -> Result<(), String> {
    run(&state, move |store| {
        store.remove_collection(&handle, &id, &expected)
    })
    .await
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
    state: State<'_, ProjectState>,
    handle: String,
    auth: Option<super::model::Auth>,
    expected: String,
) -> Result<ProjectView, String> {
    run(&state, move |store| {
        store.save_auth(&handle, auth, &expected)
    })
    .await
}

#[tauri::command]
pub async fn move_request(
    state: State<'_, ProjectState>,
    handle: String,
    input: super::moving::MoveInput,
) -> Result<Vec<CollectionView>, String> {
    let owner = handle.clone();
    run(&state, move |store| {
        store.ensure_writable(&handle)?;
        store.with_environment(&handle, |root, _| super::moving::apply(root, &owner, input))
    })
    .await
}
