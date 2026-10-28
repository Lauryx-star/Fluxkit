//! Server workspaces on disk: a mirror of the server's entity rows in the ordinary project
//! file format, plus the last known server tree (`.fluxkit/server-tree.json`).
//!
//! Writes are server-confirmed: a command first changes the mirror as for a local project,
//! then `publish` sends the difference to the server as one batch. If the server rejects it,
//! the mirror is rewritten from the last known tree, so nothing unconfirmed stays on disk.
//! `refresh` folds other users' changes from the change feed into tree and mirror. Both run
//! under `SyncState` so a refresh can never interleave with a write.
use super::sync::{self, Change, Operation, RemoteTree};
use crate::projects::{
    commands::{run, ProjectState},
    environments,
    model::Collection,
    store::{self, ProjectView},
};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};
use tauri::{Manager, State};

/// Serializes every server-workspace write and refresh in this process.
#[derive(Default)]
pub struct SyncState(tauri::async_runtime::Mutex<()>);

pub async fn guard(app: &tauri::AppHandle) -> tokio::sync::MutexGuard<'_, ()> {
    let state: &SyncState = app.state::<SyncState>().inner();
    state.0.lock().await
}

const TREE: &str = ".fluxkit/server-tree.json";

pub(crate) fn read_tree(root: &Path) -> Result<RemoteTree, String> {
    let path = root.join(TREE);
    store::safe(&path, false)?;
    store::decode(&store::read(&path)?).map_err(|_| {
        "The local server state is invalid. Reopen the project from Server connection.".into()
    })
}

/// Writes the mirror files and the tree. Collection files no longer on the server are removed.
pub(crate) fn write_mirror(root: &Path, tree: &RemoteTree) -> Result<(), String> {
    let rendered = sync::render(tree);
    rendered.project.validate()?;
    rendered.definitions.validate()?;
    for collection in &rendered.collections {
        collection.validate()?;
    }
    let collections = root.join("collections");
    store::safe(&collections, true)?;
    store::atomic(
        &root.join("project.fluxkit.json"),
        &store::encode(&rendered.project)?,
        false,
    )?;
    store::atomic(
        &root.join("environments.fluxkit.json"),
        &store::encode(&rendered.definitions)?,
        false,
    )?;
    let keep: Vec<String> = rendered
        .collections
        .iter()
        .map(|c| format!("{}.json", c.id))
        .collect();
    for collection in &rendered.collections {
        store::atomic(
            &collections.join(format!("{}.json", collection.id)),
            &store::encode(collection)?,
            false,
        )?;
    }
    for entry in
        fs::read_dir(&collections).map_err(|_| "Could not read the workspace collections.")?
    {
        let entry = entry.map_err(|_| "Could not read the workspace collections.")?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".json") && !keep.contains(&name) {
            store::safe(&entry.path(), false)?;
            fs::remove_file(entry.path())
                .map_err(|_| "Could not remove a deleted collection file.")?;
        }
    }
    store::atomic(
        &root.join(TREE),
        &serde_json::to_vec(tree).map_err(|_| "Could not save the server state.")?,
        false,
    )
}

fn local_collections(root: &Path) -> Result<Vec<Collection>, String> {
    let directory = root.join("collections");
    store::safe(&directory, true)?;
    let mut collections = Vec::new();
    for entry in
        fs::read_dir(&directory).map_err(|_| "Could not read the workspace collections.")?
    {
        let path = entry
            .map_err(|_| "Could not read the workspace collections.")?
            .path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let collection: Collection = store::decode(&store::read(&path)?)?;
            collection.validate()?;
            collections.push(collection);
        }
    }
    collections.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(collections)
}

/// The batch that makes the server match the mirror, or nothing when they already agree.
pub(crate) fn mirror_ops(root: &Path, tree: &RemoteTree) -> Result<Vec<Operation>, String> {
    let (project, _) = store::load_project(root)?;
    let collections = local_collections(root)?;
    // Keep the server's collection order for existing collections; new ones go last.
    let order: BTreeMap<&str, &str> = tree
        .collections
        .iter()
        .map(|c| (c.id.as_str(), c.data.position.as_str()))
        .collect();
    let mut collections = collections;
    collections.sort_by(|a, b| {
        let key = |c: &Collection| {
            order
                .get(c.id.as_str())
                .map(|p| p.to_string())
                .unwrap_or_else(|| "~".into())
        };
        key(a).cmp(&key(b)).then(a.id.cmp(&b.id))
    });
    let mut ops = sync::plan_project(tree, &project.name, project.auth.as_ref())?;
    ops.extend(sync::plan_collections(tree, &collections)?);
    for remote in &tree.collections {
        if !collections.iter().any(|c| c.id == remote.id) {
            ops.extend(sync::plan_collection_delete(tree, &remote.id)?);
        }
    }
    let (definitions, _) = environments::definitions(root)?;
    ops.extend(sync::plan_definitions(tree, &definitions)?);
    Ok(ops)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Accepted {
    revisions: BTreeMap<String, i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConflictEntry {
    kind: String,
    #[serde(default)]
    revision: Option<i64>,
    #[serde(default)]
    updated_by: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rejection {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    conflicts: Vec<ConflictEntry>,
}

/// A readable conflict message: what changed and who changed it.
fn conflict_message(rejection: &Rejection) -> String {
    let mut parts: Vec<String> = rejection
        .conflicts
        .iter()
        .map(|conflict| {
            let what = match conflict.kind.as_str() {
                "project" => "the project settings",
                "collection" => "a collection",
                "folder" => "a folder",
                "request" => "a request",
                "environment" => "an environment",
                _ => "a variable",
            };
            match (&conflict.updated_by, conflict.revision) {
                (_, None) => format!("{what} was deleted on the server"),
                (Some(user), _) => format!("{what} was changed by {user}"),
                (None, _) => format!("{what} was changed on the server"),
            }
        })
        .collect();
    parts.dedup();
    if parts.is_empty() {
        return rejection
            .error
            .clone()
            .unwrap_or_else(|| "The server rejected the change. Reload the project.".into());
    }
    format!(
        "Not saved: {} meanwhile. Your change was not applied; reload the project to see the current version.",
        parts.join(", ")
    )
}

async fn post(
    session: &super::Session,
    project: &str,
    ops: &[Operation],
) -> Result<BTreeMap<String, i64>, String> {
    let response = super::http_client()?
        .post(super::api_url(
            &session.identity.server_url,
            &format!("/api/projects/{project}/changes"),
        )?)
        .bearer_auth(&session.access_token)
        .json(&serde_json::json!({ "operations": ops }))
        .send()
        .await
        .map_err(|_| {
            "The server could not be reached. Nothing was saved; the change was undone locally."
                .to_string()
        })?;
    let status = response.status();
    if status.is_success() {
        let accepted: Accepted = super::values::read_json(response).await?;
        return Ok(accepted.revisions);
    }
    if status == reqwest::StatusCode::CONFLICT || status == reqwest::StatusCode::BAD_REQUEST {
        let rejection: Rejection = super::values::read_json(response)
            .await
            .unwrap_or(Rejection {
                error: None,
                conflicts: vec![],
            });
        return Err(conflict_message(&rejection));
    }
    Err(super::response_error(response).await)
}

/// Sends the mirror's difference to the server. The caller holds `guard`. On success the tree
/// records the new revisions; on any failure the mirror is rewritten from the tree.
pub async fn publish(
    app: &tauri::AppHandle,
    projects: &State<'_, ProjectState>,
    handle: &str,
) -> Result<(), String> {
    let owner = handle.to_string();
    let planned = run(projects, move |store| {
        store.with_environment(&owner, |root, project| {
            if store::server_owner(root)?.is_none() || !root.join(TREE).exists() {
                return Ok(None);
            }
            let tree = read_tree(root)?;
            let ops = match mirror_ops(root, &tree) {
                Ok(ops) => ops,
                Err(error) => {
                    write_mirror(root, &tree)?;
                    return Err(error);
                }
            };
            Ok(Some((tree, ops, project.to_string())))
        })
    })
    .await?;
    let Some((mut tree, ops, project)) = planned else {
        return Ok(());
    };
    if ops.is_empty() {
        return Ok(());
    }
    let root_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| super::SETTINGS_ERROR)?;
    let result = match super::session_for(&root_dir) {
        Ok(session) => post(&session, &project, &ops).await,
        Err(error) => Err(error),
    };
    let owner = handle.to_string();
    match result {
        Ok(revisions) => {
            sync::apply(&mut tree, &sync::accepted(&ops, &revisions)?)?;
            run(projects, move |store| {
                store.with_environment(&owner, |root, _| {
                    store::atomic(
                        &root.join(TREE),
                        &serde_json::to_vec(&tree)
                            .map_err(|_| "Could not save the server state.")?,
                        false,
                    )
                })
            })
            .await
        }
        Err(error) => {
            run(projects, move |store| {
                store.with_environment(&owner, |root, _| write_mirror(root, &tree))
            })
            .await?;
            Err(error)
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Feed {
    seq: i64,
    changes: Vec<Change>,
}

pub(super) async fn fetch_tree(
    session: &super::Session,
    project: &str,
) -> Result<Option<RemoteTree>, String> {
    let response = super::http_client()?
        .get(super::api_url(
            &session.identity.server_url,
            &format!("/api/projects/{project}/tree"),
        )?)
        .bearer_auth(&session.access_token)
        .send()
        .await;
    let response = match response {
        Ok(response)
            if response.status().is_server_error()
                || response.status() == reqwest::StatusCode::REQUEST_TIMEOUT =>
        {
            return Ok(None)
        }
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => return Ok(None),
        Err(_) => return Err("Could not reach the Fluxkit server.".into()),
    };
    if !response.status().is_success() {
        return Err(super::response_error(response).await);
    }
    let tree: RemoteTree = super::values::read_json(response).await?;
    if tree.project.id != project {
        return Err("The server returned a different project than requested.".into());
    }
    Ok(Some(tree))
}

/// Folds other users' changes into tree and mirror. Returns false when offline.
pub async fn refresh(
    app: &tauri::AppHandle,
    projects: &State<'_, ProjectState>,
    handle: &str,
) -> Result<bool, String> {
    let owner = handle.to_string();
    let current = run(projects, move |store| {
        store.with_environment(&owner, |root, project| {
            if store::server_owner(root)?.is_none() || !root.join(TREE).exists() {
                return Ok(None);
            }
            Ok(Some((read_tree(root)?, project.to_string())))
        })
    })
    .await?;
    let Some((mut tree, project)) = current else {
        return Ok(false);
    };
    let root_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| super::SETTINGS_ERROR)?;
    let session = super::session_for(&root_dir)?;
    let response = super::http_client()?
        .get(super::api_url(
            &session.identity.server_url,
            &format!("/api/projects/{project}/changes?since={}", tree.seq),
        )?)
        .bearer_auth(&session.access_token)
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => return Ok(false),
        Err(_) => return Err("Could not reach the Fluxkit server.".into()),
    };
    if response.status() == reqwest::StatusCode::GONE {
        match fetch_tree(&session, &project).await? {
            Some(fresh) => tree = fresh,
            None => return Ok(false),
        }
    } else if response.status().is_server_error() {
        return Ok(false);
    } else if !response.status().is_success() {
        return Err(super::response_error(response).await);
    } else {
        let feed: Feed = super::values::read_json(response).await?;
        if feed.changes.is_empty() && feed.seq == tree.seq {
            return Ok(true);
        }
        sync::apply(&mut tree, &feed.changes)?;
        tree.seq = feed.seq.max(tree.seq);
    }
    super::ensure_session(&session)?;
    let owner = handle.to_string();
    run(projects, move |store| {
        store.with_environment(&owner, |root, _| write_mirror(root, &tree))
    })
    .await?;
    Ok(true)
}

/// Pulls other users' changes into an open server workspace and returns its reloaded view.
#[tauri::command]
pub async fn server_refresh_workspace(
    app: tauri::AppHandle,
    projects: State<'_, ProjectState>,
    handle: String,
) -> Result<ProjectView, String> {
    {
        let _serialized = guard(&app).await;
        refresh(&app, &projects, &handle).await?;
    }
    run(&projects, move |store| store.reload(&handle)).await
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
