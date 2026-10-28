use super::model::{validate_id, validate_name, Attachment, Collection, Project, SCHEMA_VERSION};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use uuid::Uuid;

const PROJECT_FILE: &str = "project.fluxkit.json";
const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;
const IO_ERROR: &str =
    "Could not access project files. Check permissions and available disk space.";
const CONFLICT: &str =
    "Files changed outside Fluxkit. Reload the project before saving; your edits are still open.";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionView {
    pub revision: String,
    pub data: Collection,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub handle: String,
    pub directory: String,
    pub revision: String,
    pub read_only: bool,
    pub server_sync: bool,
    pub server_owned: bool,
    pub server_recovery: bool,
    pub data: Project,
    pub collections: Vec<CollectionView>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ServerWorkspaceMarker {
    pub server_url: String,
    #[serde(default)]
    pub account_id: Option<Uuid>,
    pub server_project_id: Uuid,
    pub revision: i64,
}

#[derive(Default)]
pub struct ProjectStore {
    roots: BTreeMap<String, (PathBuf, String, bool)>,
    server_identity: Option<crate::server::ServerIdentity>,
    server_identity_loaded: bool,
}

fn io<T>(result: std::io::Result<T>) -> Result<T, String> {
    result.map_err(|_| IO_ERROR.into())
}

pub(crate) fn safe(path: &Path, directory: bool) -> Result<(), String> {
    let metadata = io(fs::symlink_metadata(path))?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(
            "Project paths must be regular files and directories, without symbolic links.".into(),
        );
    }
    Ok(())
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    if !path.exists() {
        io(fs::create_dir(path))?;
    }
    safe(path, true)
}

pub(crate) fn lock(root: &Path) -> Result<File, String> {
    safe(root, true)?;
    let private = root.join(".fluxkit");
    ensure_dir(&private)?;
    let path = private.join("write.lock");
    if path.symlink_metadata().is_ok() {
        safe(&path, false)?;
    }
    let file = io(OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path))?;
    file.try_lock()
        .map_err(|_| "Another Fluxkit operation is using this project. Try again.".to_string())?;
    Ok(file)
}

pub(crate) fn read(path: &Path) -> Result<Vec<u8>, String> {
    safe(path, false)?;
    let file = io(File::open(path))?;
    if io(file.metadata())?.len() > MAX_JSON_BYTES {
        return Err("Project JSON files must not exceed 8 MiB.".into());
    }
    let mut bytes = Vec::new();
    io(file.take(MAX_JSON_BYTES + 1).read_to_end(&mut bytes))?;
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err("Project JSON file is too large.".into());
    }
    Ok(bytes)
}

pub(crate) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|_| {
        "Invalid JSON or unsupported fields in a project file. No files were changed.".into()
    })
}

pub(crate) fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn encode(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|_| "Could not serialize project data.".to_string())?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_JSON_BYTES {
        return Err("A collection JSON file must not exceed 8 MiB.".into());
    }
    Ok(bytes)
}

pub(crate) fn atomic(path: &Path, bytes: &[u8], new: bool) -> Result<(), String> {
    let parent = path.parent().ok_or(IO_ERROR)?;
    safe(parent, true)?;
    if path.symlink_metadata().is_ok() {
        safe(path, false)?;
    }
    let mut temp = io(NamedTempFile::new_in(parent))?;
    io(temp.write_all(bytes))?;
    io(temp.as_file().sync_all())?;
    if new {
        temp.persist_noclobber(path).map_err(|_| {
            "A file already exists or could not be created. Nothing was overwritten.".to_string()
        })?;
    } else {
        temp.persist(path).map_err(|_| IO_ERROR.to_string())?;
    }
    // Directory synchronization strengthens crash durability where the platform supports it.
    #[cfg(unix)]
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

pub(crate) fn replace(
    root: &Path,
    path: &Path,
    expected: &str,
    bytes: &[u8],
    backup_name: &str,
) -> Result<(), String> {
    let previous = read(path)?;
    if revision(&previous) != expected {
        return Err(CONFLICT.into());
    }
    let backups = root.join(".fluxkit/backups");
    ensure_dir(&backups)?;
    atomic(&backups.join(backup_name), &previous, false)?;
    // Editors need not honor our advisory lock. Recheck immediately before replacement.
    if read(path)? != previous {
        return Err(CONFLICT.into());
    }
    atomic(path, bytes, false)
}

pub(crate) fn server_owner(root: &Path) -> Result<Option<crate::server::ServerIdentity>, String> {
    for name in ["server-workspace.json", "server-cache.json"] {
        let path = root.join(".fluxkit").join(name);
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                safe(&path, false)?;
                let marker: crate::server::ServerCacheMarker = decode(&read(&path)?)?;
                let account_id = marker.account_id.ok_or("This legacy server cache has no account owner. Sign in and reopen it from Server connection. Its files were preserved.")?;
                return Ok(Some(crate::server::ServerIdentity {
                    server_url: marker.server_url,
                    account_id,
                }));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(IO_ERROR.into()),
        }
    }
    Ok(None)
}

/// Local history and environment selection are independent for each server account.
pub(crate) fn local_state_id(root: &Path, project: &str) -> Result<String, String> {
    match server_owner(root)? {
        Some(identity) => {
            let digest = Sha256::digest(format!("{}/{project}", identity.namespace()));
            let mut bytes = [0; 16];
            bytes.copy_from_slice(&digest[..16]);
            Ok(Uuid::from_bytes(bytes).to_string())
        }
        None => Ok(project.to_string()),
    }
}

impl ProjectStore {
    pub(crate) fn set_server_identity(&mut self, identity: Option<crate::server::ServerIdentity>) {
        self.server_identity = identity;
        self.server_identity_loaded = true;
    }
    pub(crate) fn require_server_identity(
        &self,
        identity: &crate::server::ServerIdentity,
    ) -> Result<(), String> {
        if self.server_identity.as_ref() != Some(identity) {
            return Err("This server project belongs to another account or session. Sign in with its account and reopen it.".into());
        }
        Ok(())
    }

    pub fn snapshot(&self, handle: &str) -> Result<(String, String, String), String> {
        let root = self.root(handle)?;
        let _lock = lock(&root)?;
        let view = load(&root, handle)?;
        let project_id = view.data.id.clone();
        let name = view.data.name.clone();
        if view.collections.iter().any(|collection| {
            collection.data.requests.iter().any(|request| {
                request.body.file.is_some()
                    || request.body.fields.iter().any(|field| field.file.is_some())
            })
        }) {
            return Err(
                "Projects with local file attachments cannot be uploaded to a server yet.".into(),
            );
        }
        let (environments, _) = super::environments::definitions(&root)?;
        let value = serde_json::json!({
            "project": view.data,
            "collections": view.collections.into_iter().map(|collection| collection.data).collect::<Vec<_>>(),
            "environments": environments,
        });
        let snapshot = serde_json::to_string(&value)
            .map_err(|_| "Could not serialize the project snapshot.".to_string())?;
        if snapshot.len() > 32 * 1024 * 1024 {
            return Err("Project snapshot exceeds the 32 MiB server limit.".into());
        }
        Ok((project_id, name, snapshot))
    }

    pub(crate) fn server_workspace_marker(
        &self,
        handle: &str,
    ) -> Result<ServerWorkspaceMarker, String> {
        let root = self.ensure_writable(handle)?;
        let path = root.join(".fluxkit/server-workspace.json");
        safe(&path, false)?;
        let marker: ServerWorkspaceMarker = decode(&read(&path)?)?;
        if marker.revision < 1 || marker.server_project_id.to_string() != load_project(&root)?.0.id
        {
            return Err("Server workspace metadata does not match the local project.".into());
        }
        Ok(marker)
    }

    /// Authorizes the project handle and holds its advisory lock for local value operations.
    pub fn with_environment<T>(
        &self,
        handle: &str,
        operation: impl FnOnce(&Path, &str) -> Result<T, String>,
    ) -> Result<T, String> {
        let root = self.root(handle)?;
        let _lock = lock(&root)?;
        let (project, _) = load_project(&root)?;
        operation(&root, &project.id)
    }
    pub fn create(&mut self, selected: &Path, name: String) -> Result<ProjectView, String> {
        validate_name(&name)?;
        let root = io(selected.canonicalize())?;
        safe(&root, true)?;
        let entries = io(fs::read_dir(&root))?;
        for entry in entries {
            if io(entry)?.file_name() != ".DS_Store" {
                return Err("Choose an empty folder for a new project.".into());
            }
        }
        let _lock = lock(&root)?;
        ensure_dir(&root.join("collections"))?;
        let project = Project {
            schema_version: SCHEMA_VERSION,
            id: Uuid::new_v4().to_string(),
            name,
            auth: None,
        };
        atomic(&root.join(PROJECT_FILE), &encode(&project)?, true)?;
        self.register(root, false, false)
    }

    pub fn open(&mut self, selected: &Path) -> Result<ProjectView, String> {
        // A selected root can itself be a link; authorize its canonical target once.
        let root = io(selected.canonicalize())?;
        let _lock = lock(&root)?;
        let marker = root.join(".fluxkit/server-cache.json");
        let workspace_marker = root.join(".fluxkit/server-workspace.json");
        let recovery = root.join(".fluxkit/server-recovery");
        if recovery.exists() {
            safe(&recovery, false)?;
        }
        let read_only = match fs::symlink_metadata(&marker) {
            Ok(_) => {
                safe(&marker, false)?;
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => return Err(IO_ERROR.into()),
        };
        let server_sync = match fs::symlink_metadata(&workspace_marker) {
            Ok(_) => {
                safe(&workspace_marker, false)?;
                let marker: ServerWorkspaceMarker = decode(&read(&workspace_marker)?)?;
                if marker.revision < 1
                    || marker.server_project_id.to_string() != load_project(&root)?.0.id
                {
                    return Err(
                        "Server workspace metadata does not match the local project.".into(),
                    );
                }
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => return Err(IO_ERROR.into()),
        };
        self.register(
            root,
            read_only || server_sync || recovery.exists(),
            server_sync,
        )
    }

    pub(crate) fn open_server_workspace(&mut self, selected: &Path) -> Result<ProjectView, String> {
        let root = io(selected.canonicalize())?;
        let _lock = lock(&root)?;
        let marker = root.join(".fluxkit/server-workspace.json");
        safe(&marker, false)?;
        let workspace: ServerWorkspaceMarker = decode(&read(&marker)?)?;
        if workspace.revision < 1
            || workspace.server_project_id.to_string() != load_project(&root)?.0.id
        {
            return Err("Server workspace metadata does not match the local project.".into());
        }
        if root.join(".fluxkit/server-recovery").exists() {
            return Err("This workspace is a retained recovery copy. Open the current server workspace instead.".into());
        }
        self.register(root, false, true)
    }

    fn register(
        &mut self,
        root: PathBuf,
        read_only: bool,
        server_sync: bool,
    ) -> Result<ProjectView, String> {
        if let Some(identity) = server_owner(&root)? {
            if !self.server_identity_loaded {
                self.set_server_identity(crate::server::stored_identity()?);
            }
            self.require_server_identity(&identity)?;
        }
        let (project, _) = load_project(&root)?;
        let handle = self
            .roots
            .iter()
            .find(|(_, (path, id, _))| *path == root && *id == project.id)
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut view = load(&root, &handle)?;
        view.server_owned = server_owner(&root)?.is_some();
        view.read_only = read_only;
        view.server_sync = server_sync && !read_only;
        self.roots.insert(handle, (root, project.id, read_only));
        Ok(view)
    }

    fn root(&self, handle: &str) -> Result<PathBuf, String> {
        let (root, project_id, _) = self
            .roots
            .get(handle)
            .ok_or("Project is not open. Open it with the folder chooser first.")?;
        safe(root, true)?;
        if let Some(identity) = server_owner(root)? {
            self.require_server_identity(&identity)?;
        }
        if load_project(root)?.0.id != *project_id {
            return Err(
                "The project identity changed on disk. Close and reopen the project.".into(),
            );
        }
        Ok(root.clone())
    }

    pub fn ensure_writable(&self, handle: &str) -> Result<PathBuf, String> {
        let root = self.root(handle)?;
        if self
            .roots
            .get(handle)
            .is_some_and(|(_, _, read_only)| *read_only)
        {
            return Err("This server project is read-only in the local snapshot.".into());
        }
        Ok(root)
    }

    pub fn reload(&self, handle: &str) -> Result<ProjectView, String> {
        let root = self.root(handle)?;
        let _lock = lock(&root)?;
        let mut view = load(&root, handle)?;
        view.server_owned = server_owner(&root)?.is_some();
        view.read_only = self
            .roots
            .get(handle)
            .is_some_and(|(_, _, read_only)| *read_only);
        view.server_sync = self.server_workspace_marker(handle).is_ok() && !view.read_only;
        Ok(view)
    }

    pub fn rename(
        &self,
        handle: &str,
        name: String,
        expected: &str,
    ) -> Result<ProjectView, String> {
        validate_name(&name)?;
        let root = self.ensure_writable(handle)?;
        let _lock = lock(&root)?;
        let mut view = load(&root, handle)?;
        view.data.name = name;
        replace(
            &root,
            &root.join(PROJECT_FILE),
            expected,
            &encode(&view.data)?,
            "project.json",
        )?;
        load(&root, handle)
    }

    pub fn save_auth(
        &self,
        handle: &str,
        auth: Option<super::model::Auth>,
        expected: &str,
    ) -> Result<ProjectView, String> {
        super::model::validate_auth(auth.as_ref())?;
        let root = self.ensure_writable(handle)?;
        let _lock = lock(&root)?;
        let mut view = load(&root, handle)?;
        view.data.auth = auth;
        replace(
            &root,
            &root.join(PROJECT_FILE),
            expected,
            &encode(&view.data)?,
            "project.json",
        )?;
        load(&root, handle)
    }

    pub fn save_collection(
        &self,
        handle: &str,
        collection: Collection,
        expected: Option<String>,
    ) -> Result<CollectionView, String> {
        collection.validate()?;
        let root = self.ensure_writable(handle)?;
        let _lock = lock(&root)?;
        let current = load(&root, handle)?;
        let dir = root.join("collections");
        safe(&dir, true)?;
        let path = dir.join(format!("{}.json", collection.id));
        let bytes = encode(&collection)?;
        let mut total = bytes.len();
        for saved in &current.collections {
            if saved.data.id != collection.id {
                total += encode(&saved.data)?.len();
            }
        }
        if total > 32 * 1024 * 1024 || (expected.is_none() && current.collections.len() >= 1_000) {
            return Err("Project exceeds its collection count or 32 MiB loading limit.".into());
        }
        if let Some(expected) = expected {
            replace(
                &root,
                &path,
                &expected,
                &bytes,
                &format!("{}.json", collection.id),
            )?;
        } else {
            atomic(&path, &bytes, true)?;
        }
        Ok(CollectionView {
            revision: revision(&bytes),
            data: collection,
        })
    }

    pub fn remove_collection(&self, handle: &str, id: &str, expected: &str) -> Result<(), String> {
        validate_id(id)?;
        let root = self.ensure_writable(handle)?;
        let _lock = lock(&root)?;
        safe(&root.join("collections"), true)?;
        let path = root.join(format!("collections/{id}.json"));
        if revision(&read(&path)?) != expected {
            return Err(CONFLICT.into());
        }
        let trash = root.join(".fluxkit/trash");
        ensure_dir(&trash)?;
        io(fs::rename(
            path,
            trash.join(format!("{id}-{}.json", Uuid::new_v4())),
        ))
    }

    pub fn attachment(&self, handle: &str, selected: &Path) -> Result<Attachment, String> {
        let root = self.ensure_writable(handle)?;
        let _lock = lock(&root)?;
        load_project(&root)?;
        let dir = root.join("attachments");
        ensure_dir(&dir)?;
        let mut source = io(File::open(selected))?;
        if !io(source.metadata())?.is_file() {
            return Err("Choose a regular file.".into());
        }
        let name = selected
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("Invalid file name.")?
            .to_string();
        validate_name(&name)?;
        let relative = format!("attachments/{}.bin", Uuid::new_v4());
        let mut temp = io(NamedTempFile::new_in(&dir))?;
        io(std::io::copy(&mut source, &mut temp))?;
        io(temp.as_file().sync_all())?;
        temp.persist_noclobber(root.join(&relative))
            .map_err(|_| IO_ERROR.to_string())?;
        Ok(Attachment {
            path: relative,
            name,
        })
    }

    pub fn close(&mut self, handle: &str) {
        self.roots.remove(handle);
    }
}

pub(crate) fn load_project(root: &Path) -> Result<(Project, Vec<u8>), String> {
    safe(root, true)?;
    let bytes = read(&root.join(PROJECT_FILE))?;
    let project: Project = decode(&bytes)?;
    project.validate()?;
    Ok((project, bytes))
}

pub(crate) fn load(root: &Path, handle: &str) -> Result<ProjectView, String> {
    let (project, bytes) = load_project(root)?;
    let directory = root.join("collections");
    safe(&directory, true)?;
    let mut collections = Vec::new();
    let mut paths: Vec<_> = io(fs::read_dir(&directory))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .map_err(|_| IO_ERROR)?;
    paths.sort();
    let mut total_bytes = 0usize;
    for path in paths {
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let bytes = read(&path)?;
        total_bytes += bytes.len();
        if total_bytes > 32 * 1024 * 1024 {
            return Err("Project collection data exceeds the 32 MiB loading limit.".into());
        }
        let collection: Collection = decode(&bytes)?;
        collection.validate()?;
        if path.file_stem().and_then(|s| s.to_str()) != Some(&collection.id) {
            return Err("Collection filename does not match its identifier.".into());
        }
        collections.push(CollectionView {
            revision: revision(&bytes),
            data: collection,
        });
        if collections.len() > 1_000 {
            return Err("A project can contain at most 1,000 collections.".into());
        }
    }
    Ok(ProjectView {
        handle: handle.into(),
        directory: root.to_string_lossy().into(),
        revision: revision(&bytes),
        read_only: false,
        server_sync: false,
        server_owned: server_owner(root)?.is_some(),
        server_recovery: root.join(".fluxkit/server-recovery").exists(),
        data: project,
        collections,
    })
}

/// Credential cleanup must not remove variables still referenced by persisted auth settings.
pub(super) fn check_auth_removal(root: &Path, removed: &[&str]) -> Result<(), String> {
    if removed.is_empty() {
        return Ok(());
    }
    let view = load(root, "credential-cleanup")?;
    let auth = view
        .data
        .auth
        .iter()
        .chain(view.collections.iter().flat_map(|c| {
            c.data
                .auth
                .iter()
                .chain(c.data.folders.iter().filter_map(|f| f.auth.as_ref()))
                .chain(c.data.requests.iter().filter_map(|r| r.auth.as_ref()))
        }));
    for configuration in auth {
        if configuration
            .variable_ids()
            .iter()
            .any(|id| removed.contains(id))
        {
            return Err("A variable is still referenced by saved authentication settings. Replace or remove those authentication references before deleting the variable. No values were deleted.".into());
        }
    }
    Ok(())
}
