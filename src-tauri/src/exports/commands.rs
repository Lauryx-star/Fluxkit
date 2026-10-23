use super::{mapping, storage, Plan, Preview, Target};
use crate::projects::{
    commands::{run, ProjectState},
    environments, store,
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};
use tauri::State;
#[derive(Default)]
pub struct ExportState {
    plans: Mutex<HashMap<String, Plan>>,
    busy: AtomicBool,
}
struct Operation<'a>(&'a AtomicBool);
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
impl ExportState {
    fn begin(&self) -> Result<Operation<'_>, String> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err("Another export operation is in progress.".into());
        }
        Ok(Operation(&self.busy))
    }
}
fn prune(plans: &mut HashMap<String, Plan>) {
    plans.retain(|_, p| p.created.elapsed() < Duration::from_secs(600));
}
#[tauri::command]
pub async fn preview_openapi_export(
    projects: State<'_, ProjectState>,
    state: State<'_, ExportState>,
    handle: String,
    target: Target,
) -> Result<Preview, String> {
    let _operation = state.begin()?;
    {
        let mut plans = state
            .plans
            .lock()
            .map_err(|_| "Export state is unavailable.")?;
        prune(&mut plans);
        if plans.len() >= 2 {
            return Err("Close an export preview before preparing another.".into());
        }
    }
    let plan = run(&projects, move |store| {
        store.with_environment(&handle, |root, _| {
            let (definitions, revision) = environments::definitions(root)?;
            mapping::plan(store::load(root, &handle)?, definitions, revision, target)
        })
    })
    .await?;
    let preview = plan.preview.clone();
    state
        .plans
        .lock()
        .map_err(|_| "Export state is unavailable.")?
        .insert(preview.id.clone(), plan);
    Ok(preview)
}
#[tauri::command]
pub async fn save_openapi_export(
    projects: State<'_, ProjectState>,
    state: State<'_, ExportState>,
    handle: String,
    id: String,
) -> Result<bool, String> {
    let _operation = state.begin()?;
    crate::projects::model::validate_id(&id)?;
    let plan = {
        let mut plans = state
            .plans
            .lock()
            .map_err(|_| "Export state is unavailable.")?;
        prune(&mut plans);
        let plan = plans
            .remove(&id)
            .ok_or("Export preview expired. Prepare it again.")?;
        if plan.snapshot.handle != handle {
            plans.insert(id, plan);
            return Err("Export belongs to another project.".into());
        }
        plan
    };
    if !plan.preview.blockers.is_empty() {
        state
            .plans
            .lock()
            .map_err(|_| "Export state is unavailable.")?
            .insert(id, plan);
        return Err("Resolve export blockers before saving.".into());
    }
    let destination = if plan.preview.directory {
        rfd::AsyncFileDialog::new()
            .set_title("Choose a parent folder for the new OpenAPI export folder")
            .pick_folder()
            .await
    } else {
        rfd::AsyncFileDialog::new()
            .set_title("Save OpenAPI YAML as a new file")
            .add_filter("OpenAPI YAML", &["yml", "yaml"])
            .set_file_name("fluxkit-openapi.yml")
            .save_file()
            .await
    };
    let Some(destination) = destination else {
        state
            .plans
            .lock()
            .map_err(|_| "Export state is unavailable.")?
            .insert(id, plan);
        return Ok(false);
    };
    let path = destination.path().to_path_buf();
    let (result, plan) = run(&projects, move |store| {
        let result = storage::current(store, &plan).and_then(|_| storage::write(&plan, &path));
        Ok((result, plan))
    })
    .await?;
    if result.is_err() {
        state
            .plans
            .lock()
            .map_err(|_| "Export state is unavailable.")?
            .insert(id, plan);
    }
    result.map(|_| true)
}
#[tauri::command]
pub fn discard_openapi_export(state: State<'_, ExportState>, id: String) -> Result<(), String> {
    crate::projects::model::validate_id(&id)?;
    let mut plans = state
        .plans
        .lock()
        .map_err(|_| "Export state is unavailable.")?;
    prune(&mut plans);
    plans.remove(&id);
    Ok(())
}
