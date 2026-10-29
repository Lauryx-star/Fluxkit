//! Entity-level synchronization for server workspaces.
//!
//! The server stores every collection, folder, request, environment and variable as its own row
//! with its own revision. The client keeps the last known server state (`RemoteTree`) next to
//! the workspace. From it this module
//! - renders the local mirror files the `ProjectStore` reads (`render`),
//! - turns a local save into the minimal set of entity operations (`plan_*`), each carrying the
//!   revision it expects, so unrelated edits of other users never conflict,
//! - folds accepted operations and the server's change feed back into the tree.
//!
//! Everything here is pure data transformation: no I/O, no network, fully unit-tested.
use crate::projects::{
    environments::{Definition, Definitions},
    model::{
        Auth, Body, Collection, Entry, Folder, OpenApiMetadata, PostmanMetadata, Project, Request,
        SCHEMA_VERSION,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProject {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub auth: Option<Auth>,
    pub revision: i64,
    /// Whether the signed-in account may write; reported by the server's tree.
    #[serde(default)]
    pub can_edit: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CollectionData {
    pub name: String,
    #[serde(default)]
    pub auth: Option<Auth>,
    #[serde(default)]
    pub open_api: Option<OpenApiMetadata>,
    #[serde(default)]
    pub postman: Option<PostmanMetadata>,
    pub position: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FolderData {
    pub collection_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub auth: Option<Auth>,
    pub position: String,
}

/// Everything about a request that has no column of its own on the server.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RequestDetails {
    #[serde(default)]
    pub import_issues: Vec<String>,
    #[serde(default)]
    pub auth: Option<Auth>,
    pub params: Vec<Entry>,
    pub headers: Vec<Entry>,
    pub body: Body,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RequestData {
    pub collection_id: String,
    pub folder_id: Option<String>,
    pub name: String,
    pub method: String,
    pub url: String,
    pub details: RequestDetails,
    pub position: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentData {
    pub name: String,
    pub position: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VariableData {
    pub name: String,
    pub secret: bool,
    pub position: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectData {
    pub name: String,
    #[serde(default)]
    pub auth: Option<Auth>,
}

/// One stored entity: its identity, the server revision and its data.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Remote<T> {
    pub id: String,
    pub revision: i64,
    #[serde(flatten)]
    pub data: T,
}

/// The last known server state of one project (`GET /api/projects/{id}/tree`).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTree {
    pub project: RemoteProject,
    pub collections: Vec<Remote<CollectionData>>,
    pub folders: Vec<Remote<FolderData>>,
    pub requests: Vec<Remote<RequestData>>,
    pub environments: Vec<Remote<EnvironmentData>>,
    pub variables: Vec<Remote<VariableData>>,
    /// Change-feed position this tree reflects.
    pub seq: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Project,
    Collection,
    Folder,
    Request,
    Environment,
    Variable,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Op {
    Put,
    Delete,
}

/// One entry of `POST /api/projects/{id}/changes`. `expected_revision` is `None` for a new entity.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub op: Op,
    pub kind: Kind,
    pub id: String,
    pub expected_revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// One accepted operation as the server reports it, or one change-feed entry.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub kind: Kind,
    pub id: String,
    pub operation: Op,
    pub revision: i64,
    #[serde(default)]
    pub data: Option<Value>,
}

/// The local files a server workspace mirror consists of.
pub struct Rendered {
    pub project: Project,
    pub collections: Vec<Collection>,
    pub definitions: Definitions,
}

// ---------------------------------------------------------------- positions

const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn digit(c: u8) -> usize {
    DIGITS.iter().position(|d| *d == c).unwrap_or(0)
}

/// A rank string strictly between `low` and `high` (both exclusive, `None` = open end).
/// Ranks compare as plain strings; the alphabet is `0-9a-z`.
pub fn rank_between(low: Option<&str>, high: Option<&str>) -> String {
    let low = low.unwrap_or("").as_bytes();
    let high = high.map(str::as_bytes);
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let a = low.get(i).map_or(0, |c| digit(*c));
        let b = match high {
            Some(h) => h.get(i).map_or(DIGITS.len(), |c| digit(*c)),
            None => DIGITS.len(),
        };
        if b > a + 1 {
            out.push(DIGITS[(a + b) / 2]);
            break;
        }
        // No room at this digit: keep the lower digit and continue below it. From here on the
        // upper bound no longer constrains us unless both strings still share the prefix.
        out.push(DIGITS[a]);
        if b == a + 1 {
            // The upper bound is now strictly greater whatever follows.
            let rest = &low[(i + 1).min(low.len())..];
            let tail = rank_between(
                if rest.is_empty() {
                    None
                } else {
                    std::str::from_utf8(rest).ok()
                },
                None,
            );
            out.extend_from_slice(tail.as_bytes());
            break;
        }
        i += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Positions for `items` in this order. Items that already have a position keep it when it
/// still sorts correctly (longest increasing run); only the others get new ranks.
fn positions(items: &[(String, Option<String>)]) -> Vec<String> {
    let n = items.len();
    // Longest strictly increasing subsequence over existing positions (O(n²), lists are small).
    let mut best = vec![0usize; n];
    let mut prev = vec![usize::MAX; n];
    for i in 0..n {
        let Some(pi) = &items[i].1 else { continue };
        best[i] = 1;
        for j in 0..i {
            if let Some(pj) = &items[j].1 {
                if pj < pi && best[j] + 1 > best[i] {
                    best[i] = best[j] + 1;
                    prev[i] = j;
                }
            }
        }
    }
    let mut keep = vec![false; n];
    if let Some(mut i) = (0..n).filter(|i| best[*i] > 0).max_by_key(|i| best[*i]) {
        loop {
            keep[i] = true;
            if prev[i] == usize::MAX {
                break;
            }
            i = prev[i];
        }
    }
    let mut out: Vec<Option<String>> = items
        .iter()
        .zip(&keep)
        .map(|((_, p), k)| if *k { p.clone() } else { None })
        .collect();
    let mut i = 0;
    while i < n {
        if out[i].is_some() {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && out[i].is_none() {
            i += 1;
        }
        let low = if start == 0 {
            None
        } else {
            out[start - 1].clone()
        };
        let high = out.get(i).cloned().flatten();
        let mut previous = low;
        for slot in out.iter_mut().take(i).skip(start) {
            let rank = rank_between(previous.as_deref(), high.as_deref());
            previous = Some(rank.clone());
            *slot = Some(rank);
        }
    }
    out.into_iter().map(Option::unwrap_or_default).collect()
}

// ---------------------------------------------------------------- rendering

fn sorted<T: Clone>(items: &[Remote<T>], position: impl Fn(&T) -> &str) -> Vec<Remote<T>> {
    let mut items = items.to_vec();
    items.sort_by(|a, b| {
        position(&a.data)
            .cmp(position(&b.data))
            .then(a.id.cmp(&b.id))
    });
    items
}

fn local_request(remote: &Remote<RequestData>) -> Request {
    let data = &remote.data;
    Request {
        import_issues: data.details.import_issues.clone(),
        id: remote.id.clone(),
        name: data.name.clone(),
        auth: data.details.auth.clone(),
        folder_id: data.folder_id.clone(),
        method: data.method.clone(),
        url: data.url.clone(),
        params: data.details.params.clone(),
        headers: data.details.headers.clone(),
        body: data.details.body.clone(),
    }
}

/// The mirror files for this tree, ordered by position.
pub fn render(tree: &RemoteTree) -> Rendered {
    let folders = sorted(&tree.folders, |f| &f.position);
    let requests = sorted(&tree.requests, |r| &r.position);
    let collections = sorted(&tree.collections, |c| &c.position)
        .iter()
        .map(|c| Collection {
            schema_version: SCHEMA_VERSION,
            id: c.id.clone(),
            name: c.data.name.clone(),
            auth: c.data.auth.clone(),
            open_api: c.data.open_api.clone(),
            postman: c.data.postman.clone(),
            folders: folders
                .iter()
                .filter(|f| f.data.collection_id == c.id)
                .map(|f| Folder {
                    id: f.id.clone(),
                    name: f.data.name.clone(),
                    auth: f.data.auth.clone(),
                    parent_id: f.data.parent_id.clone(),
                })
                .collect(),
            requests: requests
                .iter()
                .filter(|r| r.data.collection_id == c.id)
                .map(local_request)
                .collect(),
        })
        .collect();
    let definitions = Definitions {
        schema_version: 1,
        environments: sorted(&tree.environments, |e| &e.position)
            .iter()
            .map(|e| Definition {
                id: e.id.clone(),
                name: e.data.name.clone(),
                secret: None,
            })
            .collect(),
        variables: sorted(&tree.variables, |v| &v.position)
            .iter()
            .map(|v| Definition {
                id: v.id.clone(),
                name: v.data.name.clone(),
                secret: Some(v.data.secret),
            })
            .collect(),
    };
    Rendered {
        project: Project {
            schema_version: SCHEMA_VERSION,
            id: tree.project.id.clone(),
            name: tree.project.name.clone(),
            auth: tree.project.auth.clone(),
        },
        collections,
        definitions,
    }
}

// ---------------------------------------------------------------- planning

fn put<T: Serialize>(
    kind: Kind,
    id: &str,
    expected: Option<i64>,
    data: &T,
) -> Result<Operation, String> {
    Ok(Operation {
        op: Op::Put,
        kind,
        id: id.to_string(),
        expected_revision: expected,
        data: Some(serde_json::to_value(data).map_err(|_| "Could not encode a change.")?),
    })
}

fn delete(kind: Kind, id: &str, revision: i64) -> Operation {
    Operation {
        op: Op::Delete,
        kind,
        id: id.to_string(),
        expected_revision: Some(revision),
        data: None,
    }
}

fn index<T>(items: &[Remote<T>]) -> HashMap<&str, &Remote<T>> {
    items.iter().map(|item| (item.id.as_str(), item)).collect()
}

/// Operations that make the server match `updated` collections (whole collections as saved
/// locally). Other collections are untouched. Ordering inside the batch: parents before
/// children for puts, deletes last and only for the top-most removed entity (the server
/// cascades to everything below it).
pub fn plan_collections(
    tree: &RemoteTree,
    updated: &[Collection],
) -> Result<Vec<Operation>, String> {
    let remote_collections = index(&tree.collections);
    let remote_folders = index(&tree.folders);
    let remote_requests = index(&tree.requests);
    let mut ops = Vec::new();

    // Collection rows, positioned among all collections in their current remote order.
    let mut order: Vec<(String, Option<String>)> = sorted(&tree.collections, |c| &c.position)
        .iter()
        .map(|c| (c.id.clone(), Some(c.data.position.clone())))
        .collect();
    for collection in updated {
        if !order.iter().any(|(id, _)| *id == collection.id) {
            order.push((collection.id.clone(), None));
        }
    }
    let collection_positions: HashMap<String, String> = order
        .iter()
        .map(|(id, _)| id.clone())
        .zip(positions(&order))
        .collect();
    for collection in updated {
        let data = CollectionData {
            name: collection.name.clone(),
            auth: collection.auth.clone(),
            open_api: collection.open_api.clone(),
            postman: collection.postman.clone(),
            position: collection_positions[&collection.id].clone(),
        };
        match remote_collections.get(collection.id.as_str()) {
            Some(remote) if remote.data == data => {}
            Some(remote) => ops.push(put(
                Kind::Collection,
                &collection.id,
                Some(remote.revision),
                &data,
            )?),
            None => ops.push(put(Kind::Collection, &collection.id, None, &data)?),
        }
    }

    // Folders: parents first so a new child never references a folder created later.
    let mut new_folder_ids = HashSet::new();
    let mut new_request_ids = HashSet::new();
    for collection in updated {
        let existing: Vec<(String, Option<String>)> = collection
            .folders
            .iter()
            .map(|f| {
                let position = remote_folders
                    .get(f.id.as_str())
                    .filter(|r| r.data.collection_id == collection.id)
                    .map(|r| r.data.position.clone());
                (f.id.clone(), position)
            })
            .collect();
        let folder_positions = positions(&existing);
        let mut pending: Vec<(&Folder, String)> =
            collection.folders.iter().zip(folder_positions).collect();
        let mut placed: HashSet<&str> = HashSet::new();
        while !pending.is_empty() {
            let before = pending.len();
            let mut rest = Vec::new();
            for (folder, position) in pending {
                let parent_ready = folder.parent_id.as_deref().is_none_or(|parent| {
                    placed.contains(parent)
                        || (remote_folders.contains_key(parent)
                            && !collection.folders.iter().any(|f| f.id == parent))
                });
                if !parent_ready {
                    rest.push((folder, position));
                    continue;
                }
                placed.insert(folder.id.as_str());
                new_folder_ids.insert(folder.id.clone());
                let data = FolderData {
                    collection_id: collection.id.clone(),
                    parent_id: folder.parent_id.clone(),
                    name: folder.name.clone(),
                    auth: folder.auth.clone(),
                    position,
                };
                match remote_folders.get(folder.id.as_str()) {
                    Some(remote) if remote.data == data => {}
                    Some(remote) => {
                        ops.push(put(Kind::Folder, &folder.id, Some(remote.revision), &data)?)
                    }
                    None => ops.push(put(Kind::Folder, &folder.id, None, &data)?),
                }
            }
            if rest.len() == before {
                return Err("Folder parents form a cycle or reference a missing folder.".into());
            }
            pending = rest;
        }

        let existing: Vec<(String, Option<String>)> = collection
            .requests
            .iter()
            .map(|r| {
                let position = remote_requests
                    .get(r.id.as_str())
                    .filter(|remote| remote.data.collection_id == collection.id)
                    .map(|remote| remote.data.position.clone());
                (r.id.clone(), position)
            })
            .collect();
        for (request, position) in collection.requests.iter().zip(positions(&existing)) {
            new_request_ids.insert(request.id.clone());
            let data = RequestData {
                collection_id: collection.id.clone(),
                folder_id: request.folder_id.clone(),
                name: request.name.clone(),
                method: request.method.clone(),
                url: request.url.clone(),
                details: RequestDetails {
                    import_issues: request.import_issues.clone(),
                    auth: request.auth.clone(),
                    params: request.params.clone(),
                    headers: request.headers.clone(),
                    body: request.body.clone(),
                },
                position,
            };
            match remote_requests.get(request.id.as_str()) {
                Some(remote) if remote.data == data => {}
                Some(remote) => ops.push(put(
                    Kind::Request,
                    &request.id,
                    Some(remote.revision),
                    &data,
                )?),
                None => ops.push(put(Kind::Request, &request.id, None, &data)?),
            }
        }
    }

    // Deletes: entities that lived in an updated collection and are gone from every update.
    let updated_ids: HashSet<&str> = updated.iter().map(|c| c.id.as_str()).collect();
    let removed_folders: HashSet<&str> = tree
        .folders
        .iter()
        .filter(|f| {
            updated_ids.contains(f.data.collection_id.as_str()) && !new_folder_ids.contains(&f.id)
        })
        .map(|f| f.id.as_str())
        .collect();
    for folder in &tree.folders {
        if removed_folders.contains(folder.id.as_str())
            && !folder
                .data
                .parent_id
                .as_deref()
                .is_some_and(|p| removed_folders.contains(p))
        {
            ops.push(delete(Kind::Folder, &folder.id, folder.revision));
        }
    }
    for request in &tree.requests {
        if updated_ids.contains(request.data.collection_id.as_str())
            && !new_request_ids.contains(&request.id)
            && !request
                .data
                .folder_id
                .as_deref()
                .is_some_and(|f| removed_folders.contains(f))
        {
            ops.push(delete(Kind::Request, &request.id, request.revision));
        }
    }
    Ok(ops)
}

/// Deleting a collection removes its folders and requests on the server by cascade.
pub fn plan_collection_delete(tree: &RemoteTree, id: &str) -> Result<Vec<Operation>, String> {
    let collection = tree
        .collections
        .iter()
        .find(|c| c.id == id)
        .ok_or("This collection is no longer on the server. Reload the project.")?;
    Ok(vec![delete(Kind::Collection, id, collection.revision)])
}

/// Environment and variable definitions as saved locally.
pub fn plan_definitions(
    tree: &RemoteTree,
    updated: &Definitions,
) -> Result<Vec<Operation>, String> {
    let mut ops = Vec::new();
    let environments = index(&tree.environments);
    let order: Vec<(String, Option<String>)> = updated
        .environments
        .iter()
        .map(|e| {
            (
                e.id.clone(),
                environments
                    .get(e.id.as_str())
                    .map(|r| r.data.position.clone()),
            )
        })
        .collect();
    for (environment, position) in updated.environments.iter().zip(positions(&order)) {
        let data = EnvironmentData {
            name: environment.name.clone(),
            position,
        };
        match environments.get(environment.id.as_str()) {
            Some(remote) if remote.data == data => {}
            Some(remote) => ops.push(put(
                Kind::Environment,
                &environment.id,
                Some(remote.revision),
                &data,
            )?),
            None => ops.push(put(Kind::Environment, &environment.id, None, &data)?),
        }
    }
    let variables = index(&tree.variables);
    let order: Vec<(String, Option<String>)> = updated
        .variables
        .iter()
        .map(|v| {
            (
                v.id.clone(),
                variables
                    .get(v.id.as_str())
                    .map(|r| r.data.position.clone()),
            )
        })
        .collect();
    for (variable, position) in updated.variables.iter().zip(positions(&order)) {
        let data = VariableData {
            name: variable.name.clone(),
            secret: variable.secret.unwrap_or(true),
            position,
        };
        match variables.get(variable.id.as_str()) {
            Some(remote) if remote.data == data => {}
            Some(remote) => ops.push(put(
                Kind::Variable,
                &variable.id,
                Some(remote.revision),
                &data,
            )?),
            None => ops.push(put(Kind::Variable, &variable.id, None, &data)?),
        }
    }
    // Deletes after puts: a renamed variable may take a deleted one's name in the same batch.
    // The server applies the batch in order inside one transaction and checks uniqueness at
    // commit, so put-then-delete of a swapped name still succeeds.
    for environment in &tree.environments {
        if !updated.environments.iter().any(|e| e.id == environment.id) {
            ops.push(delete(
                Kind::Environment,
                &environment.id,
                environment.revision,
            ));
        }
    }
    for variable in &tree.variables {
        if !updated.variables.iter().any(|v| v.id == variable.id) {
            ops.push(delete(Kind::Variable, &variable.id, variable.revision));
        }
    }
    Ok(ops)
}

/// Project name and project-level authentication.
pub fn plan_project(
    tree: &RemoteTree,
    name: &str,
    auth: Option<&Auth>,
) -> Result<Vec<Operation>, String> {
    let data = ProjectData {
        name: name.to_string(),
        auth: auth.cloned(),
    };
    if tree.project.name == data.name && tree.project.auth == data.auth {
        return Ok(vec![]);
    }
    Ok(vec![put(
        Kind::Project,
        &tree.project.id,
        Some(tree.project.revision),
        &data,
    )?])
}

/// The `tree` body of `POST /api/projects` for uploading a local project: every entity as a new
/// row, folders parent-first, positions in the local order.
pub fn upload_tree(
    project: &Project,
    collections: &[Collection],
    definitions: &Definitions,
) -> Result<Value, String> {
    let empty = RemoteTree {
        project: RemoteProject {
            id: project.id.clone(),
            name: project.name.clone(),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut ops = plan_collections(&empty, collections)?;
    ops.extend(plan_definitions(&empty, definitions)?);
    let mut groups: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
    for op in &ops {
        let key = match op.kind {
            Kind::Collection => "collections",
            Kind::Folder => "folders",
            Kind::Request => "requests",
            Kind::Environment => "environments",
            Kind::Variable => "variables",
            Kind::Project => continue,
        };
        groups
            .entry(key)
            .or_default()
            .push(serde_json::json!({ "id": op.id, "data": op.data }));
    }
    let mut tree = serde_json::Map::new();
    tree.insert(
        "auth".into(),
        serde_json::to_value(&project.auth).map_err(|_| "Could not encode the project.")?,
    );
    for key in [
        "collections",
        "folders",
        "requests",
        "environments",
        "variables",
    ] {
        tree.insert(
            key.into(),
            Value::Array(groups.remove(key).unwrap_or_default()),
        );
    }
    Ok(Value::Object(tree))
}

// ---------------------------------------------------------------- applying

fn decode<T: for<'de> Deserialize<'de>>(change: &Change) -> Result<T, String> {
    serde_json::from_value(
        change
            .data
            .clone()
            .ok_or("A server change carries no data.")?,
    )
    .map_err(|_| "The server returned an invalid change.".into())
}

fn upsert<T>(items: &mut Vec<Remote<T>>, id: &str, revision: i64, data: T) {
    match items.iter_mut().find(|item| item.id == id) {
        Some(item) => {
            item.revision = revision;
            item.data = data;
        }
        None => items.push(Remote {
            id: id.to_string(),
            revision,
            data,
        }),
    }
}

/// Removes a folder and everything below it, as the server's cascade does.
fn remove_folder(tree: &mut RemoteTree, id: &str) {
    let mut gone: HashSet<String> = HashSet::from([id.to_string()]);
    loop {
        let more: Vec<String> = tree
            .folders
            .iter()
            .filter(|f| {
                f.data.parent_id.as_ref().is_some_and(|p| gone.contains(p)) && !gone.contains(&f.id)
            })
            .map(|f| f.id.clone())
            .collect();
        if more.is_empty() {
            break;
        }
        gone.extend(more);
    }
    tree.folders.retain(|f| !gone.contains(&f.id));
    tree.requests
        .retain(|r| !r.data.folder_id.as_ref().is_some_and(|f| gone.contains(f)));
}

/// Applies accepted operations or change-feed entries to the tree. Entries may arrive in any
/// order relative to local state; a put always carries the full current data.
pub fn apply(tree: &mut RemoteTree, changes: &[Change]) -> Result<(), String> {
    for change in changes {
        match (change.kind, change.operation) {
            (Kind::Project, Op::Put) => {
                let data: ProjectData = decode(change)?;
                tree.project.name = data.name;
                tree.project.auth = data.auth;
                tree.project.revision = change.revision;
            }
            (Kind::Project, Op::Delete) => {
                return Err("This project was deleted on the server.".into())
            }
            (Kind::Collection, Op::Put) => upsert(
                &mut tree.collections,
                &change.id,
                change.revision,
                decode(change)?,
            ),
            (Kind::Collection, Op::Delete) => {
                tree.collections.retain(|c| c.id != change.id);
                tree.folders.retain(|f| f.data.collection_id != change.id);
                tree.requests.retain(|r| r.data.collection_id != change.id);
            }
            (Kind::Folder, Op::Put) => upsert(
                &mut tree.folders,
                &change.id,
                change.revision,
                decode(change)?,
            ),
            (Kind::Folder, Op::Delete) => remove_folder(tree, &change.id),
            (Kind::Request, Op::Put) => upsert(
                &mut tree.requests,
                &change.id,
                change.revision,
                decode(change)?,
            ),
            (Kind::Request, Op::Delete) => tree.requests.retain(|r| r.id != change.id),
            (Kind::Environment, Op::Put) => upsert(
                &mut tree.environments,
                &change.id,
                change.revision,
                decode(change)?,
            ),
            (Kind::Environment, Op::Delete) => tree.environments.retain(|e| e.id != change.id),
            (Kind::Variable, Op::Put) => upsert(
                &mut tree.variables,
                &change.id,
                change.revision,
                decode(change)?,
            ),
            (Kind::Variable, Op::Delete) => tree.variables.retain(|v| v.id != change.id),
        }
    }
    Ok(())
}

/// The changes an accepted batch made, given the revisions the server assigned.
pub fn accepted(
    ops: &[Operation],
    revisions: &BTreeMap<String, i64>,
) -> Result<Vec<Change>, String> {
    ops.iter()
        .map(|op| {
            Ok(Change {
                kind: op.kind,
                id: op.id.clone(),
                operation: op.op,
                revision: match op.op {
                    Op::Put => *revisions
                        .get(&op.id)
                        .ok_or("The server did not confirm every change.")?,
                    Op::Delete => 0,
                },
                data: op.data.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
