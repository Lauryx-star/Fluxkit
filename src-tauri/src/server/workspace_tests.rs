use super::*;
use crate::projects::{
    environments::{Definition, Definitions},
    model::{Body, BodyKind},
};
use crate::server::sync::{
    CollectionData, EnvironmentData, Kind, Op, Remote, RemoteProject, RequestData, RequestDetails,
    VariableData,
};
use tempfile::tempdir;

fn id(n: u8) -> String {
    format!("00000000-0000-4000-8000-0000000000{n:02x}")
}

fn tree() -> RemoteTree {
    RemoteTree {
        project: RemoteProject {
            id: id(99),
            name: "Synthetic".into(),
            kind: "team".into(),
            auth: None,
            revision: 2,
            can_edit: true,
        },
        collections: vec![
            Remote {
                id: id(1),
                revision: 1,
                data: CollectionData {
                    name: "First".into(),
                    auth: None,
                    open_api: None,
                    postman: None,
                    position: "g".into(),
                },
            },
            Remote {
                id: id(2),
                revision: 4,
                data: CollectionData {
                    name: "Second".into(),
                    auth: None,
                    open_api: None,
                    postman: None,
                    position: "n".into(),
                },
            },
        ],
        folders: vec![],
        requests: vec![Remote {
            id: id(10),
            revision: 3,
            data: RequestData {
                collection_id: id(1),
                folder_id: None,
                name: "Version".into(),
                method: "GET".into(),
                url: "{{baseUrl}}/api/v0/info/version".into(),
                details: RequestDetails {
                    import_issues: vec![],
                    auth: None,
                    params: vec![],
                    headers: vec![],
                    body: Body {
                        kind: BodyKind::None,
                        text: String::new(),
                        fields: vec![],
                        file: None,
                    },
                },
                position: "g".into(),
            },
        }],
        environments: vec![Remote {
            id: id(20),
            revision: 1,
            data: EnvironmentData {
                name: "Test".into(),
                position: "g".into(),
            },
        }],
        variables: vec![Remote {
            id: id(30),
            revision: 1,
            data: VariableData {
                name: "baseUrl".into(),
                secret: false,
                position: "g".into(),
            },
        }],
        seq: 9,
    }
}

fn mirror() -> (tempfile::TempDir, RemoteTree) {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("collections")).unwrap();
    fs::create_dir(dir.path().join(".fluxkit")).unwrap();
    let tree = tree();
    write_mirror(dir.path(), &tree).unwrap();
    (dir, tree)
}

fn collection(root: &Path, n: u8) -> Collection {
    store::decode(&store::read(&root.join(format!("collections/{}.json", id(n)))).unwrap()).unwrap()
}

fn save(root: &Path, collection: &Collection) {
    store::atomic(
        &root.join(format!("collections/{}.json", collection.id)),
        &store::encode(collection).unwrap(),
        false,
    )
    .unwrap();
}

#[test]
fn a_fresh_mirror_matches_the_tree_and_sends_nothing() {
    let (dir, tree) = mirror();
    assert_eq!(read_tree(dir.path()).unwrap(), tree);
    assert!(mirror_ops(dir.path(), &tree).unwrap().is_empty());
    let (definitions, _) = environments::definitions(dir.path()).unwrap();
    assert_eq!(definitions.variables[0].secret, Some(false));
    assert_eq!(store::load_project(dir.path()).unwrap().0.name, "Synthetic");
}

#[test]
fn a_saved_request_becomes_exactly_one_operation_with_its_revision() {
    let (dir, tree) = mirror();
    let mut first = collection(dir.path(), 1);
    first.requests[0].url = "{{baseUrl}}/api/v1/info/version".into();
    save(dir.path(), &first);
    let ops = mirror_ops(dir.path(), &tree).unwrap();
    assert_eq!(ops.len(), 1, "{ops:?}");
    assert_eq!(
        (ops[0].kind, ops[0].op, ops[0].expected_revision),
        (Kind::Request, Op::Put, Some(3))
    );
}

#[test]
fn a_removed_collection_file_is_a_collection_delete_and_rewrite_restores_it() {
    let (dir, tree) = mirror();
    fs::remove_file(dir.path().join(format!("collections/{}.json", id(2)))).unwrap();
    let ops = mirror_ops(dir.path(), &tree).unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(
        (ops[0].kind, ops[0].op, ops[0].expected_revision),
        (Kind::Collection, Op::Delete, Some(4))
    );
    // A rejected batch rolls the mirror back to the server state.
    write_mirror(dir.path(), &tree).unwrap();
    assert!(mirror_ops(dir.path(), &tree).unwrap().is_empty());
}

#[test]
fn writing_the_mirror_removes_collections_that_left_the_server() {
    let (dir, mut tree) = mirror();
    tree.collections.retain(|c| c.id != id(2));
    write_mirror(dir.path(), &tree).unwrap();
    assert!(!dir
        .path()
        .join(format!("collections/{}.json", id(2)))
        .exists());
    assert!(dir
        .path()
        .join(format!("collections/{}.json", id(1)))
        .exists());
}

#[test]
fn definition_and_project_edits_are_planned_with_the_rest() {
    let (dir, tree) = mirror();
    let mut definitions: Definitions = environments::definitions(dir.path()).unwrap().0;
    definitions.variables.push(Definition {
        id: id(31),
        name: "token".into(),
        secret: Some(true),
    });
    store::atomic(
        &dir.path().join("environments.fluxkit.json"),
        &store::encode(&definitions).unwrap(),
        false,
    )
    .unwrap();
    let mut project = store::load_project(dir.path()).unwrap().0;
    project.name = "Renamed".into();
    store::atomic(
        &dir.path().join("project.fluxkit.json"),
        &store::encode(&project).unwrap(),
        false,
    )
    .unwrap();
    let ops = mirror_ops(dir.path(), &tree).unwrap();
    let kinds: Vec<_> = ops
        .iter()
        .map(|op| (op.kind, op.expected_revision))
        .collect();
    assert_eq!(kinds, [(Kind::Project, Some(2)), (Kind::Variable, None)]);
}

#[test]
fn conflict_messages_name_what_changed_and_who_changed_it() {
    let message = conflict_message(&Rejection {
        error: Some("ignored".into()),
        conflicts: vec![
            ConflictEntry {
                kind: "request".into(),
                revision: Some(4),
                updated_by: Some("anna".into()),
            },
            ConflictEntry {
                kind: "folder".into(),
                revision: None,
                updated_by: None,
            },
        ],
    });
    assert!(
        message.contains("a request was changed by anna"),
        "{message}"
    );
    assert!(
        message.contains("a folder was deleted on the server"),
        "{message}"
    );
    let plain = conflict_message(&Rejection {
        error: Some("A folder's parent must be a folder in the same collection.".into()),
        conflicts: vec![],
    });
    assert!(plain.contains("same collection"));
}
