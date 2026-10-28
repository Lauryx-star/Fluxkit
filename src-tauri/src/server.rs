//! Native boundary for an optional self-hosted Fluxkit server connection.
pub(crate) mod crypto;
pub(crate) mod oidc;
pub(crate) mod sync;
pub(crate) mod transfer;
pub(crate) mod values;
pub(crate) mod workspace;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;

const SERVICE: &str = "app.fluxkit.server-session.v2";
const ACCOUNT: &str = "access-token";
const SETTINGS_FILE: &str = "server.json";
const LIMIT: u64 = 16 * 1024;
const SETTINGS_ERROR: &str = "Could not access server connection settings.";
const TOKEN_ERROR: &str =
    "Could not access the server session in the operating system credential store.";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Settings {
    schema_version: u32,
    server_url: String,
}

/// Verified by /api/account at login; persisted only alongside the token in the OS store.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ServerIdentity {
    pub server_url: String,
    pub account_id: uuid::Uuid,
}
impl ServerIdentity {
    pub(crate) fn namespace(&self) -> String {
        format!(
            "{:x}",
            Sha256::digest(format!("{}/{}", self.server_url, self.account_id))
        )
    }
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Session {
    identity: ServerIdentity,
    access_token: String,
}
fn read_session() -> Result<Option<Session>, String> {
    match token_entry()?.get_secret() {
        Ok(value) => serde_json::from_slice(&value)
            .map(Some)
            .map_err(|_| TOKEN_ERROR.into()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(TOKEN_ERROR.into()),
    }
}
pub(crate) fn stored_identity() -> Result<Option<ServerIdentity>, String> {
    Ok(read_session()?.map(|session| session.identity))
}
fn session_for(root: &Path) -> Result<Session, String> {
    let session = read_session()?.ok_or("Sign in to the configured Fluxkit server first.")?;
    if settings(root)?.server_url != session.identity.server_url {
        return Err("Sign in to the configured Fluxkit server first.".into());
    }
    Ok(session)
}
fn account_root(root: &Path, identity: &ServerIdentity) -> Result<PathBuf, String> {
    ensure_directory(root)?;
    let accounts = root.join("server-accounts");
    ensure_directory(&accounts)?;
    let scoped = accounts.join(identity.namespace());
    ensure_directory(&scoped)?;
    Ok(scoped)
}
fn ensure_session(session: &Session) -> Result<(), String> {
    let current = read_session()?.ok_or("The server session changed. Reopen the project.")?;
    if current.identity != session.identity || current.access_token != session.access_token {
        return Err("The server session changed. Reopen the project.".into());
    }
    Ok(())
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    server_url: String,
    authenticated: bool,
    account_id: Option<uuid::Uuid>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProject {
    id: String,
    name: String,
    kind: String,
    revision: i64,
    #[serde(default)]
    can_edit: bool,
    #[serde(default)]
    can_manage_members: bool,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    collection_count: Option<i64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProjectMember {
    id: String,
    #[serde(alias = "userName")]
    username: String,
    role: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerAccount {
    id: String,
    #[serde(alias = "userName")]
    username: String,
    is_server_admin: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerAccountAdminView {
    id: String,
    #[serde(alias = "userName")]
    username: String,
    status: String,
    is_server_admin: bool,
    #[serde(default)]
    created_at: Option<String>,
    /// Collections in the account's personal project, which deleting the account removes.
    #[serde(default)]
    personal_collections: Option<i64>,
    #[serde(default)]
    team_projects: Option<i64>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CachedProjectList {
    server_url: String,
    projects: Vec<ServerProject>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ServerCacheMarker {
    pub server_url: String,
    #[serde(default)]
    pub account_id: Option<uuid::Uuid>,
    pub server_project_id: uuid::Uuid,
    pub revision: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginReply {
    access_token: String,
}

fn settings(path: &Path) -> Result<Settings, String> {
    let path = path.join(SETTINGS_FILE);
    let bytes = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings {
                schema_version: 1,
                server_url: String::new(),
            });
        }
        Err(_) => return Err(SETTINGS_ERROR.into()),
        Ok(metadata)
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > LIMIT =>
        {
            return Err(SETTINGS_ERROR.into());
        }
        Ok(_) => fs::read(&path).map_err(|_| SETTINGS_ERROR)?,
    };
    let settings: Settings = serde_json::from_slice(&bytes).map_err(|_| SETTINGS_ERROR)?;
    if settings.schema_version != 1
        || (!settings.server_url.is_empty() && normalize_url(&settings.server_url).is_err())
    {
        return Err(SETTINGS_ERROR.into());
    }
    Ok(settings)
}

fn normalize_url(value: &str) -> Result<String, String> {
    let parsed =
        reqwest::Url::parse(value.trim()).map_err(|_| "Enter a valid server URL.".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("Use an HTTP(S) server URL without credentials, query, or fragment.".into());
    }
    let local = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if parsed.scheme() != "https" && !local {
        return Err("Remote server connections require HTTPS.".into());
    }
    Ok(value.trim().trim_end_matches('/').to_string())
}

fn token_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|_| TOKEN_ERROR.into())
}

fn read_token(server_url: &str) -> Result<String, String> {
    read_session()?
        .filter(|session| session.identity.server_url == server_url)
        .map(|session| session.access_token)
        .ok_or_else(|| "Sign in to the configured Fluxkit server first.".into())
}

fn api_url(root: &str, path: &str) -> Result<reqwest::Url, String> {
    reqwest::Url::parse(&format!("{root}{path}")).map_err(|_| "Invalid server URL.".into())
}

fn ensure_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err("Server snapshot cache contains an unsafe path.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(path)
            .map_err(|_| "Could not create the server snapshot cache.".to_string()),
        Err(_) => Err("Could not access the server snapshot cache.".into()),
    }
}

pub(crate) fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| "Could not create a secure server connection.".into())
}

/// A read-only copy of a server project the account cannot edit, one directory per feed
/// position so an open copy is never rewritten underneath its viewer.
fn cache_snapshot(
    root: &Path,
    identity: &ServerIdentity,
    tree: &sync::RemoteTree,
) -> Result<PathBuf, String> {
    let project_id = uuid::Uuid::parse_str(&tree.project.id)
        .map_err(|_| "The server returned an invalid project identifier.".to_string())?;
    let revision = tree.seq + 1;
    let cache_root = root.join("server-snapshots");
    ensure_directory(&cache_root)?;
    let project_root = cache_root.join(project_id.to_string());
    ensure_directory(&project_root)?;
    let snapshot_root = project_root.join(revision.to_string());
    if snapshot_root.exists() {
        super::projects::store::safe(&snapshot_root, true)?;
        let marker = snapshot_root.join(".fluxkit/server-cache.json");
        super::projects::store::safe(&marker, false)?;
        let marker_data: ServerCacheMarker =
            serde_json::from_slice(&super::projects::store::read(&marker)?)
                .map_err(|_| "The local server snapshot marker is invalid.".to_string())?;
        if marker_data.server_url != identity.server_url
            || marker_data.account_id != Some(identity.account_id)
            || marker_data.server_project_id != project_id
            || marker_data.revision != revision
        {
            return Err("The local server snapshot belongs to a different server revision.".into());
        }
        return Ok(snapshot_root);
    }
    let staging = tempfile::Builder::new()
        .prefix(".snapshot-")
        .tempdir_in(&project_root)
        .map_err(|_| "Could not stage the server snapshot.".to_string())?;
    let stage_root = staging.path();
    ensure_directory(&stage_root.join("collections"))?;
    ensure_directory(&stage_root.join(".fluxkit"))?;
    workspace::write_mirror(stage_root, tree)?;
    let marker = serde_json::to_vec(&ServerCacheMarker {
        server_url: identity.server_url.clone(),
        account_id: Some(identity.account_id),
        server_project_id: project_id,
        revision,
    })
    .map_err(|_| "Could not write server snapshot metadata.".to_string())?;
    super::projects::store::atomic(
        &stage_root.join(".fluxkit/server-cache.json"),
        &marker,
        true,
    )?;
    fs::rename(stage_root, &snapshot_root)
        .map_err(|_| "Could not activate the server snapshot.".to_string())?;
    Ok(snapshot_root)
}

/// The editable mirror of a server project, created once per account and project and
/// rewritten from the server's tree each time the project is opened online.
fn server_workspace(
    root: &Path,
    identity: &ServerIdentity,
    tree: &sync::RemoteTree,
) -> Result<PathBuf, String> {
    let project_id = uuid::Uuid::parse_str(&tree.project.id)
        .map_err(|_| "The server returned an invalid project identifier.".to_string())?;
    let workspaces = root.join("server-workspaces");
    ensure_directory(root)?;
    ensure_directory(&workspaces)?;
    let pointer = workspaces.join(format!("{project_id}.json"));
    if pointer.try_exists().map_err(|_| SETTINGS_ERROR)? {
        let generation: uuid::Uuid =
            super::projects::store::decode(&super::projects::store::read(&pointer)?)?;
        let destination = workspaces.join(generation.to_string());
        super::projects::store::safe(&destination, true)?;
        let marker: super::projects::store::ServerWorkspaceMarker = super::projects::store::decode(
            &super::projects::store::read(&destination.join(".fluxkit/server-workspace.json"))?,
        )?;
        if marker.server_url != identity.server_url
            || marker.account_id != Some(identity.account_id)
            || marker.server_project_id != project_id
        {
            return Err(
                "A local server workspace belongs to a different account or server.".into(),
            );
        }
        let _lock = super::projects::store::lock(&destination)?;
        workspace::write_mirror(&destination, tree)?;
        return Ok(destination);
    }
    let generation = uuid::Uuid::new_v4();
    let destination = workspaces.join(generation.to_string());
    let staging = tempfile::Builder::new()
        .prefix(".fluxkit-workspace-")
        .tempdir_in(&workspaces)
        .map_err(|_| "Could not create a server workspace.".to_string())?;
    let stage_root = staging.path();
    ensure_directory(&stage_root.join("collections"))?;
    ensure_directory(&stage_root.join(".fluxkit"))?;
    workspace::write_mirror(stage_root, tree)?;
    let marker = serde_json::to_vec_pretty(&super::projects::store::ServerWorkspaceMarker {
        server_url: identity.server_url.clone(),
        account_id: Some(identity.account_id),
        server_project_id: project_id,
        revision: tree.project.revision.max(1),
    })
    .map_err(|_| "Could not write server workspace metadata.".to_string())?;
    super::projects::store::atomic(
        &stage_root.join(".fluxkit/server-workspace.json"),
        &marker,
        true,
    )?;
    fs::rename(staging.path(), &destination)
        .map_err(|_| "Could not activate the server workspace.".to_string())?;
    let bytes = serde_json::to_vec(&generation).map_err(|_| SETTINGS_ERROR)?;
    super::projects::store::atomic(&pointer, &bytes, true)?;
    Ok(destination)
}

fn offline_project(
    root: &Path,
    identity: &ServerIdentity,
    id: uuid::Uuid,
) -> Result<PathBuf, String> {
    let workspaces = root.join("server-workspaces");
    let pointer = workspaces.join(format!("{id}.json"));
    if pointer.try_exists().map_err(|_| SETTINGS_ERROR)? {
        super::projects::store::safe(&workspaces, true)?;
        let generation: uuid::Uuid =
            super::projects::store::decode(&super::projects::store::read(&pointer)?)?;
        let workspace = workspaces.join(generation.to_string());
        super::projects::store::safe(&workspace, true)?;
        let marker: super::projects::store::ServerWorkspaceMarker = super::projects::store::decode(
            &super::projects::store::read(&workspace.join(".fluxkit/server-workspace.json"))?,
        )?;
        if marker.server_url != identity.server_url
            || marker.account_id != Some(identity.account_id)
            || marker.server_project_id != id
            || marker.revision < 1
        {
            return Err(
                "The offline workspace belongs to another server account or project.".into(),
            );
        }
        return Ok(workspace);
    }
    latest_cached_snapshot(root, identity, id)?
        .ok_or_else(|| "No offline snapshot exists for this server project.".into())
}

fn latest_cached_snapshot(
    root: &Path,
    identity: &ServerIdentity,
    id: uuid::Uuid,
) -> Result<Option<PathBuf>, String> {
    let project_root = root.join("server-snapshots").join(id.to_string());
    if !project_root.exists() {
        return Ok(None);
    }
    super::projects::store::safe(&project_root, true)?;
    let mut revisions = fs::read_dir(&project_root)
        .map_err(|_| "Could not read the server snapshot cache.".to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let revision = entry.file_name().to_string_lossy().parse::<i64>().ok()?;
            (revision > 0).then_some((revision, entry.path()))
        })
        .collect::<Vec<_>>();
    revisions.sort_by_key(|(revision, _)| *revision);
    for (revision, path) in revisions.into_iter().rev() {
        let marker_path = path.join(".fluxkit/server-cache.json");
        if super::projects::store::safe(&path, true).is_err()
            || super::projects::store::safe(&marker_path, false).is_err()
        {
            continue;
        }
        let marker = super::projects::store::read(&marker_path)?;
        if let Ok(marker) = serde_json::from_slice::<ServerCacheMarker>(&marker) {
            if marker.server_url == identity.server_url
                && marker.account_id == Some(identity.account_id)
                && marker.server_project_id == id
                && marker.revision == revision
            {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

fn cached_project_list(root: &Path, server_url: &str) -> Result<Vec<ServerProject>, String> {
    let path = root.join("server-project-list.json");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
        Ok(_) => return Err(SETTINGS_ERROR.into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(SETTINGS_ERROR.into()),
    };
    if metadata.len() > 256 * 1024 {
        return Err(SETTINGS_ERROR.into());
    }
    let bytes = fs::read(path).map_err(|_| SETTINGS_ERROR)?;
    let cached: CachedProjectList = serde_json::from_slice(&bytes).map_err(|_| SETTINGS_ERROR)?;
    if cached.server_url != server_url {
        return Ok(Vec::new());
    }
    Ok(cached.projects)
}

async fn response_error(response: reqwest::Response) -> String {
    match response.status().as_u16() {
        401 | 403 => "The server rejected this account or session. Sign in again or ask the administrator to approve your account.".into(),
        404 => "The requested server resource was not found or is not available to this account.".into(),
        409 => "The server reported a conflict. Reload before retrying.".into(),
        429 => "Too many authentication attempts. Wait before trying again.".into(),
        500..=599 => "The Fluxkit server encountered an error. Try again later.".into(),
        _ => "The Fluxkit server rejected the request.".into(),
    }
}

async fn server_api_request(
    app: &tauri::AppHandle,
    method: reqwest::Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<reqwest::Response, String> {
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    if current.server_url.is_empty() {
        return Err("Configure a server URL first.".into());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| "Could not create a secure server connection.".to_string())?;
    let mut request = client
        .request(method, api_url(&current.server_url, path)?)
        .bearer_auth(read_token(&current.server_url)?);
    if let Some(body) = body {
        request = request.json(&body);
    }
    request
        .send()
        .await
        .map_err(|_| "Could not reach the Fluxkit server.".to_string())
}

#[tauri::command]
pub async fn server_project_members(
    app: tauri::AppHandle,
    project_id: String,
) -> Result<Vec<ServerProjectMember>, String> {
    let id = uuid::Uuid::parse_str(&project_id)
        .map_err(|_| "Invalid project identifier.".to_string())?;
    let response = server_api_request(
        &app,
        reqwest::Method::GET,
        &format!("/api/projects/{id}/members"),
        None,
    )
    .await?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    response
        .json()
        .await
        .map_err(|_| "The server returned an invalid project membership list.".to_string())
}

#[tauri::command]
pub async fn server_add_project_member(
    app: tauri::AppHandle,
    project_id: String,
    username: String,
    role: String,
) -> Result<(), String> {
    let id = uuid::Uuid::parse_str(&project_id)
        .map_err(|_| "Invalid project identifier.".to_string())?;
    if username.trim().is_empty()
        || username.len() > 64
        || !matches!(role.as_str(), "member" | "projectAdmin")
    {
        return Err("Enter a username and a supported project role.".into());
    }
    let response = server_api_request(
        &app,
        reqwest::Method::POST,
        &format!("/api/projects/{id}/members"),
        Some(serde_json::json!({ "username": username.trim(), "role": role })),
    )
    .await?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(response_error(response).await)
    }
}

#[tauri::command]
pub async fn server_update_project_member(
    app: tauri::AppHandle,
    project_id: String,
    user_id: String,
    role: String,
) -> Result<(), String> {
    let project_id = uuid::Uuid::parse_str(&project_id)
        .map_err(|_| "Invalid project identifier.".to_string())?;
    let user_id =
        uuid::Uuid::parse_str(&user_id).map_err(|_| "Invalid user identifier.".to_string())?;
    if !matches!(role.as_str(), "member" | "projectAdmin") {
        return Err("Unsupported project role.".into());
    }
    let response = server_api_request(
        &app,
        reqwest::Method::PUT,
        &format!("/api/projects/{project_id}/members/{user_id}"),
        Some(serde_json::json!({ "role": role })),
    )
    .await?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(response_error(response).await)
    }
}

#[tauri::command]
pub async fn server_remove_project_member(
    app: tauri::AppHandle,
    project_id: String,
    user_id: String,
) -> Result<(), String> {
    let project_id = uuid::Uuid::parse_str(&project_id)
        .map_err(|_| "Invalid project identifier.".to_string())?;
    let user_id =
        uuid::Uuid::parse_str(&user_id).map_err(|_| "Invalid user identifier.".to_string())?;
    let response = server_api_request(
        &app,
        reqwest::Method::DELETE,
        &format!("/api/projects/{project_id}/members/{user_id}"),
        None,
    )
    .await?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(response_error(response).await)
    }
}

#[tauri::command]
pub async fn server_account(app: tauri::AppHandle) -> Result<ServerAccount, String> {
    let response = server_api_request(&app, reqwest::Method::GET, "/api/account", None).await?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    response
        .json()
        .await
        .map_err(|_| "The server returned invalid account information.".to_string())
}

#[tauri::command]
pub async fn server_admin_users(
    app: tauri::AppHandle,
) -> Result<Vec<ServerAccountAdminView>, String> {
    let response = server_api_request(&app, reqwest::Method::GET, "/api/admin/users", None).await?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    response
        .json()
        .await
        .map_err(|_| "The server returned an invalid account list.".to_string())
}

#[tauri::command]
pub async fn server_admin_approve_user(
    app: tauri::AppHandle,
    user_id: String,
) -> Result<(), String> {
    let id = uuid::Uuid::parse_str(&user_id).map_err(|_| "Invalid user identifier.".to_string())?;
    let response = server_api_request(
        &app,
        reqwest::Method::POST,
        &format!("/api/admin/users/{id}/approve"),
        None,
    )
    .await?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(response_error(response).await)
    }
}

#[tauri::command]
pub async fn server_admin_set_user_status(
    app: tauri::AppHandle,
    user_id: String,
    status: String,
) -> Result<(), String> {
    let id = uuid::Uuid::parse_str(&user_id).map_err(|_| "Invalid user identifier.".to_string())?;
    if !matches!(status.as_str(), "active" | "disabled") {
        return Err("Unsupported account status.".into());
    }
    let response = server_api_request(
        &app,
        reqwest::Method::PUT,
        &format!("/api/admin/users/{id}/status"),
        Some(serde_json::json!({ "status": status })),
    )
    .await?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(response_error(response).await)
    }
}

/// Permanently deletes an account and its private server data (server administrators only).
#[tauri::command]
pub async fn server_admin_delete_user(
    app: tauri::AppHandle,
    user_id: String,
) -> Result<(), String> {
    let id = uuid::Uuid::parse_str(&user_id).map_err(|_| "Invalid user identifier.".to_string())?;
    let response = server_api_request(
        &app,
        reqwest::Method::DELETE,
        &format!("/api/admin/users/{id}"),
        None,
    )
    .await?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(response_error(response).await)
    }
}

/// Deletes an empty server project (server administrators only) and this device's local
/// mirror and read-only copies of it. Open views of the project are closed first by the UI.
#[tauri::command]
pub async fn server_delete_project(
    app: tauri::AppHandle,
    project_id: String,
    revision: i64,
) -> Result<(), String> {
    let id = uuid::Uuid::parse_str(&project_id)
        .map_err(|_| "Invalid project identifier.".to_string())?;
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let session = session_for(&root)?;
    let response = http_client()?
        .delete(api_url(
            &session.identity.server_url,
            &format!("/api/projects/{id}"),
        )?)
        .bearer_auth(&session.access_token)
        .header(reqwest::header::IF_MATCH, format!("\"{revision}\""))
        .send()
        .await
        .map_err(|_| {
            "Could not reach the Fluxkit server. The project was not deleted.".to_string()
        })?;
    if response.status() == reqwest::StatusCode::CONFLICT {
        #[derive(Deserialize)]
        struct Refusal {
            error: Option<String>,
        }
        let refusal: Option<Refusal> = response.json().await.ok();
        return Err(refusal
            .and_then(|refusal| refusal.error)
            .unwrap_or_else(|| "The project is not empty or changed meanwhile.".into()));
    }
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    ensure_session(&session)?;
    let scoped = account_root(&root, &session.identity)?;
    remove_local_copies(&scoped, id)
}

fn remove_local_copies(scoped: &Path, id: uuid::Uuid) -> Result<(), String> {
    let workspaces = scoped.join("server-workspaces");
    let pointer = workspaces.join(format!("{id}.json"));
    if pointer.exists() {
        super::projects::store::safe(&pointer, false)?;
        let generation: uuid::Uuid =
            super::projects::store::decode(&super::projects::store::read(&pointer)?)?;
        let directory = workspaces.join(generation.to_string());
        if directory.exists() {
            super::projects::store::safe(&directory, true)?;
            fs::remove_dir_all(&directory).map_err(|_| "The project was deleted on the server, but its local copy could not be removed.".to_string())?;
        }
        fs::remove_file(&pointer).map_err(|_| {
            "The project was deleted on the server, but its local copy could not be removed."
                .to_string()
        })?;
    }
    let snapshots = scoped.join("server-snapshots").join(id.to_string());
    if snapshots.exists() {
        super::projects::store::safe(&snapshots, true)?;
        fs::remove_dir_all(&snapshots).map_err(|_| "The project was deleted on the server, but its offline copies could not be removed.".to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn server_create_team_project(
    app: tauri::AppHandle,
    name: String,
) -> Result<ServerProject, String> {
    if name.trim().is_empty() || name.trim().len() > 200 {
        return Err("Project names must contain between 1 and 200 characters.".into());
    }
    let response = server_api_request(
        &app,
        reqwest::Method::POST,
        "/api/projects",
        Some(serde_json::json!({ "name": name.trim() })),
    )
    .await?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    response
        .json()
        .await
        .map_err(|_| "The server returned an invalid new project.".to_string())
}

#[tauri::command]
pub async fn server_connection_get(
    app: tauri::AppHandle,
    projects: tauri::State<'_, super::projects::commands::ProjectState>,
) -> Result<ConnectionView, String> {
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    let identity = read_session()?
        .map(|session| session.identity)
        .filter(|identity| identity.server_url == current.server_url);
    let account_id = identity.as_ref().map(|identity| identity.account_id);
    super::projects::commands::run(&projects, move |store| {
        store.set_server_identity(identity);
        Ok(())
    })
    .await?;
    Ok(ConnectionView {
        server_url: current.server_url,
        authenticated: account_id.is_some(),
        account_id,
    })
}

#[tauri::command]
pub async fn server_connection_set(
    app: tauri::AppHandle,
    projects: tauri::State<'_, super::projects::commands::ProjectState>,
    server_url: String,
) -> Result<ConnectionView, String> {
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let server_url = if server_url.trim().is_empty() {
        String::new()
    } else {
        normalize_url(&server_url)?
    };
    let previous = settings(&root)?;
    if previous.server_url != server_url {
        app.state::<oidc::OidcState>().cancel();
        app.state::<values::ValueState>().clear();
        app.state::<transfer::TransferState>().clear();
        match token_entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(_) => return Err(TOKEN_ERROR.into()),
        }
    }
    fs::create_dir_all(&root).map_err(|_| SETTINGS_ERROR)?;
    if root
        .symlink_metadata()
        .map_err(|_| SETTINGS_ERROR)?
        .file_type()
        .is_symlink()
    {
        return Err(SETTINGS_ERROR.into());
    }
    let settings = Settings {
        schema_version: 1,
        server_url,
    };
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|_| SETTINGS_ERROR)?;
    let path = root.join(SETTINGS_FILE);
    let new = !path.exists();
    super::projects::store::atomic(&path, &bytes, new).map_err(|_| SETTINGS_ERROR)?;
    server_connection_get(app, projects).await
}

#[tauri::command]
pub async fn server_register(
    app: tauri::AppHandle,
    username: String,
    password: String,
) -> Result<(), String> {
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    if current.server_url.is_empty() {
        return Err("Configure a server URL first.".into());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| "Could not create a secure server connection.".to_string())?;
    let response = client
        .post(api_url(&current.server_url, "/api/auth/register")?)
        .json(&serde_json::json!({ "username": username, "password": password }))
        .send()
        .await
        .map_err(|_| "Could not reach the Fluxkit server.".to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(response_error(response).await)
    }
}

#[tauri::command]
pub async fn server_login(
    app: tauri::AppHandle,
    projects: tauri::State<'_, super::projects::commands::ProjectState>,
    username: String,
    password: String,
) -> Result<ConnectionView, String> {
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    if current.server_url.is_empty() {
        return Err("Configure a server URL first.".into());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| "Could not create a secure server connection.".to_string())?;
    let response = client
        .post(api_url(&current.server_url, "/api/auth/login")?)
        .json(&serde_json::json!({ "username": username, "password": password }))
        .send()
        .await
        .map_err(|_| "Could not reach the Fluxkit server.".to_string())?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    let reply: LoginReply = response
        .json()
        .await
        .map_err(|_| "The server returned an invalid sign-in response.".to_string())?;
    finish_login(app, projects, current, reply, client, None).await
}

async fn finish_login(
    app: tauri::AppHandle,
    projects: tauri::State<'_, super::projects::commands::ProjectState>,
    current: Settings,
    reply: LoginReply,
    client: reqwest::Client,
    cancellation: Option<&tokio_util::sync::CancellationToken>,
) -> Result<ConnectionView, String> {
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let account_response = client
        .get(api_url(&current.server_url, "/api/account")?)
        .bearer_auth(&reply.access_token)
        .send()
        .await
        .map_err(|_| "Could not verify the server account.".to_string())?;
    if !account_response.status().is_success() {
        return Err(response_error(account_response).await);
    }
    let account: ServerAccount = account_response
        .json()
        .await
        .map_err(|_| "The server returned invalid account information.".to_string())?;
    let account_id = uuid::Uuid::parse_str(&account.id)
        .map_err(|_| "Invalid server account identity.".to_string())?;
    if settings(&root)?.server_url != current.server_url {
        return Err("The configured server changed during sign-in. Try again.".into());
    }
    let session = Session {
        identity: ServerIdentity {
            server_url: current.server_url,
            account_id,
        },
        access_token: reply.access_token,
    };
    let persist = || {
        app.state::<values::ValueState>().clear();
        app.state::<transfer::TransferState>().clear();
        token_entry()?
            .set_secret(&serde_json::to_vec(&session).map_err(|_| TOKEN_ERROR)?)
            .map_err(|_| TOKEN_ERROR.to_string())
    };
    if let Some(cancel) = cancellation {
        app.state::<oidc::OidcState>().commit(cancel, persist)?;
    } else {
        app.state::<oidc::OidcState>().cancel();
        persist()?;
    }
    server_connection_get(app, projects).await
}

#[tauri::command]
pub async fn server_logout(
    app: tauri::AppHandle,
    projects: tauri::State<'_, super::projects::commands::ProjectState>,
) -> Result<ConnectionView, String> {
    app.state::<oidc::OidcState>().cancel();
    app.state::<values::ValueState>().clear();
    app.state::<transfer::TransferState>().clear();
    match token_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(_) => return Err(TOKEN_ERROR.into()),
    }
    server_connection_get(app, projects).await
}

#[tauri::command]
pub async fn server_projects(app: tauri::AppHandle) -> Result<Vec<ServerProject>, String> {
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    let session = session_for(&root)?;
    let scoped_root = account_root(&root, &session.identity)?;
    if current.server_url.is_empty() {
        return Err("Configure a server URL first.".into());
    }
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| "Could not create a secure server connection.".to_string())?
        .get(api_url(&current.server_url, "/api/projects")?)
        .bearer_auth(&session.access_token)
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => {
            ensure_session(&session)?;
            return cached_project_list(&scoped_root, &current.server_url);
        }
        Err(_) => return Err("Could not reach the Fluxkit server.".into()),
    };
    if response.status().is_server_error()
        || response.status() == reqwest::StatusCode::REQUEST_TIMEOUT
    {
        ensure_session(&session)?;
        return cached_project_list(&scoped_root, &current.server_url);
    }
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    let projects: Vec<ServerProject> = response
        .json()
        .await
        .map_err(|_| "The server returned an invalid project list.".to_string())?;
    ensure_session(&session)?;
    ensure_directory(&scoped_root)?;
    let manifest = CachedProjectList {
        server_url: current.server_url,
        projects: projects.clone(),
    };
    let bytes = serde_json::to_vec(&manifest).map_err(|_| SETTINGS_ERROR)?;
    let path = scoped_root.join("server-project-list.json");
    let new = !path.exists();
    super::projects::store::atomic(&path, &bytes, new).map_err(|_| SETTINGS_ERROR)?;
    Ok(projects)
}

#[tauri::command]
pub async fn server_open_project(
    app: tauri::AppHandle,
    projects: tauri::State<'_, super::projects::commands::ProjectState>,
    project_id: String,
) -> Result<super::projects::store::ProjectView, String> {
    let id = uuid::Uuid::parse_str(&project_id)
        .map_err(|_| "Invalid project identifier.".to_string())?;
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    let session = session_for(&root)?;
    let scoped_root = account_root(&root, &session.identity)?;
    if current.server_url.is_empty() {
        return Err("Configure a server URL first.".into());
    }
    let _serialized = workspace::guard(&app).await;
    let (path, online_edit) = match workspace::fetch_tree(&session, &id.to_string()).await? {
        Some(tree) if tree.project.can_edit => (
            server_workspace(&scoped_root, &session.identity, &tree)?,
            true,
        ),
        Some(tree) => (
            cache_snapshot(&scoped_root, &session.identity, &tree)?,
            false,
        ),
        None => (offline_project(&scoped_root, &session.identity, id)?, false),
    };
    ensure_session(&session)?;
    super::projects::commands::run(&projects, move |store| {
        store.require_server_identity(&session.identity)?;
        if online_edit {
            store.open_server_workspace(&path)
        } else {
            store.open(&path)
        }
    })
    .await
}

#[cfg(test)]
mod tests;
