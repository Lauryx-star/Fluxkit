//! Request relocation stays within one authorized project; values and attachments keep identity.
use super::{
    model::{validate_id, Request},
    store::{self, CollectionView},
};
use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MoveInput {
    pub request: Request,
    pub source_collection_id: Option<String>,
    pub source_revision: Option<String>,
    pub target_collection_id: String,
    pub target_revision: String,
    pub project_revision: String,
    pub allow_metadata_loss: bool,
}

pub fn apply(root: &Path, handle: &str, input: MoveInput) -> Result<Vec<CollectionView>, String> {
    apply_with(root, handle, input, store::replace)
}
fn apply_with(
    root: &Path,
    handle: &str,
    input: MoveInput,
    mut write: impl FnMut(&Path, &Path, &str, &[u8], &str) -> Result<(), String>,
) -> Result<Vec<CollectionView>, String> {
    input.request.validate()?;
    validate_id(&input.target_collection_id)?;
    if let Some(id) = &input.source_collection_id {
        validate_id(id)?;
    }
    if input.source_collection_id.is_some() != input.source_revision.is_some() {
        return Err("A saved source requires its current revision.".into());
    }
    let project = store::load(root, handle)?;
    if project.revision != input.project_revision {
        return Err("Project changed. Reload it before moving this request.".into());
    }
    let target = project
        .collections
        .iter()
        .find(|c| c.data.id == input.target_collection_id)
        .ok_or("The destination collection no longer exists. Reload the project.")?;
    if target.revision != input.target_revision {
        return Err(
            "Destination changed outside Fluxkit. Reload before moving; your edits are still open."
                .into(),
        );
    }
    let source = input.source_collection_id.as_ref().map(|id| {
        let source = project.collections.iter().find(|c| &c.data.id == id)
            .ok_or("The source collection no longer exists. Reload the project.")?;
        if Some(&source.revision) != input.source_revision.as_ref() { return Err("Source changed outside Fluxkit. Reload before moving; your edits are still open."); }
        if !source.data.requests.iter().any(|r| r.id == input.request.id) { return Err("The saved request no longer exists in its source collection."); }
        Ok(source)
    }).transpose()?;
    let same = source.is_some_and(|source| source.data.id == target.data.id);
    if !same
        && target
            .data
            .requests
            .iter()
            .any(|r| r.id == input.request.id)
    {
        return Err("The destination already contains this request identity. Reload and inspect both collections before moving; no files were changed.".into());
    }
    let mut destination = target.data.clone();
    if same {
        for request in &mut destination.requests {
            if request.id == input.request.id {
                *request = input.request.clone();
            }
        }
    } else {
        destination.requests.push(input.request.clone());
    }
    if !same {
        if let Some(metadata) = source.and_then(|source| source.data.open_api.as_ref()) {
            if let Some(pointer) = metadata.operations.get(&input.request.id) {
                match &mut destination.open_api {
                    None => { let mut metadata = metadata.clone(); metadata.operations.retain(|id, _| id == &input.request.id); destination.open_api = Some(metadata); }
                    Some(target) if target.document == metadata.document && target.version == metadata.version => { target.operations.insert(input.request.id.clone(), pointer.clone()); }
                    Some(_) if input.allow_metadata_loss => {}
                    Some(_) => return Err("The collections use different OpenAPI source documents. Explicitly allow moving without the original schema association, or choose another destination. No files were changed.".into()),
                }
            }
        }
    }
    destination.validate()?;
    let destination_bytes = store::encode(&destination)?;
    let mut origin = source.filter(|_| !same).map(|source| source.data.clone());
    if let Some(origin) = &mut origin {
        origin.requests.retain(|r| r.id != input.request.id);
        origin.validate()?;
    }
    let origin_bytes = origin.as_ref().map(store::encode).transpose()?;
    let total: usize = project
        .collections
        .iter()
        .map(|collection| {
            if collection.data.id == destination.id {
                Ok(destination_bytes.len())
            } else if origin
                .as_ref()
                .is_some_and(|origin| origin.id == collection.data.id)
            {
                Ok(origin_bytes.as_ref().map_or(0, Vec::len))
            } else {
                store::encode(&collection.data).map(|bytes| bytes.len())
            }
        })
        .collect::<Result<Vec<_>, String>>()?
        .into_iter()
        .sum();
    let intermediate =
        if let (Some(source), Some(bytes)) = (source.filter(|_| !same), &origin_bytes) {
            total.saturating_add(
                store::encode(&source.data)?
                    .len()
                    .saturating_sub(bytes.len()),
            )
        } else {
            total
        };
    if intermediate > 32 * 1024 * 1024 {
        return Err("Moved project would exceed the 32 MiB collection limit.".into());
    }
    let target_path = root
        .join("collections")
        .join(format!("{}.json", destination.id));
    let original_target = store::read(&target_path)?;
    // Copy first: failure or interruption must never remove the only saved request.
    write(
        root,
        &target_path,
        &target.revision,
        &destination_bytes,
        &format!("{}.json", destination.id),
    )?;
    let mut results = vec![CollectionView {
        revision: store::revision(&destination_bytes),
        data: destination,
    }];
    if let (Some(origin), Some(bytes), Some(source)) = (origin, origin_bytes, source) {
        let source_path = root.join("collections").join(format!("{}.json", origin.id));
        let removal = if store::read(&target_path).is_ok_and(|current| current == destination_bytes)
        {
            write(
                root,
                &source_path,
                &source.revision,
                &bytes,
                &format!("{}.json", origin.id),
            )
        } else {
            Err("The destination changed during this move.".into())
        };
        if let Err(error) = removal {
            // Roll back only our exact destination snapshot. Never replace an external edit.
            let restored = store::read(&target_path)
                .is_ok_and(|current| current == destination_bytes)
                && store::atomic(&target_path, &original_target, false).is_ok();
            return Err(if restored {
                format!("Move failed; the source remains and the destination was restored. {error}")
            } else {
                "Move could not finish or restore the destination. The source request remains; a saved copy may also exist at the destination. Reload, inspect both collections and keep the intended copy. Your draft stays open.".into()
            });
        }
        results.push(CollectionView {
            revision: store::revision(&bytes),
            data: origin,
        });
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{
        model::{Collection, OpenApiMetadata},
        store::{ProjectStore, ProjectView},
    };
    use serde_json::json;
    use std::fs;
    use tempfile::{tempdir, TempDir};
    use uuid::Uuid;
    struct Fixture {
        root: TempDir,
        store: ProjectStore,
        project: ProjectView,
        source: CollectionView,
        target: CollectionView,
        input: MoveInput,
    }
    fn fixture() -> Fixture {
        let root = tempdir().unwrap();
        let mut store = ProjectStore::default();
        let project = store.create(root.path(), "Synthetic".into()).unwrap();
        let source: Collection = serde_json::from_value(json!({
            "schemaVersion":1,"id":Uuid::new_v4().to_string(),"name":"Source",
            "folders":[],"requests":[{"id":Uuid::new_v4().to_string(),"name":"Original",
                "folderId":null,"method":"GET","url":"https://example.invalid/original",
                "headers":[],"params":[],"body":{"kind":"none","text":"","fields":[],"file":null}}]
        }))
        .unwrap();
        let source = store
            .save_collection(&project.handle, source, None)
            .unwrap();
        let mut target = source.data.clone();
        target.id = Uuid::new_v4().to_string();
        target.name = "Destination".into();
        target.requests.clear();
        let target = store
            .save_collection(&project.handle, target, None)
            .unwrap();
        let mut request = source.data.requests[0].clone();
        request.name = "Edited".into();
        request.auth = Some(crate::projects::model::Auth::Bearer {
            token: Uuid::new_v4().to_string(),
        });
        let input = MoveInput {
            request,
            source_collection_id: Some(source.data.id.clone()),
            source_revision: Some(source.revision.clone()),
            target_collection_id: target.data.id.clone(),
            target_revision: target.revision.clone(),
            project_revision: project.revision.clone(),
            allow_metadata_loss: false,
        };
        Fixture {
            root,
            store,
            project,
            source,
            target,
            input,
        }
    }
    fn bytes(f: &Fixture, id: &str) -> Vec<u8> {
        fs::read(f.root.path().join(format!("collections/{id}.json"))).unwrap()
    }
    #[test]
    fn cross_collection_move_saves_edits_preserves_identity_auth_and_other_requests() {
        let f = fixture();
        let id = f.input.request.id.clone();
        let auth = serde_json::to_value(&f.input.request.auth).unwrap();
        f.store
            .with_environment(&f.project.handle, |root, _| {
                apply(root, &f.project.handle, f.input.clone())
            })
            .unwrap();
        let reopened = ProjectStore::default().open(f.root.path()).unwrap();
        let source = reopened
            .collections
            .iter()
            .find(|c| c.data.id == f.source.data.id)
            .unwrap();
        let target = reopened
            .collections
            .iter()
            .find(|c| c.data.id == f.target.data.id)
            .unwrap();
        assert!(source.data.requests.is_empty());
        assert_eq!(target.data.requests.len(), 1);
        assert_eq!(target.data.requests[0].id, id);
        assert_eq!(target.data.requests[0].name, "Edited");
        assert_eq!(
            serde_json::to_value(&target.data.requests[0].auth).unwrap(),
            auth
        );
        let backup: Collection = store::decode(
            &store::read(
                &f.root
                    .path()
                    .join(format!(".fluxkit/backups/{}.json", f.source.data.id)),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(backup.requests[0].name, "Original");
    }
    #[test]
    fn folder_move_updates_one_collection_without_reordering_or_losing_other_requests() {
        let mut f = fixture();
        let folder_id = Uuid::new_v4().to_string();
        let mut data = f.source.data.clone();
        data.folders.push(crate::projects::model::Folder {
            id: folder_id.clone(),
            name: "Nested".into(),
            parent_id: None,
            auth: None,
        });
        let mut other = data.requests[0].clone();
        other.id = Uuid::new_v4().to_string();
        other.name = "Keep".into();
        data.requests.push(other);
        let source = f
            .store
            .save_collection(&f.project.handle, data, Some(f.source.revision.clone()))
            .unwrap();
        f.input.source_revision = Some(source.revision.clone());
        f.input.target_collection_id = source.data.id.clone();
        f.input.target_revision = source.revision;
        f.input.request.folder_id = Some(folder_id.clone());
        let result = apply(f.root.path(), &f.project.handle, f.input).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].data.requests.len(), 2);
        assert_eq!(
            result[0].data.requests[0].folder_id.as_ref(),
            Some(&folder_id)
        );
        assert_eq!(result[0].data.requests[1].name, "Keep");
    }
    #[test]
    fn invalid_stale_or_duplicate_moves_leave_both_files_unchanged() {
        for kind in ["source", "target", "project", "folder", "identity"] {
            let mut f = fixture();
            if kind == "identity" {
                let mut target = f.target.data.clone();
                target.requests.push(f.source.data.requests[0].clone());
                f.target = f
                    .store
                    .save_collection(&f.project.handle, target, Some(f.target.revision))
                    .unwrap();
                f.input.target_revision = f.target.revision.clone();
            }
            match kind {
                "source" => f.input.source_revision = Some("stale".into()),
                "target" => f.input.target_revision = "stale".into(),
                "project" => f.input.project_revision = "stale".into(),
                "folder" => f.input.request.folder_id = Some(Uuid::new_v4().to_string()),
                _ => {}
            }
            let source = bytes(&f, &f.source.data.id);
            let target = bytes(&f, &f.target.data.id);
            assert!(apply(f.root.path(), &f.project.handle, f.input.clone()).is_err());
            assert_eq!(bytes(&f, &f.source.data.id), source);
            assert_eq!(bytes(&f, &f.target.data.id), target);
        }
    }
    #[test]
    fn failed_source_write_rolls_back_destination_and_keeps_both_original_files() {
        let f = fixture();
        let source = bytes(&f, &f.source.data.id);
        let target = bytes(&f, &f.target.data.id);
        let mut calls = 0;
        let error = apply_with(
            f.root.path(),
            &f.project.handle,
            f.input.clone(),
            |root, path, expected, bytes, name| {
                calls += 1;
                if calls == 2 {
                    return Err("Synthetic source write failure".into());
                }
                store::replace(root, path, expected, bytes, name)
            },
        )
        .unwrap_err();
        assert!(error.contains("destination was restored"));
        assert_eq!(bytes(&f, &f.source.data.id), source);
        assert_eq!(bytes(&f, &f.target.data.id), target);
    }
    #[test]
    fn rollback_never_overwrites_an_external_destination_edit() {
        let f = fixture();
        let source = bytes(&f, &f.source.data.id);
        let target_path = f
            .root
            .path()
            .join(format!("collections/{}.json", f.target.data.id));
        let mut calls = 0;
        let error = apply_with(
            f.root.path(),
            &f.project.handle,
            f.input.clone(),
            |root, path, expected, bytes, name| {
                calls += 1;
                if calls == 2 {
                    let mut external: Collection =
                        store::decode(&store::read(&target_path).unwrap()).unwrap();
                    external.name = "External edit".into();
                    fs::write(&target_path, store::encode(&external).unwrap()).unwrap();
                    return Err("Synthetic source write failure".into());
                }
                store::replace(root, path, expected, bytes, name)
            },
        )
        .unwrap_err();
        assert!(error.contains("copy may also exist"));
        assert_eq!(bytes(&f, &f.source.data.id), source);
        let target: Collection = store::decode(&store::read(&target_path).unwrap()).unwrap();
        assert_eq!(target.name, "External edit");
        assert_eq!(target.requests.len(), 1);
    }
    #[test]
    fn unsaved_request_is_created_once_and_invalid_ids_are_rejected() {
        let mut f = fixture();
        f.input.source_collection_id = None;
        f.input.source_revision = None;
        f.input.request.id = Uuid::new_v4().to_string();
        let result = apply(f.root.path(), &f.project.handle, f.input.clone()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].data.requests[0].id, f.input.request.id);
        let mut invalid = f.input;
        invalid.target_collection_id = "../outside".into();
        assert!(apply(f.root.path(), &f.project.handle, invalid).is_err());
    }
    #[test]
    fn openapi_association_transfers_or_requires_explicit_metadata_omission() {
        for conflict in [false, true] {
            let mut f = fixture();
            let id = f.input.request.id.clone();
            let metadata = OpenApiMetadata {
                version: "3.1.0".into(),
                document: json!({"openapi":"3.1.0","info":{"title":"Source","version":"1"},"paths":{"/items":{"get":{"responses":{"200":{"description":"OK"}}}}}}),
                operations: [(id.clone(), "#/paths/~1items/get".into())].into(),
            };
            let mut source = f.source.data.clone();
            source.open_api = Some(metadata.clone());
            f.source = f
                .store
                .save_collection(&f.project.handle, source, Some(f.source.revision))
                .unwrap();
            f.input.source_revision = Some(f.source.revision.clone());
            if conflict {
                let mut target = f.target.data.clone();
                let mut other = metadata.clone();
                other.document["info"]["title"] = json!("Other");
                other.operations.clear();
                target.open_api = Some(other);
                f.target = f
                    .store
                    .save_collection(&f.project.handle, target, Some(f.target.revision))
                    .unwrap();
                f.input.target_revision = f.target.revision.clone();
                assert!(apply(f.root.path(), &f.project.handle, f.input.clone())
                    .unwrap_err()
                    .contains("different OpenAPI"));
                f.input.allow_metadata_loss = true;
            }
            let result = apply(f.root.path(), &f.project.handle, f.input).unwrap();
            let target = &result[0].data.open_api.as_ref().unwrap();
            if conflict {
                assert!(!target.operations.contains_key(&id));
                assert_eq!(target.document["info"]["title"], "Other");
            } else {
                assert_eq!(target.operations[&id], metadata.operations[&id]);
            }
            assert_eq!(
                result[1].data.open_api.as_ref().unwrap().document,
                metadata.document
            );
        }
    }
}
