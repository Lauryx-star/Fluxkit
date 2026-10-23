use super::{postman, storage, tests::Fake};
use crate::projects::{
    environments::{self, Definitions},
    model::{Auth, BodyKind},
    store::{self, ProjectStore},
};
use serde_json::{json, Value};
use tempfile::tempdir;
const COLLECTION: &[u8] = include_bytes!("../../tests/fixtures/postman/collection-2.1.json");
const ENVIRONMENT: &[u8] = include_bytes!("../../tests/fixtures/postman/environment.json");

#[test]
fn collection_maps_hierarchy_auth_bodies_and_scopes_without_writes_or_private_source_copies() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store
        .create(root.path(), "Synthetic import".into())
        .unwrap();
    let mut plan = postman::collection(
        COLLECTION,
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    assert!(
        plan.preview.blockers.is_empty(),
        "{:?}",
        plan.preview.blockers
    );
    assert_eq!(plan.collection.requests.len(), 5);
    assert_eq!(plan.collection.folders.len(), 2);
    assert!(plan.collection.folders[1].parent_id.is_some());
    assert!(!root.path().join("environments.fluxkit.json").exists());
    assert!(matches!(plan.collection.auth, Some(Auth::Bearer { .. })));
    assert!(matches!(plan.collection.folders[0].auth, Some(Auth::None)));
    let get = &plan.collection.requests[0];
    assert!(get.auth.is_none());
    assert_eq!(get.params.len(), 2);
    assert!(!get.params[1].enabled);
    assert!(!get.url.contains('?'));
    assert!(plan
        .variables
        .iter()
        .any(|v| v.value.as_deref() == Some("a%2Fb")));
    assert!(matches!(
        plan.collection.requests[1].auth,
        Some(Auth::Basic { .. })
    ));
    assert!(matches!(
        plan.collection.requests[1].body.kind,
        BodyKind::Json
    ));
    assert!(matches!(
        plan.collection.requests[2].body.kind,
        BodyKind::Multipart
    ));
    assert!(matches!(
        plan.collection.requests[3].body.kind,
        BodyKind::UrlEncoded
    ));
    assert!(matches!(
        plan.collection.requests[4].auth,
        Some(Auth::Oauth2 { .. })
    ));
    let metadata = plan.collection.postman.as_ref().unwrap();
    assert_eq!(metadata.variables.len(), 3);
    for source in ["baseUrl", "token", "username"] {
        assert!(metadata.variables.contains_key(source));
    }
    let serialized = serde_json::to_string(&plan.collection).unwrap();
    let preview = serde_json::to_string(&plan.preview).unwrap();
    for private in [
        "synthetic-token",
        "synthetic-password",
        "synthetic-header",
        "synthetic-client-secret",
        "SAVED_RESPONSE_MUST_NOT_PERSIST",
        "must never run",
    ] {
        assert!(!serialized.contains(private));
        assert!(!preview.contains(private));
    }
    assert!(serialized.contains("synthetic-body"));
    let fake = Fake::default();
    storage::apply(root.path(), &project.data.id, &mut plan, true, &fake).unwrap();
    let imported = store.reload(&project.handle).unwrap();
    assert_eq!(imported.collections.len(), 1);
    assert!(imported.collections[0].data.postman.is_some());
    let (defs, _) = environments::definitions(root.path()).unwrap();
    let encoded = serde_json::to_string(&defs).unwrap();
    assert!(!encoded.contains("synthetic-password"));
}

#[test]
fn associated_environments_have_new_project_identity_preserve_defaults_and_retry_partial_values() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store
        .create(root.path(), "Synthetic environment".into())
        .unwrap();
    let mut collection = postman::collection(
        COLLECTION,
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    let fake = Fake::default();
    storage::apply(root.path(), &project.data.id, &mut collection, true, &fake).unwrap();
    let project = store.reload(&project.handle).unwrap();
    let collection_before = std::fs::read(
        root.path()
            .join("collections")
            .join(format!("{}.json", collection.collection.id)),
    )
    .unwrap();
    let (defs, revision) = environments::definitions(root.path()).unwrap();
    let mut plan = postman::environment(
        ENVIRONMENT,
        &project,
        defs,
        revision,
        &collection.collection.id,
    )
    .unwrap();
    assert!(plan.environment_only);
    assert_eq!(plan.preview.request_count, 0);
    let env = plan.definitions.environments[0].clone();
    assert!(plan
        .variables
        .iter()
        .all(|v| v.environment.as_deref() == Some(env.id.as_str())));
    let base_id = collection.collection.postman.as_ref().unwrap().variables["baseUrl"].clone();
    assert!(plan.variables.iter().any(|v| v.definition.id == base_id));
    fake.remaining.set(Some(1));
    assert!(storage::apply(root.path(), &project.data.id, &mut plan, true, &fake).is_err());
    assert_eq!(
        environments::definitions(root.path())
            .unwrap()
            .0
            .environments
            .len(),
        1
    );
    fake.remaining.set(None);
    storage::apply(root.path(), &project.data.id, &mut plan, true, &fake).unwrap();
    assert_eq!(
        fake.values.borrow()[&format!("{}/{base_id}/default", project.data.id)],
        "https://synthetic.invalid/api"
    );
    assert_eq!(
        fake.values.borrow()[&format!("{}/{base_id}/{}", project.data.id, env.id)],
        "https://production.synthetic.invalid/api"
    );
    assert_eq!(store.reload(&project.handle).unwrap().collections.len(), 1);
    assert_eq!(
        std::fs::read(
            root.path()
                .join("collections")
                .join(format!("{}.json", collection.collection.id))
        )
        .unwrap(),
        collection_before
    );
    let (defs, revision) = environments::definitions(root.path()).unwrap();
    let mut second = postman::environment(
        ENVIRONMENT,
        &project,
        defs,
        revision,
        &collection.collection.id,
    )
    .unwrap();
    assert_eq!(
        second.preview.environment_name.as_deref(),
        Some("Synthetic Production (import 2)")
    );
    let reads = fake.reads.get();
    storage::apply(root.path(), &project.data.id, &mut second, false, &fake).unwrap();
    assert_eq!(reads, fake.reads.get());
}

#[test]
fn postman_versions_duplicates_dynamic_scripts_and_unsafe_unsupported_fields_are_reported() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store
        .create(root.path(), "Synthetic invalid".into())
        .unwrap();
    let base: Value = serde_json::from_slice(COLLECTION).unwrap();
    for bad in [
        b"{\"info\":{},\"info\":{}}".as_slice(),
        b"info: yaml",
        b"[]",
    ] {
        assert!(
            postman::collection(bad, &project, Definitions::default(), "missing".into()).is_err()
        );
    }
    let mut version = base.clone();
    version["info"]["schema"] =
        "https://schema.getpostman.com/json/collection/v2.0.0/collection.json".into();
    assert!(postman::collection(
        &serde_json::to_vec(&version).unwrap(),
        &project,
        Definitions::default(),
        "missing".into()
    )
    .is_err());
    for request in [
        json!({"url":"https://user:password@synthetic.invalid/","method":"GET"}),
        json!({"url":"https://synthetic.invalid/","method":"POST","body":{"mode":"graphql"}}),
    ] {
        let mut source = base.clone();
        source["item"] = json!([{"name":"Blocked","request":request}]);
        let mut plan = postman::collection(
            &serde_json::to_vec(&source).unwrap(),
            &project,
            Definitions::default(),
            "missing".into(),
        )
        .unwrap();
        assert!(!plan.preview.blockers.is_empty());
        assert!(storage::apply(
            root.path(),
            &project.data.id,
            &mut plan,
            false,
            &Fake::default()
        )
        .is_err());
        assert!(!root.path().join("environments.fluxkit.json").exists());
    }
    let mut nested = base.clone();
    nested["variable"][0]["value"] = "{{other}}/api".into();
    let plan = postman::collection(
        &serde_json::to_vec(&nested).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    assert!(!plan.preview.blockers.is_empty());
    let mut unknown = base.clone();
    unknown["item"] =
        json!([{"name":"Missing","request":{"url":"{{unknown}}/path","method":"GET"}}]);
    let plan = postman::collection(
        &serde_json::to_vec(&unknown).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    assert!(plan.preview.blockers.is_empty());
    assert!(plan
        .collection
        .postman
        .as_ref()
        .unwrap()
        .variables
        .contains_key("unknown"));
    assert!(plan.variables.iter().any(|v| v.value.is_none()));
}

#[test]
fn environment_requires_explicit_live_collection_mapping_and_stale_previews_do_not_write() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store
        .create(root.path(), "Synthetic mappings".into())
        .unwrap();
    let mut plan = postman::collection(
        COLLECTION,
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    storage::apply(
        root.path(),
        &project.data.id,
        &mut plan,
        false,
        &Fake::default(),
    )
    .unwrap();
    let current = store.reload(&project.handle).unwrap();
    let (defs, rev) = environments::definitions(root.path()).unwrap();
    assert!(postman::environment(
        ENVIRONMENT,
        &current,
        defs.clone(),
        rev.clone(),
        &uuid::Uuid::new_v4().to_string()
    )
    .is_err());
    let mut globals: Value = serde_json::from_slice(ENVIRONMENT).unwrap();
    globals["_postman_variable_scope"] = "globals".into();
    assert!(postman::environment(
        &serde_json::to_vec(&globals).unwrap(),
        &current,
        defs.clone(),
        rev.clone(),
        &plan.collection.id
    )
    .is_err());
    let mut env =
        postman::environment(ENVIRONMENT, &current, defs, rev, &plan.collection.id).unwrap();
    let path = root.path().join("environments.fluxkit.json");
    let before = std::fs::read(&path).unwrap();
    let mut changed: Value = serde_json::from_slice(&before).unwrap();
    changed["environments"] = json!([{"id":uuid::Uuid::new_v4().to_string(),"name":"External"}]);
    std::fs::write(&path, serde_json::to_vec_pretty(&changed).unwrap()).unwrap();
    assert!(storage::apply(
        root.path(),
        &project.data.id,
        &mut env,
        true,
        &Fake::default()
    )
    .is_err());
    assert_eq!(
        store::load(root.path(), &project.handle)
            .unwrap()
            .collections
            .len(),
        1
    );
    assert_eq!(
        environments::definitions(root.path())
            .unwrap()
            .0
            .environments[0]
            .name,
        "External"
    );
}

#[test]
fn imported_environment_resolution_uses_own_scope_and_collection_default_fallback() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store
        .create(root.path(), "Synthetic resolution".into())
        .unwrap();
    let fake = Fake::default();
    let mut imported = postman::collection(
        COLLECTION,
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    storage::apply(root.path(), &project.data.id, &mut imported, true, &fake).unwrap();
    let project = store.reload(&project.handle).unwrap();
    let (defs, revision) = environments::definitions(root.path()).unwrap();
    let mut env = postman::environment(
        ENVIRONMENT,
        &project,
        defs.clone(),
        revision.clone(),
        &imported.collection.id,
    )
    .unwrap();
    let env_id = env.definitions.environments[0].id.clone();
    storage::apply(root.path(), &project.data.id, &mut env, true, &fake).unwrap();
    let request = &imported.collection.requests[0];
    assert_eq!(
        environments::resolve(
            &request.url,
            &project.data.id,
            &env.definitions,
            Some(&env_id),
            &fake
        )
        .unwrap(),
        "https://production.synthetic.invalid/api/products/a%2Fb"
    );
    assert_eq!(
        environments::resolve(
            &request.headers[0].value,
            &project.data.id,
            &env.definitions,
            Some(&env_id),
            &fake
        )
        .unwrap(),
        "synthetic-user"
    );
    assert_eq!(
        environments::resolve(
            &request.url,
            &project.data.id,
            &env.definitions,
            None,
            &fake
        )
        .unwrap(),
        "https://synthetic.invalid/api/products/a%2Fb"
    );
    let second = postman::collection(COLLECTION, &project, defs, revision).unwrap();
    assert_eq!(second.collection.name, "Synthetic Shop (import 2)");
    assert!(second.variables.iter().all(|v| !env
        .definitions
        .variables
        .iter()
        .any(|d| d.id == v.definition.id || d.name == v.definition.name)));
}

#[test]
fn additional_auth_and_body_shapes_are_explicit_and_private() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store
        .create(root.path(), "Synthetic shapes".into())
        .unwrap();
    let mut source: Value = serde_json::from_slice(COLLECTION).unwrap();
    source["item"] = json!([
        {"name":"Key","request":{"url":"https://synthetic.invalid/","method":"GET","auth":{"type":"apikey","apikey":[{"key":"key","value":"X-Key"},{"key":"value","value":"synthetic-private-key"},{"key":"in","value":"header"}]}}},
        {"name":"Manual OAuth","request":{"url":"https://synthetic.invalid/","method":"GET","auth":{"type":"oauth2","oauth2":[{"key":"grant_type","value":"authorization_code"},{"key":"accessToken","value":"synthetic-manual-token"}]}}},
        {"name":"File","request":{"url":"https://synthetic.invalid/","method":"PUT","body":{"mode":"file","file":{"src":"/private/never-read-synthetic"}}}},
        {"name":"Text","request":{"url":"https://synthetic.invalid/","method":"POST","body":{"mode":"raw","raw":"hello {{username}}"}}},
        {"name":"Disabled","request":{"url":"https://synthetic.invalid/","method":"POST","body":{"mode":"raw","raw":"omitted-body","disabled":true}}},
        {"name":"Short request","request":"https://synthetic.invalid/path"}
    ]);
    let plan = postman::collection(
        &serde_json::to_vec(&source).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    assert!(plan.preview.blockers.is_empty());
    assert!(matches!(
        plan.collection.requests[0].auth,
        Some(Auth::ApiKey { .. })
    ));
    assert!(matches!(
        plan.collection.requests[1].auth,
        Some(Auth::Bearer { .. })
    ));
    assert!(matches!(
        plan.collection.requests[2].body.kind,
        BodyKind::File
    ));
    assert!(plan.collection.requests[2].body.file.is_none());
    assert!(matches!(
        plan.collection.requests[3].body.kind,
        BodyKind::Text
    ));
    assert!(matches!(
        plan.collection.requests[4].body.kind,
        BodyKind::None
    ));
    let serialized = serde_json::to_string(&plan.collection).unwrap();
    for omitted in [
        "synthetic-private-key",
        "synthetic-manual-token",
        "/private/never-read-synthetic",
        "omitted-body",
    ] {
        assert!(!serialized.contains(omitted));
    }
}

#[test]
fn large_postman_mapping_bounds_preview_and_records_observed_work() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store
        .create(root.path(), "Synthetic performance".into())
        .unwrap();
    let source = json!({"info":{"name":"Large synthetic","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"variable":[{"key":"base","value":"https://synthetic.invalid"}],"item":(0..500).map(|i|json!({"name":format!("Request {i}"),"request":{"method":"GET","url":format!("{{{{base}}}}/items/{i}")}})).collect::<Vec<_>>()});
    let bytes = serde_json::to_vec(&source).unwrap();
    let started = std::time::Instant::now();
    let plan =
        postman::collection(&bytes, &project, Definitions::default(), "missing".into()).unwrap();
    println!(
        "Postman debug mapping: {} bytes, 500 requests, {} ms",
        bytes.len(),
        started.elapsed().as_millis()
    );
    assert_eq!(plan.preview.requests.len(), 200);
    assert_eq!(plan.preview.request_count, 500);
    assert_eq!(plan.variables.len(), 1);
}

#[test]
fn protocol_placeholders_and_raw_query_null_do_not_capture_existing_variables_or_drop_queries() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store
        .create(root.path(), "Synthetic protocol".into())
        .unwrap();
    let mut source: Value = serde_json::from_slice(COLLECTION).unwrap();
    source["variable"] = json!([{"key":"protocol","value":"https"}]);
    source["item"] = json!([{"name":"Protocol","request":{"method":"GET","url":{"raw":"{{protocol}}://synthetic.invalid/path?name=a%2Fb","query":null}}}]);
    let plan = postman::collection(
        &serde_json::to_vec(&source).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    let request = &plan.collection.requests[0];
    assert!(!request.url.contains("{{protocol}}"));
    assert!(request.url.starts_with("{{pm_"));
    assert_eq!(request.params.len(), 1);
    assert!(plan
        .variables
        .iter()
        .any(|v| v.value.as_deref() == Some("a/b")));
}

#[test]
fn unsupported_auth_dynamic_values_and_local_files_import_as_review_locked_requests() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store
        .create(root.path(), "Review migration".into())
        .unwrap();
    let source = json!({"info":{"name":"Review API","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"item":[
        {"name":"Dynamic A","request":{"method":"GET","url":"https://synthetic.invalid/{{$randomUUID}}"}},
        {"name":"Dynamic B","request":{"method":"GET","url":"https://synthetic.invalid/{{$randomUUID}}"}},
        {"name":"Unsupported auth","auth":{"type":"digest"},"item":[{"name":"Inherits unsupported auth","request":{"method":"GET","url":"https://synthetic.invalid/private"}}]},
        {"name":"Files","request":{"method":"POST","url":"https://synthetic.invalid/upload","body":{"mode":"formdata","formdata":[{"key":"file","type":"file","src":"/private/NEVER_OPEN_SOURCE_FILE"}]}}},
        {"name":"Supported sibling","request":{"method":"GET","url":"https://synthetic.invalid/ok"}}
    ]});
    let mut plan = postman::collection(
        &serde_json::to_vec(&source).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    assert!(plan.preview.blockers.is_empty());
    assert_eq!(plan.preview.review_count, 4);
    assert!(plan.collection.requests[..4]
        .iter()
        .all(|r| !r.import_issues.is_empty()));
    assert!(plan.collection.requests[4].import_issues.is_empty());
    assert_eq!(plan.collection.requests[3].body.fields.len(), 1);
    assert!(plan.collection.requests[3].body.fields[0].file.is_none());
    assert!(plan.collection.requests[3].body.fields[0]
        .value
        .starts_with("{{pm_"));
    assert!(!serde_json::to_string(&plan.collection)
        .unwrap()
        .contains("NEVER_OPEN_SOURCE_FILE"));
    storage::apply(
        root.path(),
        &project.data.id,
        &mut plan,
        false,
        &Fake::default(),
    )
    .unwrap();
    let project = store.reload(&project.handle).unwrap();
    assert_eq!(project.collections[0].data.requests.len(), 5);
    assert!(!project.collections[0].data.requests[0]
        .import_issues
        .is_empty());
}
