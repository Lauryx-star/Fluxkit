use super::*;
use crate::projects::model::{Body, BodyKind};
use serde_json::json;

fn id(n: u8) -> String {
    format!("00000000-0000-4000-8000-0000000000{n:02x}")
}

fn body() -> Body {
    Body {
        kind: BodyKind::None,
        text: String::new(),
        fields: vec![],
        file: None,
    }
}

fn request(
    n: u8,
    collection: u8,
    folder: Option<u8>,
    position: &str,
    revision: i64,
) -> Remote<RequestData> {
    Remote {
        id: id(n),
        revision,
        data: RequestData {
            collection_id: id(collection),
            folder_id: folder.map(id),
            name: format!("Request {n}"),
            method: "GET".into(),
            url: format!("https://synthetic.invalid/{n}"),
            details: RequestDetails {
                import_issues: vec![],
                auth: None,
                params: vec![],
                headers: vec![],
                body: body(),
            },
            position: position.into(),
        },
    }
}

fn folder(
    n: u8,
    collection: u8,
    parent: Option<u8>,
    position: &str,
    revision: i64,
) -> Remote<FolderData> {
    Remote {
        id: id(n),
        revision,
        data: FolderData {
            collection_id: id(collection),
            parent_id: parent.map(id),
            name: format!("Folder {n}"),
            auth: None,
            position: position.into(),
        },
    }
}

fn collection(n: u8, position: &str, revision: i64) -> Remote<CollectionData> {
    Remote {
        id: id(n),
        revision,
        data: CollectionData {
            name: format!("Collection {n}"),
            auth: None,
            open_api: None,
            postman: None,
            position: position.into(),
        },
    }
}

/// Collection 1 with folder 10 > folder 11 > request 21, plus top-level requests 20 and 22;
/// collection 2 with request 30; one environment and two variables.
fn tree() -> RemoteTree {
    RemoteTree {
        project: RemoteProject {
            id: id(99),
            name: "Synthetic".into(),
            kind: "team".into(),
            auth: None,
            revision: 4,
            can_edit: true,
        },
        collections: vec![collection(2, "n", 1), collection(1, "g", 3)],
        folders: vec![folder(11, 1, Some(10), "n", 2), folder(10, 1, None, "g", 1)],
        requests: vec![
            request(22, 1, None, "u", 1),
            request(20, 1, None, "g", 5),
            request(21, 1, Some(11), "n", 2),
            request(30, 2, None, "g", 1),
        ],
        environments: vec![Remote {
            id: id(40),
            revision: 1,
            data: EnvironmentData {
                name: "Test".into(),
                position: "g".into(),
            },
        }],
        variables: vec![
            Remote {
                id: id(51),
                revision: 2,
                data: VariableData {
                    name: "token".into(),
                    secret: true,
                    position: "n".into(),
                },
            },
            Remote {
                id: id(50),
                revision: 1,
                data: VariableData {
                    name: "baseUrl".into(),
                    secret: false,
                    position: "g".into(),
                },
            },
        ],
        seq: 17,
    }
}

fn collections(tree: &RemoteTree) -> Vec<Collection> {
    render(tree).collections
}

fn by_id(collections: &mut [Collection], n: u8) -> &mut Collection {
    collections.iter_mut().find(|c| c.id == id(n)).unwrap()
}

#[test]
fn ranks_sort_strictly_between_their_bounds_and_never_end_in_zero() {
    let cases = [
        (None, None),
        (Some("a"), None),
        (None, Some("a")),
        (Some("a"), Some("b")),
        (Some("a"), Some("a1")),
        (Some("az"), Some("b")),
        (Some("zz"), None),
        (None, Some("01")),
        (Some("g"), Some("h")),
        (Some("gi"), Some("gj")),
    ];
    for (low, high) in cases {
        let rank = rank_between(low, high);
        assert!(low.is_none_or(|l| l < rank.as_str()), "{low:?} < {rank}");
        assert!(high.is_none_or(|h| rank.as_str() < h), "{rank} < {high:?}");
        assert!(!rank.ends_with('0'), "{rank}");
    }
    // Repeated inserts at the same spot keep producing distinct, ordered ranks.
    let mut low = "a".to_string();
    let high = "b".to_string();
    for _ in 0..200 {
        let next = rank_between(Some(&low), Some(&high));
        assert!(low < next && next < high);
        low = next;
    }
}

#[test]
fn positions_keep_existing_order_and_only_rank_new_or_moved_items() {
    let items = vec![
        ("a".to_string(), Some("g".to_string())),
        ("new".to_string(), None),
        ("b".to_string(), Some("n".to_string())),
        ("moved".to_string(), Some("c".to_string())),
        ("c".to_string(), Some("u".to_string())),
    ];
    let result = positions(&items);
    assert_eq!(result[0], "g");
    assert_eq!(result[2], "n");
    assert_eq!(result[4], "u");
    assert!("g" < result[1].as_str() && result[1].as_str() < "n");
    assert!("n" < result[3].as_str() && result[3].as_str() < "u");
}

#[test]
fn render_orders_by_position_and_an_unchanged_save_sends_nothing() {
    let tree = tree();
    let rendered = render(&tree);
    assert_eq!(rendered.collections[0].id, id(1));
    let ids: Vec<_> = rendered.collections[0]
        .requests
        .iter()
        .map(|r| r.id.clone())
        .collect();
    assert_eq!(ids, [id(20), id(21), id(22)]);
    assert_eq!(rendered.collections[0].folders[0].id, id(10));
    assert_eq!(rendered.definitions.variables[0].name, "baseUrl");
    assert_eq!(rendered.definitions.variables[0].secret, Some(false));
    for collection in &rendered.collections {
        collection.validate().unwrap();
    }
    rendered.definitions.validate().unwrap();
    assert!(plan_collections(&tree, &rendered.collections)
        .unwrap()
        .is_empty());
    assert!(plan_definitions(&tree, &rendered.definitions)
        .unwrap()
        .is_empty());
}

#[test]
fn editing_one_request_sends_exactly_that_request_with_its_revision() {
    let tree = tree();
    let mut local = collections(&tree);
    by_id(&mut local, 1).requests[0].url = "https://changed.invalid".into();
    let ops = plan_collections(&tree, &[by_id(&mut local, 1).clone()]).unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].op, Op::Put);
    assert_eq!(ops[0].kind, Kind::Request);
    assert_eq!(ops[0].id, id(20));
    assert_eq!(ops[0].expected_revision, Some(5));
    assert_eq!(ops[0].data.as_ref().unwrap()["position"], "g");
}

#[test]
fn a_new_request_between_two_others_gets_a_rank_between_them_and_nothing_else_moves() {
    let tree = tree();
    let mut local = collections(&tree);
    let mut new = local[0].requests[0].clone();
    new.id = id(23);
    new.name = "Inserted".into();
    by_id(&mut local, 1).requests.insert(1, new);
    let ops = plan_collections(&tree, &[by_id(&mut local, 1).clone()]).unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].expected_revision, None);
    let position = ops[0].data.as_ref().unwrap()["position"]
        .as_str()
        .unwrap()
        .to_string();
    assert!("g" < position.as_str() && position.as_str() < "n");
}

#[test]
fn moving_a_request_to_another_collection_is_one_put_and_no_delete() {
    let tree = tree();
    let mut local = collections(&tree);
    let moved = by_id(&mut local, 1).requests.remove(2);
    by_id(&mut local, 2).requests.push(moved);
    let ops = plan_collections(&tree, &local).unwrap();
    assert_eq!(ops.len(), 1, "{ops:?}");
    assert_eq!(ops[0].op, Op::Put);
    assert_eq!(ops[0].id, id(22));
    assert_eq!(ops[0].data.as_ref().unwrap()["collectionId"], json!(id(2)));
}

#[test]
fn deleting_a_folder_sends_one_delete_for_the_top_folder_only() {
    let tree = tree();
    let mut local = collections(&tree);
    let first = by_id(&mut local, 1);
    first.folders.clear();
    first.requests.retain(|r| r.folder_id.is_none());
    let ops = plan_collections(&tree, std::slice::from_ref(first)).unwrap();
    assert_eq!(ops, vec![delete(Kind::Folder, &id(10), 1)]);
}

#[test]
fn a_removed_top_level_request_is_deleted_with_its_revision() {
    let tree = tree();
    let mut local = collections(&tree);
    let first = by_id(&mut local, 1);
    first.requests.retain(|r| r.id != id(22));
    let ops = plan_collections(&tree, std::slice::from_ref(first)).unwrap();
    assert_eq!(ops, vec![delete(Kind::Request, &id(22), 1)]);
}

#[test]
fn new_nested_folders_are_sent_parent_first_even_when_listed_child_first() {
    let tree = tree();
    let mut local = collections(&tree);
    let first = by_id(&mut local, 1);
    first.folders.insert(
        0,
        Folder {
            id: id(13),
            name: "Child".into(),
            auth: None,
            parent_id: Some(id(12)),
        },
    );
    first.folders.push(Folder {
        id: id(12),
        name: "Parent".into(),
        auth: None,
        parent_id: None,
    });
    let ops = plan_collections(&tree, std::slice::from_ref(first)).unwrap();
    let order: Vec<_> = ops.iter().map(|op| op.id.clone()).collect();
    let parent = order.iter().position(|o| *o == id(12)).unwrap();
    let child = order.iter().position(|o| *o == id(13)).unwrap();
    assert!(parent < child, "{order:?}");
    assert!(
        ops.iter().all(|op| op.expected_revision.is_none()),
        "{ops:?}"
    );
}

#[test]
fn folder_cycles_are_rejected_before_anything_is_sent() {
    let tree = tree();
    let mut local = collections(&tree);
    let first = by_id(&mut local, 1);
    first.folders[0].parent_id = Some(id(11));
    assert!(plan_collections(&tree, std::slice::from_ref(first)).is_err());
}

#[test]
fn a_new_collection_sends_its_header_folders_and_requests() {
    let tree = tree();
    let new = Collection {
        schema_version: SCHEMA_VERSION,
        id: id(3),
        name: "New".into(),
        auth: None,
        open_api: None,
        postman: None,
        folders: vec![Folder {
            id: id(60),
            name: "F".into(),
            auth: None,
            parent_id: None,
        }],
        requests: vec![local_request(&request(61, 3, Some(60), "", 0))],
    };
    let ops = plan_collections(&tree, &[new]).unwrap();
    let kinds: Vec<_> = ops.iter().map(|op| op.kind).collect();
    assert_eq!(kinds, [Kind::Collection, Kind::Folder, Kind::Request]);
    let position = ops[0].data.as_ref().unwrap()["position"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        position.as_str() > "n",
        "new collections go last: {position}"
    );
}

#[test]
fn collection_delete_needs_the_current_revision() {
    let tree = tree();
    assert_eq!(
        plan_collection_delete(&tree, &id(1)).unwrap(),
        vec![delete(Kind::Collection, &id(1), 3)]
    );
    assert!(plan_collection_delete(&tree, &id(9)).is_err());
}

#[test]
fn definition_changes_send_only_the_changed_environment_or_variable() {
    let tree = tree();
    let mut definitions = render(&tree).definitions;
    definitions.variables[1].secret = Some(false);
    definitions.variables.remove(0);
    definitions.environments.push(Definition {
        id: id(41),
        name: "Prod".into(),
        secret: None,
    });
    let ops = plan_definitions(&tree, &definitions).unwrap();
    assert_eq!(ops.len(), 3, "{ops:?}");
    assert_eq!(
        (ops[0].kind, ops[0].id.clone(), ops[0].expected_revision),
        (Kind::Environment, id(41), None)
    );
    assert_eq!(
        (ops[1].kind, ops[1].id.clone(), ops[1].expected_revision),
        (Kind::Variable, id(51), Some(2))
    );
    assert_eq!(ops[1].data.as_ref().unwrap()["secret"], false);
    assert_eq!(ops[2], delete(Kind::Variable, &id(50), 1));
}

#[test]
fn project_changes_are_sent_only_when_name_or_auth_differ() {
    let tree = tree();
    assert!(plan_project(&tree, "Synthetic", None).unwrap().is_empty());
    let ops = plan_project(&tree, "Renamed", None).unwrap();
    assert_eq!(ops[0].kind, Kind::Project);
    assert_eq!(ops[0].expected_revision, Some(4));
}

#[test]
fn accepted_operations_and_feed_entries_update_the_tree() {
    let mut tree = tree();
    let mut local = collections(&tree);
    by_id(&mut local, 1).requests[0].name = "Renamed".into();
    let ops = plan_collections(&tree, &[by_id(&mut local, 1).clone()]).unwrap();
    let changes = accepted(&ops, &BTreeMap::from([(id(20), 6)])).unwrap();
    apply(&mut tree, &changes).unwrap();
    let saved = tree.requests.iter().find(|r| r.id == id(20)).unwrap();
    assert_eq!((saved.revision, saved.data.name.as_str()), (6, "Renamed"));
    assert!(plan_collections(&tree, &[by_id(&mut local, 1).clone()])
        .unwrap()
        .is_empty());
    assert!(accepted(&ops, &BTreeMap::new()).is_err());

    // Another user's feed: a new request in collection 2, the folder tree removed.
    let new = request(31, 2, None, "n", 1);
    apply(
        &mut tree,
        &[
            Change {
                kind: Kind::Request,
                id: id(31),
                operation: Op::Put,
                revision: 1,
                data: Some(serde_json::to_value(&new.data).unwrap()),
            },
            Change {
                kind: Kind::Folder,
                id: id(10),
                operation: Op::Delete,
                revision: 0,
                data: None,
            },
        ],
    )
    .unwrap();
    assert!(tree.folders.is_empty());
    assert!(!tree.requests.iter().any(|r| r.id == id(21)));
    assert!(tree.requests.iter().any(|r| r.id == id(31)));

    apply(
        &mut tree,
        &[Change {
            kind: Kind::Collection,
            id: id(2),
            operation: Op::Delete,
            revision: 0,
            data: None,
        }],
    )
    .unwrap();
    assert!(!tree.requests.iter().any(|r| r.data.collection_id == id(2)));
    assert!(apply(
        &mut tree,
        &[Change {
            kind: Kind::Project,
            id: id(99),
            operation: Op::Delete,
            revision: 0,
            data: None,
        }]
    )
    .is_err());
}

#[test]
fn wire_format_is_camel_case_with_lowercase_kinds() {
    let tree = tree();
    let mut local = collections(&tree);
    by_id(&mut local, 1).requests[0].method = "POST".into();
    let ops = plan_collections(&tree, &[by_id(&mut local, 1).clone()]).unwrap();
    let wire = serde_json::to_value(&ops).unwrap();
    assert_eq!(wire[0]["op"], "put");
    assert_eq!(wire[0]["kind"], "request");
    assert_eq!(wire[0]["expectedRevision"], 5);
    assert_eq!(wire[0]["data"]["collectionId"], json!(id(1)));
    assert!(wire[0]["data"]["details"]["body"].is_object());
    let decoded: RemoteTree = serde_json::from_value(serde_json::to_value(&tree).unwrap()).unwrap();
    assert_eq!(decoded, tree);
}
