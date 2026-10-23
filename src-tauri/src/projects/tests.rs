use super::{model::Collection, store::ProjectStore};
use serde_json::json;
use std::fs;
use tempfile::tempdir;
use uuid::Uuid;

fn collection() -> Collection {
    serde_json::from_value(json!({
        "schemaVersion": 1, "id": Uuid::new_v4().to_string(), "name": "Test API",
        "folders": [], "requests": [{
            "id": Uuid::new_v4().to_string(), "name": "Create item", "folderId": null,
            "method": "POST", "url": "https://example.test/items", "params": [],
            "headers": [{"id": 1, "enabled": true, "key": "Accept", "value": "application/json"}],
            "body": {"kind": "json", "text": "{\"title\":\"test\"}", "fields": [], "file": null}
        }]
    }))
    .expect("test fixture")
}

#[test]
fn saved_project_and_collection_survive_a_new_store() {
    let dir = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(dir.path(), "Project α".into()).unwrap();
    let data = collection();
    let id = data.id.clone();
    store.save_collection(&project.handle, data, None).unwrap();
    let reopened = ProjectStore::default().open(dir.path()).unwrap();
    assert_eq!(reopened.data.id, project.data.id);
    assert_eq!(reopened.collections[0].data.id, id);
    assert_eq!(
        reopened.collections[0].data.requests[0].body.text,
        "{\"title\":\"test\"}"
    );
}

#[test]
fn stale_revisions_preserve_external_edits_and_unsaved_input() {
    let dir = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(dir.path(), "Test".into()).unwrap();
    let saved = store
        .save_collection(&project.handle, collection(), None)
        .unwrap();
    let path = dir
        .path()
        .join(format!("collections/{}.json", saved.data.id));
    let mut external = saved.data.clone();
    external.name = "External edit".into();
    let bytes = serde_json::to_vec_pretty(&external).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert!(store
        .save_collection(&project.handle, saved.data, Some(saved.revision))
        .unwrap_err()
        .contains("outside"));
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn writes_keep_previous_version_and_removals_are_recoverable() {
    let dir = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(dir.path(), "Test".into()).unwrap();
    let mut saved = store
        .save_collection(&project.handle, collection(), None)
        .unwrap();
    let id = saved.data.id.clone();
    saved.data.name = "Renamed".into();
    let saved = store
        .save_collection(&project.handle, saved.data, Some(saved.revision))
        .unwrap();
    assert!(dir
        .path()
        .join(format!(".fluxkit/backups/{id}.json"))
        .is_file());
    store
        .remove_collection(&project.handle, &id, &saved.revision)
        .unwrap();
    assert_eq!(store.reload(&project.handle).unwrap().collections.len(), 0);
    assert_eq!(
        fs::read_dir(dir.path().join(".fluxkit/trash"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn invalid_or_future_files_are_not_rewritten() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("collections")).unwrap();
    let path = dir.path().join("project.fluxkit.json");
    for bytes in [
        "not json".to_string(),
        json!({"schemaVersion": 99,"id": Uuid::new_v4().to_string(),"name":"Test"}).to_string(),
    ] {
        fs::write(&path, &bytes).unwrap();
        assert!(ProjectStore::default().open(dir.path()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), bytes);
    }
}

#[test]
fn unsafe_identifiers_cycles_and_missing_parents_are_rejected() {
    let mut data = collection();
    data.id = "../../escape".into();
    assert!(data.validate().is_err());
    data.id = Uuid::new_v4().to_string();
    let id = Uuid::new_v4().to_string();
    data.folders.push(super::model::Folder {
        auth: None,
        id: id.clone(),
        name: "Loop".into(),
        parent_id: Some(id),
    });
    assert!(data.validate().is_err());
    data.folders[0].parent_id = Some(Uuid::new_v4().to_string());
    assert!(data.validate().is_err());
}

#[test]
fn attachments_are_copied_and_portable() {
    let dir = tempdir().unwrap();
    let source = tempdir().unwrap();
    let input = source.path().join("example.bin");
    fs::write(&input, [0, 1, 255]).unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(dir.path(), "Files".into()).unwrap();
    let attachment = store.attachment(&project.handle, &input).unwrap();
    attachment.validate().unwrap();
    assert!(!attachment.path.contains(source.path().to_str().unwrap()));
    assert_eq!(
        fs::read(dir.path().join(attachment.path)).unwrap(),
        [0, 1, 255]
    );
}

#[test]
fn folder_and_form_variants_round_trip_and_new_saves_do_not_overwrite() {
    let dir = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(dir.path(), "Forms".into()).unwrap();
    let mut data = collection();
    let parent = Uuid::new_v4().to_string();
    data.folders.push(super::model::Folder {
        auth: None,
        id: parent.clone(),
        name: "Parent".into(),
        parent_id: None,
    });
    data.requests[0].folder_id = Some(parent);
    data.requests[0].body.kind = super::model::BodyKind::Multipart;
    data.requests[0].body.fields.push(super::model::FormField {
        id: 1,
        enabled: true,
        key: "field".into(),
        value: "value".into(),
        file: None,
    });
    store
        .save_collection(&project.handle, data.clone(), None)
        .unwrap();
    assert!(store.save_collection(&project.handle, data, None).is_err());
    let loaded = store.reload(&project.handle).unwrap();
    assert_eq!(
        loaded.collections[0].data.requests[0].body.fields[0].value,
        "value"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_collections_and_backup_targets_are_rejected() {
    use std::os::unix::fs::symlink;
    let dir = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(dir.path(), "Safe".into()).unwrap();
    let saved = store
        .save_collection(&project.handle, collection(), None)
        .unwrap();
    fs::create_dir(dir.path().join(".fluxkit/backups")).unwrap();
    let outside_file = outside.path().join("untouched.json");
    fs::write(&outside_file, "untouched").unwrap();
    symlink(
        &outside_file,
        dir.path()
            .join(format!(".fluxkit/backups/{}.json", saved.data.id)),
    )
    .unwrap();
    assert!(store
        .save_collection(&project.handle, saved.data, Some(saved.revision))
        .is_err());
    assert_eq!(fs::read_to_string(&outside_file).unwrap(), "untouched");
    fs::remove_dir_all(dir.path().join("collections")).unwrap();
    symlink(outside.path(), dir.path().join("collections")).unwrap();
    assert!(store.reload(&project.handle).is_err());
}

#[test]
fn unknown_handles_and_nonempty_project_folders_are_rejected() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("keep.txt"), "keep").unwrap();
    let mut store = ProjectStore::default();
    assert!(store.create(dir.path(), "Test".into()).is_err());
    assert!(store.reload("unknown").is_err());
    assert_eq!(
        fs::read_to_string(dir.path().join("keep.txt")).unwrap(),
        "keep"
    );
}

#[test]
fn project_rename_keeps_identity_and_stale_or_replaced_manifests_are_rejected() {
    let dir = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let original = store.create(dir.path(), "Original".into()).unwrap();
    let renamed = store
        .rename(&original.handle, "Renamed".into(), &original.revision)
        .unwrap();
    assert_eq!(renamed.data.id, original.data.id);
    assert!(store
        .rename(&original.handle, "Stale".into(), &original.revision)
        .is_err());
    let mut replacement = renamed.data;
    replacement.id = Uuid::new_v4().to_string();
    fs::write(
        dir.path().join("project.fluxkit.json"),
        serde_json::to_vec(&replacement).unwrap(),
    )
    .unwrap();
    assert!(store
        .save_collection(&original.handle, collection(), None)
        .is_err());
}

#[test]
fn another_fluxkit_writer_cannot_bypass_the_project_lock() {
    use std::fs::OpenOptions;
    let dir = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(dir.path(), "Locked".into()).unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.path().join(".fluxkit/write.lock"))
        .unwrap();
    lock.lock().unwrap();
    assert!(store
        .save_collection(&project.handle, collection(), None)
        .is_err());
    assert_eq!(
        fs::read_dir(dir.path().join("collections"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn unrecognized_fields_fail_without_exposing_values() {
    let dir = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(dir.path(), "Fields".into()).unwrap();
    let saved = store
        .save_collection(&project.handle, collection(), None)
        .unwrap();
    let path = dir
        .path()
        .join(format!("collections/{}.json", saved.data.id));
    let mut value = serde_json::to_value(saved.data).unwrap();
    value["unknown"] = json!("private-sentinel-never-display");
    let bytes = serde_json::to_vec(&value).unwrap();
    fs::write(&path, &bytes).unwrap();
    let message = store.reload(&project.handle).unwrap_err();
    assert!(!message.contains("private-sentinel"));
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn authentication_roundtrips_with_backups_without_rewriting_legacy_definitions() {
    use super::model::Auth;
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Auth project".into()).unwrap();
    let original = fs::read(root.path().join("project.fluxkit.json")).unwrap();
    assert!(!String::from_utf8_lossy(&original).contains("auth"));
    let variable = Uuid::new_v4().to_string();
    let auth = Auth::Bearer {
        token: variable.clone(),
    };
    let saved = store
        .save_auth(&project.handle, Some(auth.clone()), &project.revision)
        .unwrap();
    assert_eq!(saved.data.auth, Some(auth.clone()));
    assert_eq!(
        fs::read(root.path().join(".fluxkit/backups/project.json")).unwrap(),
        original
    );
    assert!(store
        .save_auth(&project.handle, Some(Auth::None), &project.revision)
        .is_err());
    let mut data = collection();
    data.auth = Some(Auth::None);
    data.requests[0].auth = Some(auth.clone());
    store.save_collection(&project.handle, data, None).unwrap();
    let renamed = store
        .rename(&project.handle, "Renamed".into(), &saved.revision)
        .unwrap();
    assert_eq!(renamed.data.auth, Some(auth));
    let reopened = ProjectStore::default().open(root.path()).unwrap();
    assert_eq!(reopened.collections[0].data.auth, Some(Auth::None));
    assert_eq!(
        reopened.collections[0].data.requests[0].auth,
        Some(Auth::Bearer { token: variable })
    );
    let bytes = fs::read(root.path().join("project.fluxkit.json")).unwrap();
    let invalid = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("bearer", "unknownAuthMethod");
    fs::write(root.path().join("project.fluxkit.json"), invalid.as_bytes()).unwrap();
    assert!(store.reload(&project.handle).is_err());
    assert_eq!(
        fs::read(root.path().join("project.fluxkit.json")).unwrap(),
        invalid.as_bytes()
    );
}
