//! Native boundary for an optional self-hosted Fluxkit server connection.
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};
use tauri::Manager;

const SERVICE: &str = "app.fluxkit.server-session.v1";
const ACCOUNT: &str = "access-token";
const SETTINGS_FILE: &str = "server.json";
const LIMIT: u64 = 16 * 1024;
const SETTINGS_ERROR: &str = "Could not access server connection settings.";
const TOKEN_ERROR: &str = "Could not access the server session in the operating system credential store.";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Settings {
    schema_version: u32,
    server_url: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    server_url: String,
    authenticated: bool,
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
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerProjectSnapshot {
    id: String,
    name: String,
    kind: String,
    snapshot: String,
    revision: i64,
    #[serde(default)]
    can_edit: bool,
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
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SnapshotContents {
    project: super::projects::model::Project,
    collections: Vec<super::projects::model::Collection>,
    environments: super::projects::environments::Definitions,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CachedProjectList {
    server_url: String,
    projects: Vec<ServerProject>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServerCacheMarker {
    server_url: String,
    server_project_id: uuid::Uuid,
    revision: i64,
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
            return Ok(Settings { schema_version: 1, server_url: String::new() });
        }
        Err(_) => return Err(SETTINGS_ERROR.into()),
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > LIMIT => {
            return Err(SETTINGS_ERROR.into());
        }
        Ok(_) => fs::read(&path).map_err(|_| SETTINGS_ERROR)?,
    };
    let settings: Settings = serde_json::from_slice(&bytes).map_err(|_| SETTINGS_ERROR)?;
    if settings.schema_version != 1 || (!settings.server_url.is_empty() && normalize_url(&settings.server_url).is_err()) {
        return Err(SETTINGS_ERROR.into());
    }
    Ok(settings)
}

fn normalize_url(value: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(value.trim()).map_err(|_| "Enter a valid server URL.".to_string())?;
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

fn read_token() -> Result<String, String> {
    match token_entry()?.get_secret() {
        Ok(value) => String::from_utf8(value).map_err(|_| TOKEN_ERROR.into()),
        Err(keyring::Error::NoEntry) => Err("Sign in to the configured Fluxkit server first.".into()),
        Err(_) => Err(TOKEN_ERROR.into()),
    }
}

fn api_url(root: &str, path: &str) -> Result<reqwest::Url, String> {
    reqwest::Url::parse(&format!("{root}{path}")).map_err(|_| "Invalid server URL.".into())
}

fn ensure_directory(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err("Server snapshot cache contains an unsafe path.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|_| "Could not create the server snapshot cache.".to_string())
        }
        Err(_) => Err("Could not access the server snapshot cache.".into()),
    }
}

fn cache_snapshot(
    root: &Path,
    server_url: &str,
    value: &ServerProjectSnapshot,
) -> Result<PathBuf, String> {
    if value.revision < 1 || value.snapshot.len() > 32 * 1024 * 1024 {
        return Err("The server project snapshot exceeds the local cache limit.".into());
    }
    let project_id = uuid::Uuid::parse_str(&value.id)
        .map_err(|_| "The server returned an invalid project identifier.".to_string())?;
    let contents: SnapshotContents = serde_json::from_str(&value.snapshot)
        .map_err(|_| "The server returned an invalid project snapshot.".to_string())?;
    contents.project.validate()?;
    contents.environments.validate()?;
    if contents.project.id != project_id.to_string() || contents.project.name != value.name {
        return Err("The server project metadata does not match its snapshot.".into());
    }
    if contents.collections.len() > 1_000 {
        return Err("The server project contains too many collections.".into());
    }
    for collection in &contents.collections {
        collection.validate()?;
        if collection.requests.iter().any(|request| {
            request.body.file.is_some() || request.body.fields.iter().any(|field| field.file.is_some())
        }) {
            return Err("The server snapshot references unavailable file attachments.".into());
        }
    }

    ensure_directory(root)?;
    let cache_root = root.join("server-snapshots");
    ensure_directory(&cache_root)?;
    let project_root = cache_root.join(project_id.to_string());
    ensure_directory(&project_root)?;
    let snapshot_root = project_root.join(value.revision.to_string());
    if snapshot_root.exists() {
        super::projects::store::safe(&snapshot_root, true)?;
        let marker = snapshot_root.join(".fluxkit/server-cache.json");
        super::projects::store::safe(&marker, false)?;
        let marker_data: ServerCacheMarker = serde_json::from_slice(
            &super::projects::store::read(&marker)?,
        )
        .map_err(|_| "The local server snapshot marker is invalid.".to_string())?;
        if marker_data.server_url != server_url
            || marker_data.server_project_id != project_id
            || marker_data.revision != value.revision
        {
            return Err("The local server snapshot belongs to a different server revision.".into());
        }
        return Ok(snapshot_root);
    }
    ensure_directory(&snapshot_root)?;
    let collections_root = snapshot_root.join("collections");
    ensure_directory(&collections_root)?;
    let private_root = snapshot_root.join(".fluxkit");
    ensure_directory(&private_root)?;

    let project_bytes = super::projects::store::encode(&contents.project)?;
    super::projects::store::atomic(&snapshot_root.join("project.fluxkit.json"), &project_bytes, true)?;
    for collection in contents.collections {
        let bytes = super::projects::store::encode(&collection)?;
        super::projects::store::atomic(
            &collections_root.join(format!("{}.json", collection.id)),
            &bytes,
            true,
        )?;
    }
    let environment_bytes = super::projects::store::encode(&contents.environments)?;
    super::projects::store::atomic(
        &snapshot_root.join("environments.fluxkit.json"),
        &environment_bytes,
        true,
    )?;
    let marker = serde_json::to_vec(&ServerCacheMarker {
        server_url: server_url.to_string(),
        server_project_id: project_id,
        revision: value.revision,
    })
    .map_err(|_| "Could not write server snapshot metadata.".to_string())?;
    super::projects::store::atomic(&private_root.join("server-cache.json"), &marker, true)?;
    Ok(snapshot_root)
}

fn server_workspace(
    root: &Path,
    server_url: &str,
    value: &ServerProjectSnapshot,
    cached_root: &Path,
) -> Result<PathBuf, String> {
    let project_id = uuid::Uuid::parse_str(&value.id)
        .map_err(|_| "The server returned an invalid project identifier.".to_string())?;
    let workspaces = root.join("server-workspaces");
    ensure_directory(root)?;
    ensure_directory(&workspaces)?;
    let destination = workspaces.join(project_id.to_string());
    match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            let marker_path = destination.join(".fluxkit/server-workspace.json");
            super::projects::store::safe(&marker_path, false)?;
            let marker: super::projects::store::ServerWorkspaceMarker =
                super::projects::store::decode(&super::projects::store::read(&marker_path)?)?;
            if marker.server_url != server_url || marker.server_project_id != project_id {
                return Err("A local server workspace belongs to a different server.".into());
            }
            return Ok(destination);
        }
        Ok(_) => return Err("The server workspace path is unsafe.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Could not access the server workspace.".into()),
    }

    let staging = tempfile::Builder::new()
        .prefix(".fluxkit-workspace-")
        .tempdir_in(&workspaces)
        .map_err(|_| "Could not create a server workspace.".to_string())?;
    let stage_root = staging.path();
    let collections_root = stage_root.join("collections");
    let private_root = stage_root.join(".fluxkit");
    ensure_directory(&collections_root)?;
    ensure_directory(&private_root)?;
    for filename in ["project.fluxkit.json", "environments.fluxkit.json"] {
        let bytes = super::projects::store::read(&cached_root.join(filename))?;
        super::projects::store::atomic(&stage_root.join(filename), &bytes, true)?;
    }
    let source_collections = cached_root.join("collections");
    super::projects::store::safe(&source_collections, true)?;
    for entry in fs::read_dir(&source_collections)
        .map_err(|_| "Could not read the server snapshot collections.".to_string())?
    {
        let entry = entry.map_err(|_| "Could not read the server snapshot collections.".to_string())?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let bytes = super::projects::store::read(&path)?;
        super::projects::store::atomic(
            &collections_root.join(entry.file_name()),
            &bytes,
            true,
        )?;
    }
    let marker = serde_json::to_vec_pretty(&super::projects::store::ServerWorkspaceMarker {
        server_url: server_url.to_string(),
        server_project_id: project_id,
        revision: value.revision,
    })
    .map_err(|_| "Could not write server workspace metadata.".to_string())?;
    super::projects::store::atomic(&private_root.join("server-workspace.json"), &marker, true)?;
    let staging_path = staging.keep();
    fs::rename(&staging_path, &destination)
        .map_err(|_| "Could not activate the server workspace.".to_string())?;
    Ok(destination)
}

fn latest_cached_snapshot(
    root: &Path,
    server_url: &str,
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
            if marker.server_url == server_url
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
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| "Could not create a secure server connection.".to_string())?;
    let mut request = client
        .request(method, api_url(&current.server_url, path)?)
        .bearer_auth(read_token()?);
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
    let user_id = uuid::Uuid::parse_str(&user_id)
        .map_err(|_| "Invalid user identifier.".to_string())?;
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
    let user_id = uuid::Uuid::parse_str(&user_id)
        .map_err(|_| "Invalid user identifier.".to_string())?;
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
    let id = uuid::Uuid::parse_str(&user_id)
        .map_err(|_| "Invalid user identifier.".to_string())?;
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
    let id = uuid::Uuid::parse_str(&user_id)
        .map_err(|_| "Invalid user identifier.".to_string())?;
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

#[tauri::command]
pub async fn server_create_team_project(
    app: tauri::AppHandle,
    name: String,
) -> Result<ServerProjectSnapshot, String> {
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
pub fn server_connection_get(app: tauri::AppHandle) -> Result<ConnectionView, String> {
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    let authenticated = match token_entry()?.get_secret() {
        Ok(_) => true,
        Err(keyring::Error::NoEntry) => false,
        Err(_) => return Err(TOKEN_ERROR.into()),
    };
    Ok(ConnectionView { server_url: current.server_url, authenticated })
}

#[tauri::command]
pub fn server_connection_set(app: tauri::AppHandle, server_url: String) -> Result<ConnectionView, String> {
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let server_url = if server_url.trim().is_empty() { String::new() } else { normalize_url(&server_url)? };
    let previous = settings(&root)?;
    if previous.server_url != server_url {
        match token_entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(_) => return Err(TOKEN_ERROR.into()),
        }
    }
    fs::create_dir_all(&root).map_err(|_| SETTINGS_ERROR)?;
    if root.symlink_metadata().map_err(|_| SETTINGS_ERROR)?.file_type().is_symlink() {
        return Err(SETTINGS_ERROR.into());
    }
    let settings = Settings { schema_version: 1, server_url };
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|_| SETTINGS_ERROR)?;
    let path = root.join(SETTINGS_FILE);
    let new = !path.exists();
    super::projects::store::atomic(&path, &bytes, new).map_err(|_| SETTINGS_ERROR)?;
    server_connection_get(app)
}

#[tauri::command]
pub async fn server_register(app: tauri::AppHandle, username: String, password: String) -> Result<(), String> {
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    if current.server_url.is_empty() { return Err("Configure a server URL first.".into()); }
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build()
        .map_err(|_| "Could not create a secure server connection.".to_string())?;
    let response = client.post(api_url(&current.server_url, "/api/auth/register")?)
        .json(&serde_json::json!({ "username": username, "password": password })).send().await
        .map_err(|_| "Could not reach the Fluxkit server.".to_string())?;
    if response.status().is_success() { Ok(()) } else { Err(response_error(response).await) }
}

#[tauri::command]
pub async fn server_login(app: tauri::AppHandle, username: String, password: String) -> Result<ConnectionView, String> {
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    if current.server_url.is_empty() { return Err("Configure a server URL first.".into()); }
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build()
        .map_err(|_| "Could not create a secure server connection.".to_string())?;
    let response = client.post(api_url(&current.server_url, "/api/auth/login")?)
        .json(&serde_json::json!({ "username": username, "password": password })).send().await
        .map_err(|_| "Could not reach the Fluxkit server.".to_string())?;
    if !response.status().is_success() { return Err(response_error(response).await); }
    let reply: LoginReply = response.json().await.map_err(|_| "The server returned an invalid sign-in response.".to_string())?;
    token_entry()?.set_secret(reply.access_token.as_bytes()).map_err(|_| TOKEN_ERROR)?;
    server_connection_get(app)
}

#[tauri::command]
pub fn server_logout(app: tauri::AppHandle) -> Result<ConnectionView, String> {
    match token_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(_) => return Err(TOKEN_ERROR.into()),
    }
    server_connection_get(app)
}

#[tauri::command]
pub async fn server_projects(app: tauri::AppHandle) -> Result<Vec<ServerProject>, String> {
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    if current.server_url.is_empty() { return Err("Configure a server URL first.".into()); }
    let response = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build()
        .map_err(|_| "Could not create a secure server connection.".to_string())?
        .get(api_url(&current.server_url, "/api/projects")?)
        .bearer_auth(read_token()?).send().await;
    let response = match response {
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => {
            return cached_project_list(&root, &current.server_url);
        }
        Err(_) => return Err("Could not reach the Fluxkit server.".into()),
    };
    if response.status().is_server_error() || response.status() == reqwest::StatusCode::REQUEST_TIMEOUT {
        return cached_project_list(&root, &current.server_url);
    }
    if !response.status().is_success() { return Err(response_error(response).await); }
    let projects: Vec<ServerProject> = response.json().await
        .map_err(|_| "The server returned an invalid project list.".to_string())?;
    ensure_directory(&root)?;
    let manifest = CachedProjectList { server_url: current.server_url, projects: projects.clone() };
    let bytes = serde_json::to_vec(&manifest).map_err(|_| SETTINGS_ERROR)?;
    let path = root.join("server-project-list.json");
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
    if current.server_url.is_empty() {
        return Err("Configure a server URL first.".into());
    }
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| "Could not create a secure server connection.".to_string())?
        .get(api_url(&current.server_url, &format!("/api/projects/{project_id}"))?)
        .bearer_auth(read_token()?)
        .send()
        .await;
    let (snapshot_root, online_edit) = match response {
        Ok(response)
            if response.status().is_server_error()
                || response.status() == reqwest::StatusCode::REQUEST_TIMEOUT =>
        {
            (
                latest_cached_snapshot(&root, &current.server_url, id)?
                    .ok_or("No offline snapshot exists for this server project.")?,
                false,
            )
        }
        Ok(response) => {
            if !response.status().is_success() {
                return Err(response_error(response).await);
            }
            if response.content_length().is_some_and(|length| length > 34 * 1024 * 1024) {
                return Err("The server project snapshot exceeds the local cache limit.".into());
            }
            let bytes = response.bytes().await
                .map_err(|_| "Could not read the server project snapshot.".to_string())?;
            if bytes.len() > 34 * 1024 * 1024 {
                return Err("The server project snapshot exceeds the local cache limit.".into());
            }
            let snapshot: ServerProjectSnapshot = serde_json::from_slice(&bytes)
                .map_err(|_| "The server returned an invalid project snapshot.".to_string())?;
            if snapshot.id != id.to_string() {
                return Err("The server returned a different project than requested.".into());
            }
            let cached = cache_snapshot(&root, &current.server_url, &snapshot)?;
            if snapshot.can_edit {
                (
                    server_workspace(&root, &current.server_url, &snapshot, &cached)?,
                    true,
                )
            } else {
                (cached, false)
            }
        }
        Err(error) if error.is_connect() || error.is_timeout() => {
            (
                latest_cached_snapshot(&root, &current.server_url, id)?
                    .ok_or("No offline snapshot exists for this server project.")?,
                false,
            )
        }
        Err(_) => return Err("Could not reach the Fluxkit server.".into()),
    };
    super::projects::commands::run(&projects, move |store| {
        if online_edit {
            store.open_server_workspace(&snapshot_root)
        } else {
            store.open(&snapshot_root)
        }
    })
    .await
}

#[tauri::command]
pub async fn server_project_snapshot(
    app: tauri::AppHandle,
    project_id: String,
) -> Result<ServerProjectSnapshot, String> {
    uuid::Uuid::parse_str(&project_id).map_err(|_| "Invalid project identifier.".to_string())?;
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    if current.server_url.is_empty() {
        return Err("Configure a server URL first.".into());
    }
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| "Could not create a secure server connection.".to_string())?
        .get(api_url(&current.server_url, &format!("/api/projects/{project_id}"))?)
        .bearer_auth(read_token()?)
        .send()
        .await
        .map_err(|_| "Could not reach the Fluxkit server.".to_string())?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    response
        .json()
        .await
        .map_err(|_| "The server returned an invalid project snapshot.".to_string())
}

#[tauri::command]
pub async fn server_save_project_snapshot(
    app: tauri::AppHandle,
    project_id: String,
    revision: i64,
    snapshot: String,
) -> Result<ServerProjectSnapshot, String> {
    uuid::Uuid::parse_str(&project_id).map_err(|_| "Invalid project identifier.".to_string())?;
    if revision < 1 || snapshot.len() > 32 * 1024 * 1024 {
        return Err("Invalid revision or project snapshot size.".into());
    }
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    if current.server_url.is_empty() {
        return Err("Configure a server URL first.".into());
    }
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| "Could not create a secure server connection.".to_string())?
        .put(api_url(&current.server_url, &format!("/api/projects/{project_id}"))?)
        .bearer_auth(read_token()?)
        .header(reqwest::header::IF_MATCH, format!("\"{revision}\""))
        .json(&serde_json::json!({ "snapshot": snapshot }))
        .send()
        .await
        .map_err(|_| "Could not reach the Fluxkit server.".to_string())?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    response
        .json()
        .await
        .map_err(|_| "The server returned an invalid project snapshot.".to_string())
}

#[tauri::command]
pub async fn server_sync_project(
    app: tauri::AppHandle,
    projects: tauri::State<'_, super::projects::commands::ProjectState>,
    handle: String,
) -> Result<ServerProjectSnapshot, String> {
    let snapshot_handle = handle.clone();
    let (marker, project_id, _name, snapshot) =
        super::projects::commands::run(&projects, move |store| {
            let marker = store.server_workspace_marker(&snapshot_handle)?;
            let (project_id, name, snapshot) = store.snapshot(&snapshot_handle)?;
            Ok((marker, project_id, name, snapshot))
        })
        .await?;
    let id = uuid::Uuid::parse_str(&project_id)
        .map_err(|_| "The local project has an invalid identity.".to_string())?;
    if id != marker.server_project_id {
        return Err("This project is not the server workspace recorded on disk.".into());
    }
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    if current.server_url != marker.server_url {
        return Err("The configured server differs from this workspace. Switch back to its server URL before syncing.".into());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| "Could not create a secure server connection.".to_string())?;
    let remote_response = client
        .get(api_url(&current.server_url, &format!("/api/projects/{id}"))?)
        .bearer_auth(read_token()?)
        .send()
        .await
        .map_err(|_| "Could not reach the Fluxkit server. Local edits remain saved.".to_string())?;
    if !remote_response.status().is_success() {
        return Err(response_error(remote_response).await);
    }
    let remote: ServerProjectSnapshot = remote_response
        .json()
        .await
        .map_err(|_| "The server returned an invalid project snapshot.".to_string())?;
    if remote.revision != marker.revision {
        return Err("The server project changed since this workspace was opened. Local edits remain saved; this sync was rejected.".into());
    }
    if remote.snapshot == snapshot {
        return Ok(remote);
    }
    let response = client
        .put(api_url(&current.server_url, &format!("/api/projects/{id}"))?)
        .bearer_auth(read_token()?)
        .header(reqwest::header::IF_MATCH, format!("\"{}\"", marker.revision))
        .json(&serde_json::json!({ "snapshot": snapshot }))
        .send()
        .await
        .map_err(|_| "Could not reach the Fluxkit server. Local edits remain saved.".to_string())?;
    if !response.status().is_success() {
        if response.status() == reqwest::StatusCode::CONFLICT {
            return Err("The server project changed during synchronization. Local edits remain saved; this sync was rejected.".into());
        }
        return Err(response_error(response).await);
    }
    let saved: ServerProjectSnapshot = response
        .json()
        .await
        .map_err(|_| "The server returned an invalid synchronization response.".to_string())?;
    if saved.id != id.to_string() || saved.revision <= marker.revision {
        return Err("The server returned an unexpected project revision.".into());
    }
    let saved_revision = saved.revision;
    let server_url = marker.server_url.clone();
    let marker_project_id = marker.server_project_id;
    super::projects::commands::run(&projects, move |store| {
        store.update_server_workspace_revision(
            &handle,
            &server_url,
            marker_project_id,
            saved_revision,
        )
    })
    .await?;
    let cache_result = cache_snapshot(&root, &current.server_url, &saved);
    if cache_result.is_err() {
        return Err("Project changes reached the server, but the offline snapshot could not be refreshed.".into());
    }
    Ok(saved)
}

#[tauri::command]
pub async fn server_upload_local_project(
    app: tauri::AppHandle,
    projects: tauri::State<'_, super::projects::commands::ProjectState>,
    handle: String,
) -> Result<ServerProjectSnapshot, String> {
    let (project_id, name, snapshot) = super::projects::commands::run(&projects, move |store| {
        store.snapshot(&handle)
    })
    .await?;
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    if current.server_url.is_empty() {
        return Err("Configure a server URL first.".into());
    }
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| "Could not create a secure server connection.".to_string())?
        .post(api_url(
            &current.server_url,
            &format!("/api/projects/{project_id}/takeover"),
        )?)
        .bearer_auth(read_token()?)
        .json(&serde_json::json!({
            "name": name,
            "kind": "personal",
            "snapshot": snapshot,
            "encryptedSecrets": [],
        }))
        .send()
        .await
        .map_err(|_| "Could not reach the Fluxkit server.".to_string())?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    response
        .json()
        .await
        .map_err(|_| "The server returned an invalid project after upload.".to_string())
}
