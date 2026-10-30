//! Machine-local session: open projects, open request tabs (including unsaved edits) and the
//! request/response layout, restored at the next start. Separate from portable project files.
//!
//! Tab contents are the editor's own data and may contain sensitive literal request content.
//! They are stored only in this file in the app-data folder (owner-only permissions on Unix),
//! never in project files, and never contain resolved variable values.
use super::{
    model::validate_id,
    store::{self, ProjectStore, ProjectView},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

const FILE: &str = "session.json";
const LIMIT: u64 = 16 * 1024 * 1024;
const MAX_PROJECTS: usize = 50;
const MAX_TABS: usize = 200;
/// Collection and folder ids whose tree branch was expanded; older entries beyond this are dropped.
const MAX_EXPANDED: usize = 10_000;
const SETTINGS_ERROR: &str =
    "Could not save the open tabs for the next start. Check app-data permissions and disk space.";
const INVALID: &str =
    "The saved session is invalid or uses an unsupported version. It was ignored.";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredProject {
    directory: PathBuf,
    project_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredTab {
    /// Index into `projects`; none for a tab that belongs to no project.
    project: Option<usize>,
    draft: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Stored {
    schema_version: u32,
    projects: Vec<StoredProject>,
    active_project: Option<usize>,
    tabs: Vec<StoredTab>,
    active_tab: Option<usize>,
    request_pane_ratio: Option<f64>,
    /// Expanded collections and folders; none in sessions saved before this was remembered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tree_expanded: Option<Vec<String>>,
}

/// What the UI hands over: open handles stand for projects.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionInput {
    projects: Vec<String>,
    active_project: Option<String>,
    tabs: Vec<SessionTab>,
    active_tab: Option<usize>,
    request_pane_ratio: Option<f64>,
    /// Expanded collections and folders; none while every branch is still expanded.
    #[serde(default)]
    tree_expanded: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionTab {
    /// An open project handle; none for a tab without a project.
    project: Option<String>,
    draft: serde_json::Value,
}

/// The restored session; tabs refer to the handles of `projects` opened just now.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoredSession {
    projects: Vec<ProjectView>,
    active_project: Option<String>,
    tabs: Vec<SessionTab>,
    active_tab: Option<usize>,
    request_pane_ratio: Option<f64>,
    tree_expanded: Option<Vec<String>>,
    /// Projects that could not be reopened (moved, deleted, invalid); their tabs are kept
    /// without a project so unsaved edits are not lost.
    unavailable: Vec<String>,
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

fn valid_ratio(ratio: Option<f64>) -> Result<Option<f64>, String> {
    match ratio {
        Some(value) if !(0.0..=1.0).contains(&value) || !value.is_finite() => {
            Err("The request/response layout is invalid.".into())
        }
        other => Ok(other),
    }
}

fn decode(bytes: &[u8]) -> Result<Stored, String> {
    let stored: Stored = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
    let in_range = |index: Option<usize>, len: usize| index.is_none_or(|index| index < len);
    if stored.schema_version != 1
        || stored.projects.len() > MAX_PROJECTS
        || stored.tabs.len() > MAX_TABS
        || !in_range(stored.active_project, stored.projects.len())
        || !in_range(stored.active_tab, stored.tabs.len())
        || stored
            .tabs
            .iter()
            .any(|tab| !in_range(tab.project, stored.projects.len()) || !tab.draft.is_object())
        || stored.projects.iter().any(|project| {
            !project.directory.is_absolute() || validate_id(&project.project_id).is_err()
        })
        || valid_ratio(stored.request_pane_ratio).is_err()
        || stored.tree_expanded.as_ref().is_some_and(|ids| {
            ids.len() > MAX_EXPANDED || ids.iter().any(|id| validate_id(id).is_err())
        })
    {
        return Err(INVALID.into());
    }
    Ok(stored)
}

/// Saves the session. Tabs of handles that are not open are dropped.
pub fn save(
    settings_root: &Path,
    projects: &ProjectStore,
    input: SessionInput,
) -> Result<(), String> {
    if input.projects.len() > MAX_PROJECTS || input.tabs.len() > MAX_TABS {
        return Err("Too many open projects or tabs to remember for the next start.".into());
    }
    let mut stored_projects = Vec::new();
    let mut handles: Vec<&str> = Vec::new();
    for handle in &input.projects {
        if handles.contains(&handle.as_str()) {
            continue;
        }
        let project = projects.with_environment(handle, |root, id| {
            Ok(StoredProject {
                directory: root.to_path_buf(),
                project_id: id.to_owned(),
            })
        })?;
        stored_projects.push(project);
        handles.push(handle);
    }
    let index = |handle: &Option<String>| {
        handle
            .as_deref()
            .and_then(|handle| handles.iter().position(|item| *item == handle))
    };
    let mut tabs = Vec::new();
    let mut active_tab = None;
    for (position, tab) in input.tabs.into_iter().enumerate() {
        if !tab.draft.is_object() || (tab.project.is_some() && index(&tab.project).is_none()) {
            continue;
        }
        if input.active_tab == Some(position) {
            active_tab = Some(tabs.len());
        }
        tabs.push(StoredTab {
            project: index(&tab.project),
            draft: tab.draft,
        });
    }
    let stored = Stored {
        schema_version: 1,
        active_project: index(&input.active_project),
        projects: stored_projects,
        tabs,
        active_tab,
        request_pane_ratio: valid_ratio(input.request_pane_ratio)?,
        tree_expanded: input.tree_expanded.map(|ids| {
            let mut kept: Vec<String> = Vec::new();
            for id in ids {
                if validate_id(&id).is_ok() && !kept.contains(&id) {
                    kept.push(id);
                }
            }
            // The UI lists the most recently expanded last; keep those when over the limit.
            let excess = kept.len().saturating_sub(MAX_EXPANDED);
            kept.split_off(excess)
        }),
    };
    let bytes = serde_json::to_vec(&stored).map_err(|_| SETTINGS_ERROR)?;
    if bytes.len() as u64 > LIMIT {
        return Err("The open tabs are too large to remember for the next start.".into());
    }
    fs::create_dir_all(settings_root).map_err(|_| SETTINGS_ERROR)?;
    store::safe(settings_root, true).map_err(|_| SETTINGS_ERROR)?;
    let path = settings_root.join(FILE);
    if read(&path).ok().flatten().as_deref() == Some(bytes.as_slice()) {
        return Ok(());
    }
    // The temporary file is created owner-only and then renamed into place.
    store::atomic(&path, &bytes, false).map_err(|_| SETTINGS_ERROR.into())
}

/// Reopens the saved projects with ordinary validation. Returns none when no session was
/// saved yet, so the caller can fall back to the last-project setting.
pub fn restore(
    settings_root: &Path,
    projects: &mut ProjectStore,
) -> Result<Option<RestoredSession>, String> {
    let Some(bytes) = read(&settings_root.join(FILE))? else {
        return Ok(None);
    };
    let stored = decode(&bytes)?;
    let mut views = Vec::new();
    let mut handles: Vec<Option<String>> = Vec::new();
    let mut unavailable = Vec::new();
    for project in &stored.projects {
        let opened = store::load_project(&project.directory)
            .ok()
            .filter(|(data, _)| data.id == project.project_id)
            .and_then(|_| projects.open(&project.directory).ok());
        match opened {
            Some(view) if view.data.id == project.project_id => {
                handles.push(Some(view.handle.clone()));
                views.push(view);
            }
            Some(view) => {
                projects.close(&view.handle);
                handles.push(None);
                unavailable.push(project.directory.display().to_string());
            }
            None => {
                handles.push(None);
                unavailable.push(project.directory.display().to_string());
            }
        }
    }
    let handle = |index: Option<usize>| index.and_then(|index| handles[index].clone());
    Ok(Some(RestoredSession {
        projects: views,
        active_project: handle(stored.active_project),
        tabs: stored
            .tabs
            .into_iter()
            .map(|tab| SessionTab {
                project: handle(tab.project),
                draft: tab.draft,
            })
            .collect(),
        active_tab: stored.active_tab,
        request_pane_ratio: stored.request_pane_ratio,
        tree_expanded: stored.tree_expanded,
        unavailable,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn input(handles: &[&str], tabs: Vec<(Option<&str>, serde_json::Value)>) -> SessionInput {
        SessionInput {
            projects: handles.iter().map(|handle| handle.to_string()).collect(),
            active_project: handles.last().map(|handle| handle.to_string()),
            tabs: tabs
                .into_iter()
                .map(|(project, draft)| SessionTab {
                    project: project.map(str::to_string),
                    draft,
                })
                .collect(),
            active_tab: Some(1),
            request_pane_ratio: Some(0.75),
            tree_expanded: None,
        }
    }

    #[test]
    fn expanded_tree_branches_are_remembered_validated_and_optional() {
        let settings = tempdir().unwrap();
        let folder = tempdir().unwrap();
        let mut projects = ProjectStore::default();
        let a = projects.create(folder.path(), "Tree".into()).unwrap();
        let open = uuid::Uuid::new_v4().to_string();
        let older = uuid::Uuid::new_v4().to_string();
        let mut session = input(&[&a.handle], vec![]);
        session.tree_expanded = Some(vec![
            older.clone(),
            "not-an-id".into(),
            open.clone(),
            open.clone(),
        ]);
        save(settings.path(), &projects, session).unwrap();
        let restored = restore(settings.path(), &mut ProjectStore::default())
            .unwrap()
            .unwrap();
        assert_eq!(restored.tree_expanded, Some(vec![older, open]));

        // Sessions from before keep every branch expanded (no list at all).
        save(settings.path(), &projects, input(&[&a.handle], vec![])).unwrap();
        let bytes = std::fs::read(settings.path().join(FILE)).unwrap();
        assert!(!String::from_utf8(bytes).unwrap().contains("treeExpanded"));
        let restored = restore(settings.path(), &mut ProjectStore::default())
            .unwrap()
            .unwrap();
        assert_eq!(restored.tree_expanded, None);
    }

    #[test]
    fn open_projects_tabs_and_layout_come_back_with_new_handles() {
        let settings = tempdir().unwrap();
        let first = tempdir().unwrap();
        let second = tempdir().unwrap();
        let mut projects = ProjectStore::default();
        let a = projects.create(first.path(), "First".into()).unwrap();
        let b = projects.create(second.path(), "Second".into()).unwrap();
        assert!(restore(settings.path(), &mut ProjectStore::default())
            .unwrap()
            .is_none());
        save(
            settings.path(),
            &projects,
            input(
                &[&a.handle, &b.handle],
                vec![
                    (
                        Some(&a.handle),
                        json!({"name": "Saved", "url": "{{baseUrl}}/a"}),
                    ),
                    (
                        Some(&b.handle),
                        json!({"name": "Unsaved edit", "body": "synthetic"}),
                    ),
                    (None, json!({"name": "Loose"})),
                    (Some("closed-handle"), json!({"name": "Dropped"})),
                ],
            ),
        )
        .unwrap();
        let mut reopened = ProjectStore::default();
        let session = restore(settings.path(), &mut reopened).unwrap().unwrap();
        assert_eq!(
            session
                .projects
                .iter()
                .map(|view| &view.data.id)
                .collect::<Vec<_>>(),
            [&a.data.id, &b.data.id]
        );
        let (new_a, new_b) = (&session.projects[0].handle, &session.projects[1].handle);
        assert_ne!(new_a, &a.handle);
        assert_eq!(session.active_project.as_ref(), Some(new_b));
        assert_eq!(session.tabs.len(), 3);
        assert_eq!(session.tabs[0].project.as_ref(), Some(new_a));
        assert_eq!(session.tabs[1].project.as_ref(), Some(new_b));
        assert_eq!(session.tabs[1].draft["body"], "synthetic");
        assert_eq!(session.tabs[2].project, None);
        assert_eq!(session.active_tab, Some(1));
        assert_eq!(session.request_pane_ratio, Some(0.75));
        assert!(session.unavailable.is_empty());
    }

    #[test]
    fn a_vanished_project_keeps_its_tabs_without_a_project() {
        let settings = tempdir().unwrap();
        let folder = tempdir().unwrap();
        let mut projects = ProjectStore::default();
        let project = projects.create(folder.path(), "Gone".into()).unwrap();
        save(
            settings.path(),
            &projects,
            input(
                &[&project.handle],
                vec![(Some(&project.handle), json!({"name": "Kept"}))],
            ),
        )
        .unwrap();
        fs::remove_file(folder.path().join("project.fluxkit.json")).unwrap();
        let session = restore(settings.path(), &mut ProjectStore::default())
            .unwrap()
            .unwrap();
        assert!(session.projects.is_empty());
        assert_eq!(session.unavailable.len(), 1);
        assert_eq!(session.tabs[0].project, None);
        assert_eq!(session.tabs[0].draft["name"], "Kept");
        assert_eq!(session.active_project, None);
    }

    #[test]
    fn invalid_sessions_are_rejected_without_changes() {
        let settings = tempdir().unwrap();
        let path = settings.path().join(FILE);
        for bytes in [
            br#"{"schemaVersion":2,"projects":[],"activeProject":null,"tabs":[],"activeTab":null,"requestPaneRatio":null}"#.as_slice(),
            br#"{"schemaVersion":1,"projects":[],"activeProject":3,"tabs":[],"activeTab":null,"requestPaneRatio":null}"#,
            br#"{"schemaVersion":1,"projects":[],"activeProject":null,"tabs":[{"project":null,"draft":"text"}],"activeTab":null,"requestPaneRatio":null}"#,
            br#"{"schemaVersion":1,"projects":[],"activeProject":null,"tabs":[],"activeTab":null,"requestPaneRatio":7}"#,
            br#"{"schemaVersion":1,"projects":[{"directory":"relative","projectId":"x"}],"activeProject":null,"tabs":[],"activeTab":null,"requestPaneRatio":null}"#,
        ] {
            fs::write(&path, bytes).unwrap();
            assert_eq!(
                restore(settings.path(), &mut ProjectStore::default()).unwrap_err(),
                INVALID
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        let projects = ProjectStore::default();
        let mut bad = input(&[], vec![]);
        bad.request_pane_ratio = Some(f64::NAN);
        assert!(save(settings.path(), &projects, bad).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn the_session_file_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let settings = tempdir().unwrap();
        save(
            settings.path(),
            &ProjectStore::default(),
            input(&[], vec![(None, json!({}))]),
        )
        .unwrap();
        let mode = fs::metadata(settings.path().join(FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0);
    }
}
