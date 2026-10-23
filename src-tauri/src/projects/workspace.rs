//! Machine-local startup selection, separate from portable projects and credential values.
use super::{
    model::validate_id,
    store::{self, ProjectStore, ProjectView},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

const FILE: &str = "workspace.json";
const LIMIT: u64 = 16 * 1024;
const SETTINGS_ERROR: &str = "Could not access local workspace settings. Check app-data permissions and available disk space.";
const INVALID: &str = "Local workspace settings are invalid or use an unsupported version. They were preserved. Check workspace.json in Fluxkit's app-data folder.";
const REOPEN_ERROR: &str = "The last project could not be reopened. Its folder may have moved, be unavailable, or contain invalid project files. Use Open project to choose it again. No saved project files were changed.";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LastProject {
    directory: PathBuf,
    project_id: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Settings {
    schema_version: u32,
    last_project: Option<LastProject>,
}

fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(SETTINGS_ERROR.into()),
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(INVALID.into());
        }
        Ok(metadata) if metadata.len() > LIMIT => return Err(INVALID.into()),
        Ok(_) => {}
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| SETTINGS_ERROR)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SETTINGS_ERROR)?;
    if bytes.len() as u64 > LIMIT {
        return Err(INVALID.into());
    }
    Ok(Some(bytes))
}
fn decode(bytes: &[u8]) -> Result<Settings, String> {
    let settings: Settings = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
    if settings.schema_version != 1 {
        return Err(INVALID.into());
    }
    if let Some(last) = &settings.last_project {
        if !last.directory.is_absolute() || validate_id(&last.project_id).is_err() {
            return Err(INVALID.into());
        }
    }
    Ok(settings)
}

/// Restore only the previously selected project, using the ordinary project-file validation.
pub fn restore(
    settings_root: &Path,
    projects: &mut ProjectStore,
) -> Result<Option<ProjectView>, String> {
    let Some(bytes) = read(&settings_root.join(FILE))? else {
        return Ok(None);
    };
    let Some(last) = decode(&bytes)?.last_project else {
        return Ok(None);
    };
    let (project, _) = store::load_project(&last.directory).map_err(|_| REOPEN_ERROR)?;
    if project.id != last.project_id {
        return Err(REOPEN_ERROR.into());
    }
    let view = projects.open(&last.directory).map_err(|_| REOPEN_ERROR)?;
    if view.data.id != last.project_id {
        projects.close(&view.handle);
        return Err(REOPEN_ERROR.into());
    }
    Ok(Some(view))
}

/// Only native-issued open handles can become a startup target; null explicitly clears it.
pub fn remember(
    settings_root: &Path,
    projects: &ProjectStore,
    handle: Option<&str>,
) -> Result<(), String> {
    let last_project = handle
        .map(|handle| {
            projects.with_environment(handle, |root, id| {
                Ok(LastProject {
                    directory: root.to_path_buf(),
                    project_id: id.to_owned(),
                })
            })
        })
        .transpose()?;
    fs::create_dir_all(settings_root).map_err(|_| SETTINGS_ERROR)?;
    store::safe(settings_root, true).map_err(|_| SETTINGS_ERROR)?;
    let lock_path = settings_root.join("workspace.lock");
    if lock_path.symlink_metadata().is_ok() {
        store::safe(&lock_path, false).map_err(|_| SETTINGS_ERROR)?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)
        .map_err(|_| SETTINGS_ERROR)?;
    lock.try_lock()
        .map_err(|_| "Another Fluxkit instance is updating workspace settings. Try again.")?;
    let path = settings_root.join(FILE);
    let previous = read(&path)?;
    if let Some(bytes) = &previous {
        decode(bytes)?;
    }
    let settings = Settings {
        schema_version: 1,
        last_project,
    };
    let mut bytes = serde_json::to_vec_pretty(&settings).map_err(|_| SETTINGS_ERROR)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > LIMIT {
        return Err("The project path exceeds the workspace settings limit.".into());
    }
    if previous.as_ref() == Some(&bytes) {
        return Ok(());
    }
    if let Some(previous) = &previous {
        store::atomic(
            &settings_root.join("workspace.previous.json"),
            previous,
            false,
        )
        .map_err(|_| SETTINGS_ERROR)?;
    }
    if read(&path)? != previous {
        return Err(
            "Workspace settings changed outside Fluxkit. They were not overwritten.".into(),
        );
    }
    store::atomic(&path, &bytes, previous.is_none()).map_err(|_| SETTINGS_ERROR.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn restores_only_the_last_active_project_and_explicit_clear_stays_empty() {
        let local = tempdir().unwrap();
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let mut projects = ProjectStore::default();
        let a = projects.create(first.path(), "First α".into()).unwrap();
        let b = projects.create(second.path(), "Second".into()).unwrap();
        assert!(restore(local.path(), &mut ProjectStore::default())
            .unwrap()
            .is_none());
        remember(local.path(), &projects, Some(&a.handle)).unwrap();
        remember(local.path(), &projects, Some(&b.handle)).unwrap();
        let restored = restore(local.path(), &mut ProjectStore::default())
            .unwrap()
            .unwrap();
        assert_eq!(restored.data.id, b.data.id);
        assert_ne!(restored.handle, b.handle);
        let value = serde_json::to_value(&restored).unwrap();
        assert!(value.get("handle").is_some());
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(local.path().join(FILE)).unwrap()).unwrap();
        assert_eq!(saved["schemaVersion"], 1);
        assert_eq!(saved["lastProject"]["projectId"], b.data.id);
        assert_eq!(saved["lastProject"].as_object().unwrap().len(), 2);
        assert!(local.path().join("workspace.previous.json").is_file());
        remember(local.path(), &projects, None).unwrap();
        assert!(restore(local.path(), &mut ProjectStore::default())
            .unwrap()
            .is_none());
    }
    #[test]
    fn missing_or_replaced_projects_do_not_change_settings_or_create_projects() {
        let local = tempdir().unwrap();
        let directory = tempdir().unwrap();
        let mut projects = ProjectStore::default();
        let project = projects.create(directory.path(), "Test".into()).unwrap();
        remember(local.path(), &projects, Some(&project.handle)).unwrap();
        let before = fs::read(local.path().join(FILE)).unwrap();
        let path = directory.path().join("project.fluxkit.json");
        let original = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(restore(local.path(), &mut ProjectStore::default())
            .unwrap_err()
            .contains("Open project"));
        assert!(!path.exists());
        let mut changed: serde_json::Value = serde_json::from_slice(&original).unwrap();
        changed["id"] = uuid::Uuid::new_v4().to_string().into();
        fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(restore(local.path(), &mut ProjectStore::default()).is_err());
        assert_eq!(fs::read(local.path().join(FILE)).unwrap(), before);
    }
    #[test]
    fn invalid_settings_and_unknown_handles_are_never_overwritten() {
        let local = tempdir().unwrap();
        let path = local.path().join(FILE);
        for bytes in [b"broken".as_slice(), b"{\"schemaVersion\":2,\"lastProject\":null}", b"{\"schemaVersion\":1,\"lastProject\":{\"directory\":\"../escape\",\"projectId\":\"bad\"}}"] {
            fs::write(&path, bytes).unwrap();
            assert!(restore(local.path(), &mut ProjectStore::default()).is_err());
            assert!(remember(local.path(), &ProjectStore::default(), None).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        assert!(remember(local.path(), &ProjectStore::default(), Some("unknown")).is_err());
        fs::write(&path, vec![b'x'; LIMIT as usize + 1]).unwrap();
        assert!(restore(local.path(), &mut ProjectStore::default()).is_err());
    }
    #[test]
    fn unicode_paths_round_trip_and_failed_backups_preserve_the_current_settings() {
        let local = tempdir().unwrap();
        let parent = tempdir().unwrap();
        let directory = parent.path().join("Project Δ with spaces");
        fs::create_dir(&directory).unwrap();
        let mut projects = ProjectStore::default();
        let project = projects.create(&directory, "Synthetic".into()).unwrap();
        remember(local.path(), &projects, Some(&project.handle)).unwrap();
        let reopened = restore(local.path(), &mut ProjectStore::default())
            .unwrap()
            .unwrap();
        assert_eq!(reopened.data.id, project.data.id);
        let path = local.path().join(FILE);
        let previous = fs::read(&path).unwrap();
        fs::create_dir(local.path().join("workspace.previous.json")).unwrap();
        assert!(remember(local.path(), &projects, None).is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);
    }
    #[cfg(unix)]
    #[test]
    fn refuses_symlink_settings_without_touching_the_target() {
        let local = tempdir().unwrap();
        let target = local.path().join("target.json");
        fs::write(&target, b"preserve").unwrap();
        std::os::unix::fs::symlink(&target, local.path().join(FILE)).unwrap();
        assert!(restore(local.path(), &mut ProjectStore::default()).is_err());
        assert!(remember(local.path(), &ProjectStore::default(), None).is_err());
        assert_eq!(fs::read(target).unwrap(), b"preserve");
    }
}
