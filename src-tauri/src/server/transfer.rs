//! Preview-bound local-to-server copies. Only explicit selections authorize OS value reads.
use super::{
    crypto,
    values::{SharedValue, ValueState},
    ServerIdentity,
};
use crate::projects::{
    commands::{run, ProjectState},
    environments::{Definitions, OsValues, Values},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{Manager, State};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Slot {
    pub variable_id: String,
    pub environment_id: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    id: String,
    name: String,
    collections: usize,
    requests: usize,
    definitions: Definitions,
    ready: bool,
    selected: usize,
    available: usize,
    missing: Vec<Slot>,
}
struct Prepared {
    shared: Vec<SharedValue>,
    secrets: Vec<crypto::CipherValue>,
}
impl Drop for Prepared {
    fn drop(&mut self) {
        for value in &mut self.shared {
            value.value.zeroize();
        }
    }
}
struct Plan {
    owner: ServerIdentity,
    handle: String,
    snapshot: String,
    project: String,
    created: Instant,
    preview: Preview,
    prepared: Option<Prepared>,
}
#[derive(Clone, Default)]
pub struct TransferState(Arc<Mutex<HashMap<String, Plan>>>);
impl TransferState {
    pub fn clear(&self) {
        if let Ok(mut plans) = self.0.lock() {
            plans.clear();
        }
    }
}
fn prune(plans: &mut HashMap<String, Plan>) {
    plans.retain(|_, plan| plan.created.elapsed() < Duration::from_secs(600));
}
fn check(plan: &Plan, owner: &ServerIdentity, handle: &str, snapshot: &str) -> Result<(), String> {
    if &plan.owner != owner || plan.handle != handle || plan.snapshot != snapshot {
        return Err(
            "The project or server account changed. Prepare a new transfer preview.".into(),
        );
    }
    Ok(())
}
#[tauri::command]
pub async fn server_preview_transfer(
    app: tauri::AppHandle,
    projects: State<'_, ProjectState>,
    handle: String,
) -> Result<Preview, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| super::SETTINGS_ERROR)?;
    let session = super::fresh_session(&root).await?;
    let transfers = app.state::<TransferState>().inner().clone();
    run(&projects, move |store| {
        let (project, name, snapshot) = store.snapshot(&handle)?;
        let definitions = store.with_environment(&handle, |root, _| {
            if crate::projects::store::server_owner(root)?.is_some() { return Err("Choose a local project for transfer. Server workspaces use explicit synchronization.".into()); }
            Ok(crate::projects::environments::definitions(root)?.0)
        })?;
        let view = store.reload(&handle)?;
        let preview = Preview { id: uuid::Uuid::new_v4().to_string(), name, collections: view.collections.len(), requests: view.collections.iter().map(|collection| collection.data.requests.len()).sum(), definitions, ready: false, selected: 0, available: 0, missing: vec![] };
        let mut plans = transfers.0.lock().map_err(|_| "Transfer state is unavailable.")?;
        prune(&mut plans);
        if plans.len() >= 2 { return Err("Close an existing transfer preview before starting another.".into()); }
        plans.insert(preview.id.clone(), Plan { owner: session.identity, handle, snapshot, project, created: Instant::now(), preview: preview.clone(), prepared: None });
        Ok(preview)
    }).await
}
#[tauri::command]
pub async fn server_prepare_transfer(
    app: tauri::AppHandle,
    projects: State<'_, ProjectState>,
    handle: String,
    id: String,
    selections: Vec<Slot>,
) -> Result<Preview, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| super::SETTINGS_ERROR)?;
    let session = super::fresh_session(&root).await?;
    let transfers = app.state::<TransferState>().inner().clone();
    let values = app.state::<ValueState>().inner().clone();
    run(&projects, move |store| {
        let snapshot = store.snapshot(&handle)?.2;
        let mut plans = transfers
            .0
            .lock()
            .map_err(|_| "Transfer state is unavailable.")?;
        prune(&mut plans);
        let plan = plans
            .get_mut(&id)
            .ok_or("The transfer preview expired. Create a new preview.")?;
        check(plan, &session.identity, &handle, &snapshot)?;
        plan.prepared = None;
        plan.preview.ready = false;
        let (prepared, missing) = prepare(
            &plan.owner,
            &plan.project,
            &plan.preview.definitions,
            &selections,
            &OsValues,
            &values,
        )?;
        plan.preview.selected = selections.len();
        plan.preview.available = prepared.shared.len() + prepared.secrets.len();
        plan.preview.missing = missing;
        plan.preview.ready = true;
        plan.prepared = Some(prepared);
        Ok(plan.preview.clone())
    })
    .await
}
fn prepare(
    owner: &ServerIdentity,
    project: &str,
    definitions: &Definitions,
    slots: &[Slot],
    source: &impl Values,
    state: &ValueState,
) -> Result<(Prepared, Vec<Slot>), String> {
    if slots.len() > 1000 {
        return Err("Select at most 1,000 value slots per transfer.".into());
    }
    let mut unique = HashSet::new();
    let mut needs_key = false;
    for slot in slots {
        let variable = definitions
            .variables
            .iter()
            .find(|variable| variable.id == slot.variable_id)
            .ok_or("Unknown transfer variable.")?;
        if !unique.insert((&slot.variable_id, &slot.environment_id))
            || slot.environment_id != "default"
                && !definitions
                    .environments
                    .iter()
                    .any(|environment| environment.id == slot.environment_id)
        {
            return Err("Invalid or duplicate transfer slot.".into());
        }
        needs_key |= variable.secret.unwrap_or(true);
    }
    let key = if needs_key {
        Some(state.key(owner)?)
    } else {
        None
    };
    let mut prepared = Prepared {
        shared: vec![],
        secrets: vec![],
    };
    let mut missing = vec![];
    for slot in slots {
        let variable = definitions
            .variables
            .iter()
            .find(|variable| variable.id == slot.variable_id)
            .ok_or("Unknown transfer variable.")?;
        let value = source
            .get(&format!(
                "{project}/{}/{}",
                slot.variable_id, slot.environment_id
            ))?
            .map(Zeroizing::new);
        let Some(value) = value else {
            missing.push(slot.clone());
            continue;
        };
        if value.len() > 2560 {
            return Err(
                "A selected value exceeds 2,560 UTF-8 bytes. No project was transferred.".into(),
            );
        }
        if variable.secret.unwrap_or(true) {
            prepared.secrets.push(crypto::encrypt(
                owner,
                project,
                &slot.variable_id,
                &slot.environment_id,
                &value,
                key.as_ref().ok_or("Unlock the account vault first.")?,
            )?);
        } else {
            prepared.shared.push(SharedValue {
                variable_id: slot.variable_id.clone(),
                environment_id: slot.environment_id.clone(),
                value: value.to_string(),
                revision: 0,
            });
        }
    }
    Ok((prepared, missing))
}
#[tauri::command]
pub async fn server_commit_transfer(
    app: tauri::AppHandle,
    projects: State<'_, ProjectState>,
    handle: String,
    id: String,
) -> Result<super::ServerProject, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| super::SETTINGS_ERROR)?;
    let session = super::fresh_session(&root).await?;
    let transfers = app.state::<TransferState>().inner().clone();
    let plan = run(&projects, move |store| {
        let snapshot = store.snapshot(&handle)?.2;
        let mut plans = transfers
            .0
            .lock()
            .map_err(|_| "Transfer state is unavailable.")?;
        prune(&mut plans);
        let plan = plans.get(&id).ok_or("The transfer preview expired.")?;
        check(plan, &session.identity, &handle, &snapshot)?;
        if plan.prepared.is_none() {
            return Err("Check the selected value transfer before uploading.".into());
        }
        plans
            .remove(&id)
            .ok_or_else(|| "Transfer preview is unavailable.".into())
    })
    .await?;
    let session = super::fresh_session(&root).await?;
    if session.identity != plan.owner {
        return Err("The server account changed. No upload was started.".into());
    }
    let prepared = plan
        .prepared
        .as_ref()
        .ok_or("Prepare the transfer first.")?;
    #[derive(Deserialize)]
    struct Snapshot {
        project: crate::projects::model::Project,
        collections: Vec<crate::projects::model::Collection>,
        environments: Definitions,
    }
    let snapshot: Snapshot = serde_json::from_str(&plan.snapshot)
        .map_err(|_| "The local project could not be read for upload.".to_string())?;
    let tree = super::sync::upload_tree(
        &snapshot.project,
        &snapshot.collections,
        &snapshot.environments,
    )?;
    let response = super::http_client()?
        .post(super::api_url(&plan.owner.server_url, "/api/projects")?)
        .bearer_auth(&session.access_token)
        .json(&serde_json::json!({
            "id": plan.project,
            "name": plan.preview.name,
            "kind": "personal",
            "tree": tree,
            "sharedValues": prepared.shared,
            "vaultValues": prepared.secrets
        }))
        .send()
        .await
        .map_err(|_| "The upload response was interrupted. Local files remain unchanged. Check the server project list before preparing a new transfer; the server may have saved this copy.".to_string())?;
    if !response.status().is_success() {
        return Err(super::response_error(response).await);
    }
    super::ensure_session(&session)?;
    super::values::read_json(response).await
}
#[tauri::command]
pub fn server_discard_transfer(app: tauri::AppHandle, id: String) -> Result<(), String> {
    app.state::<TransferState>()
        .0
        .lock()
        .map_err(|_| "Transfer state is unavailable.")?
        .remove(&id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::environments::Definition;
    use std::cell::RefCell;
    struct Source {
        reads: RefCell<Vec<String>>,
        values: HashMap<String, String>,
    }
    impl Values for Source {
        fn get(&self, key: &str) -> Result<Option<String>, String> {
            self.reads.borrow_mut().push(key.into());
            Ok(self.values.get(key).cloned())
        }
        fn set(&self, _: &str, _: &str) -> Result<(), String> {
            panic!("Transfer must not write local values")
        }
        fn delete(&self, _: &str) -> Result<(), String> {
            panic!("Transfer must not remove local values")
        }
    }
    #[test]
    fn definitions_only_transfer_reads_nothing_and_only_explicit_slots_can_become_shared() {
        let owner = ServerIdentity {
            server_url: "https://synthetic.example.test".into(),
            account_id: uuid::Uuid::new_v4(),
        };
        let project = uuid::Uuid::new_v4().to_string();
        let id = uuid::Uuid::new_v4().to_string();
        let definitions = Definitions {
            schema_version: 1,
            environments: vec![],
            variables: vec![Definition {
                id: id.clone(),
                name: "baseUrl".into(),
                secret: Some(false),
            }],
        };
        let reference = format!("{project}/{id}/default");
        let source = Source {
            reads: RefCell::new(vec![]),
            values: HashMap::from([(reference.clone(), "Synthetic shared".into())]),
        };
        let state = ValueState::default();
        let (prepared, _) = prepare(&owner, &project, &definitions, &[], &source, &state).unwrap();
        assert!(prepared.shared.is_empty());
        assert!(source.reads.borrow().is_empty());
        let slot = Slot {
            variable_id: id,
            environment_id: "default".into(),
        };
        let (prepared, missing) = prepare(
            &owner,
            &project,
            &definitions,
            std::slice::from_ref(&slot),
            &source,
            &state,
        )
        .unwrap();
        assert_eq!(prepared.shared[0].value, "Synthetic shared");
        assert!(missing.is_empty());
        assert_eq!(*source.reads.borrow(), vec![reference]);
        assert!(prepare(
            &owner,
            &project,
            &definitions,
            &[slot.clone(), slot],
            &source,
            &state
        )
        .is_err());
    }
    #[test]
    fn legacy_secret_selections_fail_while_locked_before_reading_any_os_value() {
        let owner = ServerIdentity {
            server_url: "https://synthetic.example.test".into(),
            account_id: uuid::Uuid::new_v4(),
        };
        let project = uuid::Uuid::new_v4().to_string();
        let id = uuid::Uuid::new_v4().to_string();
        let definitions = Definitions {
            schema_version: 1,
            environments: vec![],
            variables: vec![Definition {
                id: id.clone(),
                name: "token".into(),
                secret: None,
            }],
        };
        let source = Source {
            reads: RefCell::new(vec![]),
            values: HashMap::new(),
        };
        assert!(prepare(
            &owner,
            &project,
            &definitions,
            &[Slot {
                variable_id: id,
                environment_id: "default".into()
            }],
            &source,
            &ValueState::default()
        )
        .is_err());
        assert!(source.reads.borrow().is_empty());
    }
}
