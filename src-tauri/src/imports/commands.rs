use super::{document, mapping, postman, storage, CommitResult, Plan, Preview};
use crate::projects::{
    commands::{run, ProjectState},
    environments, store,
};
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};
use tauri::State;

#[derive(Default)]
pub struct ImportState {
    plans: Mutex<HashMap<String, Plan>>,
    operation: AtomicBool,
}
struct Operation<'a>(&'a AtomicBool);
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
impl ImportState {
    fn begin(&self) -> Result<Operation<'_>, String> {
        if self.operation.swap(true, Ordering::SeqCst) {
            return Err("Another import operation is in progress. Finish it first.".into());
        }
        Ok(Operation(&self.operation))
    }
}
fn prune(plans: &mut HashMap<String, Plan>) {
    plans.retain(|_, p| p.created.elapsed() < Duration::from_secs(600));
}
#[tauri::command]
pub async fn choose_openapi_import(
    projects: State<'_, ProjectState>,
    state: State<'_, ImportState>,
    handle: String,
) -> Result<Option<Preview>, String> {
    choose(projects, state, handle, false, None).await
}
#[tauri::command]
pub async fn choose_postman_import(
    projects: State<'_, ProjectState>,
    state: State<'_, ImportState>,
    handle: String,
    collection_id: Option<String>,
) -> Result<Option<Preview>, String> {
    if let Some(id) = &collection_id {
        crate::projects::model::validate_id(id)?;
    }
    choose(projects, state, handle, true, collection_id).await
}
async fn choose(
    projects: State<'_, ProjectState>,
    state: State<'_, ImportState>,
    handle: String,
    postman_source: bool,
    collection_id: Option<String>,
) -> Result<Option<Preview>, String> {
    let _operation = state.begin()?;
    {
        let mut plans = state
            .plans
            .lock()
            .map_err(|_| "Import state is unavailable.")?;
        prune(&mut plans);
        if plans.len() >= 2 {
            return Err("Close an existing import preview before opening another.".into());
        }
    }
    let context = handle.clone();
    let (project, definitions, revision) = run(&projects, move |store| {
        store.with_environment(&context, |root, _| {
            let (definitions, revision) = environments::definitions(root)?;
            Ok((store::load(root, &context)?, definitions, revision))
        })
    })
    .await?;
    let Some(selected) = rfd::AsyncFileDialog::new()
        .set_title(if postman_source {
            "Preview a Postman export"
        } else {
            "Preview an OpenAPI document"
        })
        .add_filter(
            if postman_source {
                "Postman JSON"
            } else {
                "OpenAPI YAML / JSON"
            },
            if postman_source {
                &["json"][..]
            } else {
                &["yaml", "yml", "json"][..]
            },
        )
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let path = selected.path().to_path_buf();
    let plan = tauri::async_runtime::spawn_blocking(move || {
        let file = File::open(path).map_err(|_| "Could not open the selected import file.")?;
        let metadata = file
            .metadata()
            .map_err(|_| "Could not inspect the selected import file.")?;
        if !metadata.is_file() || metadata.len() > document::INPUT_LIMIT as u64 {
            return Err("Choose a regular import file up to 4 MiB.".into());
        }
        let mut bytes = Vec::new();
        file.take(document::INPUT_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "Could not read the selected import file.")?;
        if postman_source {
            if let Some(id) = collection_id {
                postman::environment(&bytes, &project, definitions, revision, &id)
            } else {
                postman::collection(&bytes, &project, definitions, revision)
            }
        } else {
            mapping::plan(&bytes, &project, definitions, revision)
        }
    })
    .await
    .map_err(|_| "Could not prepare the import preview.")??;
    let preview = plan.preview.clone();
    let mut plans = state
        .plans
        .lock()
        .map_err(|_| "Import state is unavailable.")?;
    prune(&mut plans);
    if plans.len() >= 2 {
        return Err("Close an existing import preview before opening another.".into());
    }
    plans.insert(preview.id.clone(), plan);
    Ok(Some(preview))
}
#[tauri::command]
pub async fn commit_openapi_import(
    projects: State<'_, ProjectState>,
    state: State<'_, ImportState>,
    handle: String,
    id: String,
    store_values: bool,
) -> Result<CommitResult, String> {
    let _operation = state.begin()?;
    crate::projects::model::validate_id(&id)?;
    let mut plan = {
        let mut plans = state
            .plans
            .lock()
            .map_err(|_| "Import state is unavailable.")?;
        prune(&mut plans);
        let plan = plans
            .remove(&id)
            .ok_or("This preview expired or is already importing. Preview the file again.")?;
        if plan.handle != handle {
            plans.insert(id, plan);
            return Err("This preview belongs to another project.".into());
        }
        plan
    };
    let (result, returned) = run(&projects, move |store| {
        let result = storage::commit(store, &mut plan, store_values);
        Ok((result, plan))
    })
    .await?;
    if !result.as_ref().is_ok_and(|result| result.error.is_none()) {
        state
            .plans
            .lock()
            .map_err(|_| "Import state is unavailable.")?
            .insert(id, returned);
    }
    result
}
#[tauri::command]
pub fn discard_openapi_import(state: State<'_, ImportState>, id: String) -> Result<(), String> {
    crate::projects::model::validate_id(&id)?;
    let mut plans = state
        .plans
        .lock()
        .map_err(|_| "Import state is unavailable.")?;
    prune(&mut plans);
    plans.remove(&id);
    Ok(())
}

#[tauri::command]
pub async fn rename_import_variable(
    state: State<'_, ImportState>,
    handle: String,
    id: String,
    old_name: String,
    name: String,
) -> Result<Preview, String> {
    let _operation = state.begin()?;
    crate::projects::model::validate_id(&id)?;
    let mut plan = {
        let mut plans = state
            .plans
            .lock()
            .map_err(|_| "Import state is unavailable.")?;
        prune(&mut plans);
        let plan = plans
            .get(&id)
            .ok_or("This preview expired. Preview the source again.")?;
        if plan.handle != handle {
            return Err("This preview belongs to another project.".into());
        }
        plans.remove(&id).ok_or("This preview is unavailable.")?
    };
    let (plan, result) = tauri::async_runtime::spawn_blocking(move || {
        let result = super::rename::variable(&mut plan, &old_name, &name);
        (plan, result)
    })
    .await
    .map_err(|_| "Could not rename the preview variable. Preview the source again.")?;
    state
        .plans
        .lock()
        .map_err(|_| "Import state is unavailable.")?
        .insert(id, plan);
    result
}
