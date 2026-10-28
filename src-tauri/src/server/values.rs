//! Synchronized value ownership, bounded caches and an explicitly unlocked account key.
use super::{
    crypto::{self, CipherValue, DataKey, Profile},
    ServerIdentity,
};
use crate::projects::{
    commands::{run, ProjectState},
    environments::{self, Definitions, OsValues, ScopedValues},
    store,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{Manager, State};
use zeroize::Zeroizing;

const UNSYNCED_DEFINITIONS: &str = "The server does not know this variable or environment yet, or its secret setting changed locally. Use Sync saved changes in the server dialog to publish the variable definitions first, then save the value again.";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueRevisions {
    pub shared: i64,
    pub secrets: i64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SharedValue {
    pub variable_id: String,
    pub environment_id: String,
    pub value: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bundle<T> {
    pub revision: i64,
    pub project_revision: i64,
    pub values: Vec<T>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountProfile {
    pub revision: i64,
    pub profile: Option<Profile>,
}
#[derive(Default)]
struct Inner {
    generation: u64,
    key_operation_running: bool,
    profiles: HashMap<String, AccountProfile>,
    keys: HashMap<String, DataKey>,
    shared: HashMap<String, Bundle<SharedValue>>,
    secrets: HashMap<String, Bundle<CipherValue>>,
}
#[derive(Clone, Default)]
pub struct ValueState(Arc<Mutex<Inner>>);
impl ValueState {
    fn with<T>(&self, action: impl FnOnce(&mut Inner) -> Result<T, String>) -> Result<T, String> {
        let mut inner = self
            .0
            .lock()
            .map_err(|_| "Server value state is unavailable. Restart Fluxkit.".to_string())?;
        action(&mut inner)
    }
    fn reserve_key_operation(&self) -> Result<KeyOperation, String> {
        self.with(|inner| {
            if inner.key_operation_running {
                return Err(
                    "Another vault key operation is running. Wait for it to finish.".into(),
                );
            }
            inner.key_operation_running = true;
            Ok(KeyOperation(self.clone()))
        })
    }
    pub fn clear(&self) {
        if let Ok(mut inner) = self.0.lock() {
            let generation = inner.generation.wrapping_add(1);
            let running = inner.key_operation_running;
            *inner = Inner::default();
            inner.generation = generation;
            inner.key_operation_running = running;
        }
    }
    pub fn lock(&self, identity: &ServerIdentity) -> Result<(), String> {
        self.with(|inner| {
            inner.keys.remove(&identity.namespace());
            inner.generation = inner.generation.wrapping_add(1);
            Ok(())
        })
    }
    pub fn key(&self, identity: &ServerIdentity) -> Result<DataKey, String> {
        self.with(|inner| {
            inner
                .keys
                .get(&identity.namespace())
                .cloned()
                .ok_or_else(|| "Unlock your account vault with its master password first.".into())
        })
    }
    fn install_key(
        &self,
        owner: &ServerIdentity,
        profile: AccountProfile,
        key: DataKey,
        generation: u64,
    ) -> Result<(), String> {
        self.with(|inner| {
            if inner.generation != generation { return Err("The vault was locked or the account changed during unlock. Unlock again explicitly.".into()); }
            inner.profiles.insert(owner.namespace(), profile);
            inner.keys.insert(owner.namespace(), key);
            inner.generation = inner.generation.wrapping_add(1);
            Ok(())
        })
    }
    pub fn revisions(
        &self,
        identity: &ServerIdentity,
        project: &str,
    ) -> Result<ValueRevisions, String> {
        self.with(|inner| {
            let scope = scope(identity, project);
            Ok(ValueRevisions {
                shared: inner
                    .shared
                    .get(&scope)
                    .ok_or("Reload shared values.")?
                    .revision,
                secrets: inner
                    .secrets
                    .get(&scope)
                    .ok_or("Reload secret metadata.")?
                    .revision,
            })
        })
    }
    pub fn access(
        &self,
        identity: ServerIdentity,
        project: String,
        definitions: Definitions,
    ) -> ValueAccess {
        ValueAccess {
            state: self.clone(),
            identity,
            project,
            definitions,
        }
    }
}
struct KeyOperation(ValueState);
impl Drop for KeyOperation {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.0 .0.lock() {
            inner.key_operation_running = false;
        }
    }
}
fn scope(identity: &ServerIdentity, project: &str) -> String {
    format!("{}/{project}", identity.namespace())
}
#[derive(Clone)]
pub struct ValueAccess {
    state: ValueState,
    identity: ServerIdentity,
    project: String,
    definitions: Definitions,
}
impl ValueAccess {
    fn slot<'a>(&self, key: &'a str) -> Result<(&'a str, &'a str, bool), String> {
        let parts: Vec<_> = key.split('/').collect();
        if parts.len() != 3 || parts[0] != self.project {
            return Err("Invalid synchronized value reference.".into());
        }
        let variable = self
            .definitions
            .variables
            .iter()
            .find(|variable| variable.id == parts[1])
            .ok_or("The variable definition is missing.")?;
        Ok((parts[1], parts[2], variable.secret.unwrap_or(true)))
    }
    pub fn get(&self, key: &str) -> Result<Option<String>, String> {
        let (variable, environment, secret) = self.slot(key)?;
        self.state.with(|inner| {
            let id = scope(&self.identity, &self.project);
            if secret {
                let bundle = inner
                    .secrets
                    .get(&id)
                    .ok_or("Load server values before resolving secrets.")?;
                let Some(value) = bundle.values.iter().find(|value| {
                    value.variable_id == variable && value.environment_id == environment
                }) else {
                    return Ok(None);
                };
                let data_key = inner
                    .keys
                    .get(&self.identity.namespace())
                    .ok_or("Unlock your account vault with its master password first.")?;
                Ok(Some(
                    crypto::decrypt(&self.identity, &self.project, value, data_key)?.to_string(),
                ))
            } else {
                let bundle = inner
                    .shared
                    .get(&id)
                    .ok_or("Load shared server values before resolving variables.")?;
                Ok(bundle
                    .values
                    .iter()
                    .find(|value| {
                        value.variable_id == variable && value.environment_id == environment
                    })
                    .map(|value| value.value.clone()))
            }
        })
    }
    pub fn check_removal(&self, key: &str) -> Result<(), String> {
        let (variable, environment, _) = self.slot(key)?;
        let id = scope(&self.identity, &self.project);
        self.state.with(|inner| {
            let shared = inner.shared.get(&id).ok_or("Load server values before removing definitions.")?;
            let secrets = inner.secrets.get(&id).ok_or("Load server values before removing definitions.")?;
            if shared.values.iter().any(|value| value.variable_id == variable && value.environment_id == environment)
                || secrets.values.iter().any(|value| value.variable_id == variable && value.environment_id == environment) {
                return Err("Remove synchronized values explicitly before deleting their definitions. Other users' encrypted values are retained.".into());
            }
            Ok(())
        })
    }
}
#[derive(Clone)]
pub struct Context {
    pub identity: ServerIdentity,
    pub project: String,
    pub definitions: Definitions,
    pub definitions_revision: String,
}
pub async fn context(
    projects: &State<'_, ProjectState>,
    handle: &str,
    writable: bool,
) -> Result<Option<Context>, String> {
    let handle = handle.to_string();
    run(projects, move |store| {
        if writable {
            store.ensure_writable(&handle)?;
        }
        store.with_environment(&handle, |root, project| {
            let Some(identity) = store::server_owner(root)? else {
                return Ok(None);
            };
            let (definitions, definitions_revision) = environments::definitions(root)?;
            Ok(Some(Context {
                identity,
                project: project.into(),
                definitions,
                definitions_revision,
            }))
        })
    })
    .await
}
pub(super) async fn read_json<T: DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Could not read synchronized values.")?
    {
        if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
            return Err("The synchronized value response exceeds 8 MiB.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| "The server returned invalid synchronized value data.".into())
}
async fn get<T: DeserializeOwned>(
    session: &super::Session,
    path: &str,
) -> Result<Option<T>, String> {
    let response = client()?
        .get(super::api_url(&session.identity.server_url, path)?)
        .bearer_auth(&session.access_token)
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => {
            return Ok(None);
        }
        Err(_) => return Err("Could not reach the Fluxkit server.".into()),
    };
    if response.status().is_server_error()
        || response.status() == reqwest::StatusCode::REQUEST_TIMEOUT
    {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(super::response_error(response).await);
    }
    Ok(Some(read_json(response).await?))
}
fn client() -> Result<reqwest::Client, String> {
    let builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20));
    #[cfg(test)]
    let builder = protocol_tests::fixture_client(builder)?;
    builder
        .build()
        .map_err(|_| "Could not create a secure server connection.".into())
}
async fn put<T: Serialize, R: DeserializeOwned>(
    session: &super::Session,
    path: &str,
    revision: i64,
    body: &T,
) -> Result<R, String> {
    put_with(session, path, revision, body, None).await
}
/// The server rejects values for variables/environments missing from its definition snapshot,
/// or with a different secret classification, with 400. `bad_request` explains that case.
async fn put_with<T: Serialize, R: DeserializeOwned>(
    session: &super::Session,
    path: &str,
    revision: i64,
    body: &T,
    bad_request: Option<&str>,
) -> Result<R, String> {
    let response = client()?.put(super::api_url(&session.identity.server_url, path)?)
        .bearer_auth(&session.access_token).header(reqwest::header::IF_MATCH, format!("\"{revision}\""))
        .json(body).send().await.map_err(|_| "The server response was interrupted. Reload values to check whether the change was saved; no automatic retry was made.".to_string())?;
    if response.status() == reqwest::StatusCode::BAD_REQUEST {
        if let Some(message) = bad_request {
            return Err(message.into());
        }
    }
    if !response.status().is_success() {
        return Err(super::response_error(response).await);
    }
    let reply = read_json(response).await?;
    Ok(reply)
}
fn cached<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&store::read(path)?)
        .map_err(|_| "The local synchronized value cache is invalid.".into())
}
fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    store::atomic(path, &store::encode(value)?, !path.exists()).map_err(|_| "The value reached the server, but its local cache could not be saved. Reload values before retrying.".into())
}
fn bundle_paths(root: &Path, project: &str) -> (PathBuf, PathBuf) {
    (
        root.join(format!("shared-{project}.json")),
        root.join(format!("encrypted-{project}.json")),
    )
}
pub async fn refresh(app: &tauri::AppHandle, context: &Context) -> Result<(), String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| super::SETTINGS_ERROR)?;
    let session = super::session_for(&root)?;
    if session.identity != context.identity {
        return Err("This workspace belongs to another server account.".into());
    }
    let scoped = super::account_root(&root, &context.identity)?;
    let (shared_path, secret_path) = bundle_paths(&scoped, &context.project);
    let profile_path = scoped.join("vault-profile.json");
    let shared = get::<Bundle<SharedValue>>(
        &session,
        &format!("/api/projects/{}/values", context.project),
    )
    .await?;
    let secrets = get::<Bundle<CipherValue>>(
        &session,
        &format!("/api/projects/{}/vault-values", context.project),
    )
    .await?;
    let profile = get::<AccountProfile>(&session, "/api/account/vault").await?;
    super::ensure_session(&session)?;
    let fetched_shared = shared.is_some();
    let fetched_secrets = secrets.is_some();
    let fetched_profile = profile.is_some();
    let shared = match shared {
        Some(value) => value,
        None => cached(&shared_path)?,
    };
    let secrets = match secrets {
        Some(value) => value,
        None => cached(&secret_path)?,
    };
    let profile = match profile {
        Some(value) => value,
        None => cached(&profile_path)?,
    };
    if shared.values.len() > 1000
        || secrets.values.len() > 1000
        || shared.revision < 0
        || secrets.revision < 0
        || shared.project_revision < 1
        || secrets.project_revision < 1
        || profile.revision < 0
        || shared.values.iter().any(|value| value.value.len() > 2560)
    {
        return Err("Invalid synchronized value limits.".into());
    }
    if fetched_shared {
        save(&shared_path, &shared)?;
    }
    if fetched_secrets {
        save(&secret_path, &secrets)?;
    }
    if fetched_profile {
        save(&profile_path, &profile)?;
    }
    app.state::<ValueState>().with(|inner| {
        let account = context.identity.namespace();
        if inner
            .profiles
            .get(&account)
            .is_some_and(|old| old.revision != profile.revision)
        {
            inner.keys.remove(&account);
            inner.generation = inner.generation.wrapping_add(1);
        }
        inner.profiles.insert(account, profile);
        let key = scope(&context.identity, &context.project);
        inner.shared.insert(key.clone(), shared);
        inner.secrets.insert(key, secrets);
        Ok(())
    })
}
pub async fn prepare(
    app: &tauri::AppHandle,
    projects: &State<'_, ProjectState>,
    handle: &str,
) -> Result<ValueState, String> {
    if let Some(context) = context(projects, handle, false).await? {
        refresh(app, &context).await?;
    }
    Ok(app.state::<ValueState>().inner().clone())
}
pub fn adapter(state: &ValueState, root: &Path) -> Result<ScopedValues<OsValues>, String> {
    let values = ScopedValues::for_root(OsValues, Some(root))?;
    if let Some(identity) = store::server_owner(root)? {
        let project = store::load_project(root)?.0.id;
        let definitions = environments::definitions(root)?.0;
        Ok(values.with_remote(state.access(identity, project, definitions)))
    } else {
        Ok(values)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    configured: bool,
    unlocked: bool,
}
#[tauri::command]
pub async fn server_vault_action(
    app: tauri::AppHandle,
    action: String,
    password: Option<String>,
) -> Result<VaultStatus, String> {
    let password = Zeroizing::new(password.unwrap_or_default());
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| super::SETTINGS_ERROR)?;
    let session = super::session_for(&root)?;
    let scoped = super::account_root(&root, &session.identity)?;
    let state = app.state::<ValueState>().inner().clone();
    if action == "lock" {
        state.lock(&session.identity)?;
    } else if action != "status"
        && action != "unlock"
        && action != "create"
        && action != "changePassword"
    {
        return Err("Unknown vault action.".into());
    }
    let _key_operation = if matches!(action.as_str(), "unlock" | "create" | "changePassword") {
        Some(state.reserve_key_operation()?)
    } else {
        None
    };
    let generation = state.with(|inner| Ok(inner.generation))?;
    let remote = get::<AccountProfile>(&session, "/api/account/vault").await?;
    super::ensure_session(&session)?;
    let profile_path = scoped.join("vault-profile.json");
    let profile = match remote {
        Some(value) => {
            save(&profile_path, &value)?;
            value
        }
        None if action == "status" || action == "unlock" || action == "lock" => {
            cached(&profile_path)?
        }
        None => return Err("The server must be online to configure or change a vault.".into()),
    };
    if action == "unlock" || action == "create" || action == "changePassword" {
        let owner = session.identity.clone();
        let previous = profile.clone();
        let action_worker = action.clone();
        let key = if action == "changePassword" {
            Some(state.key(&owner)?)
        } else {
            None
        };
        let (new_profile, data_key) = tauri::async_runtime::spawn_blocking(move || {
            match action_worker.as_str() {
                "unlock" => {
                    let encrypted = previous.profile.as_ref().ok_or("Create your account vault first.")?;
                    Ok((None, crypto::unlock(&owner, &password, encrypted)?))
                }
                "create" if previous.profile.is_none() => { let (profile, key) = crypto::create(&owner, &password)?; Ok((Some(profile), key)) }
                "changePassword" => { let key = key.ok_or("Unlock the vault first.")?; Ok((Some(crypto::rewrap(&owner, &password, &key)?), key)) }
                _ => Err("A vault already exists. Unlock it; creating another key would lose existing secrets.".to_string()),
            }
        }).await.map_err(|_| "Vault key operation failed.".to_string())??;
        let updated = if let Some(new_profile) = new_profile {
            put::<_, AccountProfile>(
                &session,
                "/api/account/vault",
                profile.revision,
                &serde_json::json!({"profile": new_profile}),
            )
            .await?
        } else {
            profile.clone()
        };
        super::ensure_session(&session)?;
        save(&profile_path, &updated)?;
        state.install_key(&session.identity, updated, data_key, generation)?;
    } else {
        state.with(|inner| {
            if inner
                .profiles
                .get(&session.identity.namespace())
                .is_some_and(|old| old.revision != profile.revision)
            {
                inner.keys.remove(&session.identity.namespace());
                inner.generation = inner.generation.wrapping_add(1);
            }
            inner.profiles.insert(session.identity.namespace(), profile);
            Ok(())
        })?;
    }
    state.with(|inner| {
        Ok(VaultStatus {
            configured: inner
                .profiles
                .get(&session.identity.namespace())
                .is_some_and(|profile| profile.profile.is_some()),
            unlocked: inner.keys.contains_key(&session.identity.namespace()),
        })
    })
}

pub async fn set_value(
    app: &tauri::AppHandle,
    context: &Context,
    variable: &str,
    environment: Option<&str>,
    value: Option<String>,
    expected: &str,
    value_revision: Option<i64>,
) -> Result<(), String> {
    let value = value.map(Zeroizing::new);
    if expected != context.definitions_revision {
        return Err("Variable definitions changed. Reload variables before saving.".into());
    }
    let variable = context
        .definitions
        .variables
        .iter()
        .find(|entry| entry.id == variable)
        .ok_or("Variable no longer exists.")?;
    let environment = environment.unwrap_or("default");
    if environment != "default"
        && !context
            .definitions
            .environments
            .iter()
            .any(|entry| entry.id == environment)
    {
        return Err("Environment no longer exists.".into());
    }
    if value.as_ref().is_some_and(|value| value.len() > 2560) {
        return Err("Values must not exceed 2,560 UTF-8 bytes.".into());
    }
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| super::SETTINGS_ERROR)?;
    let session = super::session_for(&root)?;
    if session.identity != context.identity {
        return Err("The server account changed.".into());
    }
    let state = app.state::<ValueState>().inner().clone();
    let scoped = super::account_root(&root, &context.identity)?;
    let (shared_path, secret_path) = bundle_paths(&scoped, &context.project);
    let key = scope(&context.identity, &context.project);
    if variable.secret.unwrap_or(true) {
        let mut bundle = state.with(|inner| {
            inner
                .secrets
                .get(&key)
                .cloned()
                .ok_or_else(|| "Load server values before saving.".into())
        })?;
        if value_revision != Some(bundle.revision) {
            return Err("Synchronized values changed. Reload variables before saving; your input is still open.".into());
        }
        bundle.values.retain(|entry| {
            entry.variable_id != variable.id || entry.environment_id != environment
        });
        if let Some(value) = value {
            bundle.values.push(crypto::encrypt(
                &context.identity,
                &context.project,
                &variable.id,
                environment,
                &value,
                &state.key(&context.identity)?,
            )?);
        }
        let saved: Bundle<CipherValue> = put_with(&session, &format!("/api/projects/{}/vault-values", context.project), bundle.revision, &serde_json::json!({"projectRevision": bundle.project_revision, "values": bundle.values}), Some(UNSYNCED_DEFINITIONS)).await?;
        super::ensure_session(&session)?;
        state.with(|inner| {
            inner.secrets.insert(key, saved.clone());
            Ok(())
        })?;
        save(&secret_path, &saved)?;
    } else {
        let mut bundle = state.with(|inner| {
            inner
                .shared
                .get(&key)
                .cloned()
                .ok_or_else(|| "Load server values before saving.".into())
        })?;
        if value_revision != Some(bundle.revision) {
            return Err("Synchronized values changed. Reload variables before saving; your input is still open.".into());
        }
        bundle.values.retain(|entry| {
            entry.variable_id != variable.id || entry.environment_id != environment
        });
        if let Some(value) = value {
            bundle.values.push(SharedValue {
                variable_id: variable.id.clone(),
                environment_id: environment.into(),
                value: value.to_string(),
            });
        }
        let saved: Bundle<SharedValue> = put_with(&session, &format!("/api/projects/{}/values", context.project), bundle.revision, &serde_json::json!({"projectRevision": bundle.project_revision, "values": bundle.values}), Some(UNSYNCED_DEFINITIONS)).await?;
        super::ensure_session(&session)?;
        state.with(|inner| {
            inner.shared.insert(key, saved.clone());
            Ok(())
        })?;
        save(&shared_path, &saved)?;
    }
    Ok(())
}

/// One imported value slot: variable ID, environment ID (`default` for the default slot), value.
pub type ImportSlot = (String, String, Zeroizing<String>);

/// Adds imported values to the server bundles after the definitions were synced. Ordinary
/// values join the shared bundle; secret values are encrypted into the caller's vault bundle.
/// A slot that already holds the same value is skipped, so a retry does not duplicate work.
/// A different shared value or any existing secret slot is never overwritten.
pub async fn import_values(
    app: &tauri::AppHandle,
    context: &Context,
    slots: Vec<ImportSlot>,
) -> Result<(), String> {
    refresh(app, context).await?;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| super::SETTINGS_ERROR)?;
    let session = super::session_for(&root)?;
    if session.identity != context.identity {
        return Err("The server account changed.".into());
    }
    let state = app.state::<ValueState>().inner().clone();
    let scoped = super::account_root(&root, &context.identity)?;
    let (shared_path, secret_path) = bundle_paths(&scoped, &context.project);
    let key = scope(&context.identity, &context.project);
    let (mut shared, mut secrets) = state.with(|inner| {
        Ok((
            inner
                .shared
                .get(&key)
                .cloned()
                .ok_or("Load server values before importing.")?,
            inner
                .secrets
                .get(&key)
                .cloned()
                .ok_or("Load server values before importing.")?,
        ))
    })?;
    let mut shared_changed = false;
    let mut secrets_changed = false;
    for (variable, environment, value) in slots {
        let definition = context
            .definitions
            .variables
            .iter()
            .find(|entry| entry.id == variable)
            .ok_or("An imported variable is missing from the workspace definitions.")?;
        if value.len() > 2560 {
            continue;
        }
        if definition.secret.unwrap_or(true) {
            if secrets
                .values
                .iter()
                .any(|entry| entry.variable_id == variable && entry.environment_id == environment)
            {
                continue;
            }
            secrets.values.push(crypto::encrypt(
                &context.identity,
                &context.project,
                &variable,
                &environment,
                &value,
                &state.key(&context.identity)?,
            )?);
            secrets_changed = true;
        } else {
            match shared
                .values
                .iter()
                .find(|entry| entry.variable_id == variable && entry.environment_id == environment)
            {
                Some(entry) if *entry.value == **value => {}
                Some(_) => return Err("An imported value was changed on the server meanwhile. It was not overwritten; check Project variables.".into()),
                None => {
                    shared.values.push(SharedValue {
                        variable_id: variable,
                        environment_id: environment,
                        value: value.to_string(),
                    });
                    shared_changed = true;
                }
            }
        }
    }
    if shared.values.len() > 1000 || secrets.values.len() > 1000 {
        return Err("The server value bundle would exceed 1,000 slots.".into());
    }
    if shared_changed {
        let saved: Bundle<SharedValue> = put_with(&session, &format!("/api/projects/{}/values", context.project), shared.revision, &serde_json::json!({"projectRevision": shared.project_revision, "values": shared.values}), Some(UNSYNCED_DEFINITIONS)).await?;
        super::ensure_session(&session)?;
        state.with(|inner| {
            inner.shared.insert(key.clone(), saved.clone());
            Ok(())
        })?;
        save(&shared_path, &saved)?;
    }
    if secrets_changed {
        let saved: Bundle<CipherValue> = put_with(&session, &format!("/api/projects/{}/vault-values", context.project), secrets.revision, &serde_json::json!({"projectRevision": secrets.project_revision, "values": secrets.values}), Some(UNSYNCED_DEFINITIONS)).await?;
        super::ensure_session(&session)?;
        state.with(|inner| {
            inner.secrets.insert(key, saved.clone());
            Ok(())
        })?;
        save(&secret_path, &saved)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (ValueState, ServerIdentity, String, Definitions) {
        let owner = ServerIdentity {
            server_url: "https://synthetic.example.test".into(),
            account_id: uuid::Uuid::new_v4(),
        };
        let project = uuid::Uuid::new_v4().to_string();
        let definitions = Definitions {
            schema_version: 1,
            environments: vec![],
            variables: vec![
                environments::Definition {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: "shared".into(),
                    secret: Some(false),
                },
                environments::Definition {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: "secret".into(),
                    secret: None,
                },
            ],
        };
        (ValueState::default(), owner, project, definitions)
    }
    #[test]
    fn ordinary_values_are_shared_data_but_legacy_secrets_require_unlock_and_never_fall_back_to_plaintext(
    ) {
        let (state, owner, project, definitions) = fixture();
        let key = Zeroizing::new([42; 32]);
        let encrypted = crypto::encrypt(
            &owner,
            &project,
            &definitions.variables[1].id,
            "default",
            "Synthetic private",
            &key,
        )
        .unwrap();
        state
            .with(|inner| {
                inner.shared.insert(
                    scope(&owner, &project),
                    Bundle {
                        revision: 1,
                        project_revision: 2,
                        values: vec![
                            SharedValue {
                                variable_id: definitions.variables[0].id.clone(),
                                environment_id: "default".into(),
                                value: "Shared synthetic".into(),
                            },
                            SharedValue {
                                variable_id: definitions.variables[1].id.clone(),
                                environment_id: "default".into(),
                                value: "Do not use this plaintext for a secret".into(),
                            },
                        ],
                    },
                );
                inner.secrets.insert(
                    scope(&owner, &project),
                    Bundle {
                        revision: 1,
                        project_revision: 2,
                        values: vec![encrypted],
                    },
                );
                Ok(())
            })
            .unwrap();
        let access = state.access(owner.clone(), project.clone(), definitions.clone());
        assert_eq!(
            access
                .get(&format!(
                    "{}/{}/default",
                    project, definitions.variables[0].id
                ))
                .unwrap()
                .as_deref(),
            Some("Shared synthetic")
        );
        let reference = format!("{}/{}/default", project, definitions.variables[1].id);
        assert!(access.get(&reference).unwrap_err().contains("Unlock"));
        state
            .with(|inner| {
                inner.keys.insert(owner.namespace(), key);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            access.get(&reference).unwrap().as_deref(),
            Some("Synthetic private")
        );
        state.lock(&owner).unwrap();
        assert!(access.get(&reference).is_err());
        assert!(access.check_removal(&reference).is_err());
        let other = ServerIdentity {
            account_id: uuid::Uuid::new_v4(),
            ..owner
        };
        assert!(state
            .access(other, project, definitions)
            .get(&reference)
            .is_err());
    }
    #[test]
    fn lock_and_account_switch_prevent_a_pending_unlock_from_restoring_a_stale_key() {
        let (state, owner, _, _) = fixture();
        let operation = state.reserve_key_operation().unwrap();
        assert!(state.reserve_key_operation().is_err());
        drop(operation);
        assert!(state.reserve_key_operation().is_ok());
        let generation = state.with(|inner| Ok(inner.generation)).unwrap();
        state.lock(&owner).unwrap();
        assert!(state
            .install_key(
                &owner,
                AccountProfile {
                    revision: 1,
                    profile: None
                },
                Zeroizing::new([1; 32]),
                generation
            )
            .is_err());
        let generation = state.with(|inner| Ok(inner.generation)).unwrap();
        state.clear();
        assert!(state
            .install_key(
                &owner,
                AccountProfile {
                    revision: 1,
                    profile: None
                },
                Zeroizing::new([1; 32]),
                generation
            )
            .is_err());
        assert!(state.key(&owner).is_err());
    }
    #[test]
    fn server_metadata_uses_the_same_kdf_and_ipc_field_names_as_the_backend() {
        let json = serde_json::json!({"revision":1,"profile":{"schemaVersion":1,"algorithm":"AES-256-GCM","kdf":{"algorithm":"Argon2id","version":19,"memoryKiB":65536,"iterations":3,"parallelism":1},"salt":"AAAAAAAAAAAAAAAAAAAAAA==","nonce":"AAAAAAAAAAAAAAAA","wrappedKey":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}});
        let profile: AccountProfile = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(profile).unwrap(), json);
        let status = serde_json::to_value(VaultStatus {
            configured: true,
            unlocked: true,
        })
        .unwrap();
        assert_eq!(status.as_object().unwrap().len(), 2);
        assert!(status.get("key").is_none());
    }
}

/// Explicit protocol integration uses a disposable loopback server, never a user's OS store.
#[cfg(test)]
mod protocol_tests {
    use super::*;
    // Compiled only for tests; production clients never consult fixture CA or DNS settings.
    pub(super) fn fixture_client(
        builder: reqwest::ClientBuilder,
    ) -> Result<reqwest::ClientBuilder, String> {
        if std::env::var_os("FLUXKIT_E2E_SESSION").is_none() {
            return Ok(builder);
        }
        let url = reqwest::Url::parse(
            &std::env::var("FLUXKIT_VALUE_TEST_URL").map_err(|_| "Missing E2E URL")?,
        )
        .map_err(|_| "Invalid E2E URL")?;
        if url.scheme() != "https"
            || url.host_str() != Some("fluxkit.test")
            || url.port().is_none_or(|port| port < 1024)
        {
            return Err(
                "E2E fixture must use its reserved HTTPS hostname and unprivileged port.".into(),
            );
        }
        let path = std::env::var("FLUXKIT_E2E_CA").map_err(|_| "Missing E2E CA")?;
        let pem = std::fs::read(path).map_err(|_| "Could not read E2E CA")?;
        if pem.len() > 64 * 1024 {
            return Err("Invalid E2E CA size.".into());
        }
        let certificate = reqwest::Certificate::from_pem(&pem).map_err(|_| "Invalid E2E CA")?;
        Ok(builder
            .add_root_certificate(certificate)
            .no_proxy()
            .resolve(
                "fluxkit.test",
                std::net::SocketAddr::from(([127, 0, 0, 1], url.port().ok_or("Missing E2E port")?)),
            ))
    }
    #[tokio::test]
    #[ignore = "explicit integration: requires FLUXKIT_VALUE_TEST_URL for a disposable local server"]
    async fn rust_crypto_and_value_protocol_roundtrip_through_the_real_backend() {
        let root = std::env::var("FLUXKIT_VALUE_TEST_URL").expect("isolated loopback server URL");
        let url = reqwest::Url::parse(&root).unwrap();
        let client = client().unwrap();
        let (token, member_token) = if let Ok(path) = std::env::var("FLUXKIT_E2E_SESSION") {
            assert_eq!(url.scheme(), "https");
            assert_eq!(url.host_str(), Some("fluxkit.test"));
            assert!(url.port().is_some_and(|port| port >= 1024));
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct FixtureSession {
                admin_token: String,
                member_token: String,
            }
            let bytes = std::fs::read(path).expect("synthetic E2E session file");
            assert!(bytes.len() <= 16 * 1024);
            let fixture: FixtureSession =
                serde_json::from_slice(&bytes).expect("synthetic E2E session format");
            (fixture.admin_token, Some(fixture.member_token))
        } else {
            assert_eq!(url.scheme(), "http");
            assert_eq!(url.host_str(), Some("127.0.0.1"));
            let login: serde_json::Value = client.post(format!("{root}/api/auth/login"))
                .json(&serde_json::json!({"username":"nativeintegration","password":"SyntheticAdmin42!password"}))
                .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
            (login["accessToken"].as_str().unwrap().to_string(), None)
        };
        let account: serde_json::Value = client
            .get(format!("{root}/api/account"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let identity = ServerIdentity {
            server_url: root.clone(),
            account_id: uuid::Uuid::parse_str(account["id"].as_str().unwrap()).unwrap(),
        };
        let session = super::super::Session {
            identity: identity.clone(),
            access_token: token.clone(),
        };
        let before: AccountProfile = get(&session, "/api/account/vault").await.unwrap().unwrap();
        assert!(
            before.profile.is_none(),
            "use a fresh disposable account for this integration"
        );
        let (profile, key) = crypto::create(&identity, "Synthetic master password 42").unwrap();
        let profile: AccountProfile = put(
            &session,
            "/api/account/vault",
            0,
            &serde_json::json!({"profile": profile}),
        )
        .await
        .unwrap();
        assert_eq!(profile.revision, 1);
        let created: serde_json::Value = client
            .post(format!("{root}/api/projects"))
            .bearer_auth(&token)
            .json(&serde_json::json!({"name":"Native protocol integration"}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        let project = created["id"].as_str().unwrap();
        let shared = uuid::Uuid::new_v4().to_string();
        let secret = uuid::Uuid::new_v4().to_string();
        let snapshot = serde_json::json!({"project":{"schemaVersion":1,"id":project,"name":"Native protocol integration"},"collections":[],"environments":{"schemaVersion":1,"environments":[],"variables":[{"id":shared,"name":"baseUrl","secret":false},{"id":secret,"name":"apiToken"}]}}).to_string();
        let _: serde_json::Value = put(
            &session,
            &format!("/api/projects/{project}"),
            1,
            &serde_json::json!({"snapshot":snapshot}),
        )
        .await
        .unwrap();
        let shared_values = vec![SharedValue {
            variable_id: shared,
            environment_id: "default".into(),
            value: "Shared synthetic value".into(),
        }];
        let saved: Bundle<SharedValue> = put(
            &session,
            &format!("/api/projects/{project}/values"),
            0,
            &serde_json::json!({"projectRevision":2,"values":shared_values}),
        )
        .await
        .unwrap();
        assert_eq!(saved.revision, 1);
        let encrypted = crypto::encrypt(
            &identity,
            project,
            &secret,
            "default",
            "Native encrypted synthetic secret",
            &key,
        )
        .unwrap();
        let saved: Bundle<CipherValue> = put(
            &session,
            &format!("/api/projects/{project}/vault-values"),
            0,
            &serde_json::json!({"projectRevision":2,"values":[encrypted]}),
        )
        .await
        .unwrap();
        assert_eq!(
            crypto::decrypt(&identity, project, &saved.values[0], &key)
                .unwrap()
                .as_str(),
            "Native encrypted synthetic secret"
        );
        let wrapper = crypto::rewrap(&identity, "New synthetic master password 42", &key).unwrap();
        let updated: AccountProfile = put(
            &session,
            "/api/account/vault",
            1,
            &serde_json::json!({"profile":wrapper}),
        )
        .await
        .unwrap();
        let profile = updated.profile.unwrap();
        assert!(crypto::unlock(&identity, "Synthetic master password 42", &profile).is_err());
        let reopened =
            crypto::unlock(&identity, "New synthetic master password 42", &profile).unwrap();
        let downloaded: Bundle<CipherValue> =
            get(&session, &format!("/api/projects/{project}/vault-values"))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(
            crypto::decrypt(&identity, project, &downloaded.values[0], &reopened)
                .unwrap()
                .as_str(),
            "Native encrypted synthetic secret"
        );
        if let Some(member_token) = member_token {
            let member: serde_json::Value = client
                .get(format!("{root}/api/account"))
                .bearer_auth(&member_token)
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap();
            client
                .post(format!("{root}/api/projects/{project}/members"))
                .bearer_auth(&token)
                .json(&serde_json::json!({"username": member["userName"], "role": "member"}))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap();
            let peer = super::super::Session {
                identity: ServerIdentity {
                    server_url: root.clone(),
                    account_id: uuid::Uuid::parse_str(member["id"].as_str().unwrap()).unwrap(),
                },
                access_token: member_token,
            };
            let shared: Bundle<SharedValue> =
                get(&peer, &format!("/api/projects/{project}/values"))
                    .await
                    .unwrap()
                    .unwrap();
            assert_eq!(shared.values.len(), 1);
            let private: Bundle<CipherValue> =
                get(&peer, &format!("/api/projects/{project}/vault-values"))
                    .await
                    .unwrap()
                    .unwrap();
            assert!(private.values.is_empty());
            let profile: AccountProfile = get(&peer, "/api/account/vault").await.unwrap().unwrap();
            assert!(profile.profile.is_none());
        }
        let snapshot: serde_json::Value = get(&session, &format!("/api/projects/{project}"))
            .await
            .unwrap()
            .unwrap();
        assert!(!snapshot["snapshot"]
            .as_str()
            .unwrap()
            .contains("synthetic value"));
        assert!(!snapshot["snapshot"]
            .as_str()
            .unwrap()
            .contains("synthetic secret"));
        assert!(!serde_json::to_string(&downloaded)
            .unwrap()
            .contains("Native encrypted synthetic secret"));
    }
}
