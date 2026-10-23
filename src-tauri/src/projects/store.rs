use super::model::{validate_id, validate_name, Attachment, Collection, Project, SCHEMA_VERSION};
use serde::{de::DeserializeOwned, Serialize};
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
    pub data: Project,
    pub collections: Vec<CollectionView>,
}

#[derive(Default)]
pub struct ProjectStore {
    roots: BTreeMap<String, (PathBuf, String)>,
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

fn lock(root: &Path) -> Result<File, String> {
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

impl ProjectStore {
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
        self.register(root)
    }

    pub fn open(&mut self, selected: &Path) -> Result<ProjectView, String> {
        // A selected root can itself be a link; authorize its canonical target once.
        let root = io(selected.canonicalize())?;
        let _lock = lock(&root)?;
        self.register(root)
    }

    fn register(&mut self, root: PathBuf) -> Result<ProjectView, String> {
        let (project, _) = load_project(&root)?;
        let handle = self
            .roots
            .iter()
            .find(|(_, (path, id))| *path == root && *id == project.id)
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let view = load(&root, &handle)?;
        self.roots.insert(handle, (root, project.id));
        Ok(view)
    }

    fn root(&self, handle: &str) -> Result<PathBuf, String> {
        let (root, project_id) = self
            .roots
            .get(handle)
            .ok_or("Project is not open. Open it with the folder chooser first.")?;
        safe(root, true)?;
        if load_project(root)?.0.id != *project_id {
            return Err(
                "The project identity changed on disk. Close and reopen the project.".into(),
            );
        }
        Ok(root.clone())
    }

    pub fn reload(&self, handle: &str) -> Result<ProjectView, String> {
        let root = self.root(handle)?;
        let _lock = lock(&root)?;
        load(&root, handle)
    }

    pub fn rename(
        &self,
        handle: &str,
        name: String,
        expected: &str,
    ) -> Result<ProjectView, String> {
        validate_name(&name)?;
        let root = self.root(handle)?;
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
        let root = self.root(handle)?;
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
        let root = self.root(handle)?;
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
        let root = self.root(handle)?;
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
        let root = self.root(handle)?;
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
