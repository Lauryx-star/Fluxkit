use super::{postman, storage, tests::Fake};
use crate::projects::{
    environments::{self, Definition, Definitions},
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
    assert_eq!(get.url, "{{baseUrl}}/products/a%2Fb");
    assert_eq!(get.params[0].value, "yes");
    assert_eq!(get.headers[0].value, "{{username}}");
    assert_eq!(get.headers[1].value, "synthetic-header");
    let form = &plan.collection.requests[3];
    assert_eq!(form.url, "https://synthetic.invalid/form");
    assert_eq!(form.params[0].value, "a/b");
    assert_eq!(form.headers[0].value, "synthetic-trace");
    assert_eq!(
        plan.collection.requests[4].url,
        "https://synthetic.invalid:8443/oauth"
    );
    let names: Vec<_> = plan
        .variables
        .iter()
        .map(|v| v.definition.name.as_str())
        .collect();
    for name in [
        "baseUrl",
        "token",
        "username",
        "password",
        "clientId",
        "clientSecret",
    ] {
        assert!(names.contains(&name), "{names:?}");
    }
    assert!(names.iter().all(|n| !n.starts_with("pm_")));
    assert!(plan.variables.iter().all(|v| !v.reused));
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
        "synthetic-client-secret",
        "SAVED_RESPONSE_MUST_NOT_PERSIST",
        "must never run",
    ] {
        assert!(!serialized.contains(private));
        assert!(!preview.contains(private));
    }
    assert!(serialized.contains("synthetic-body"));
    assert!(serialized.contains("synthetic-header"));
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
    let project = store.reload(&project.handle).unwrap();
    let (defs, revision) = environments::definitions(root.path()).unwrap();
    let mut second = postman::collection(COLLECTION, &project, defs.clone(), revision).unwrap();
    assert_eq!(second.collection.name, "Synthetic Shop (import 2)");
    // Source names matching existing project variables reuse their identity.
    let first = imported.collection.postman.as_ref().unwrap();
    assert_eq!(
        second.collection.postman.as_ref().unwrap().variables,
        first.variables
    );
    for source in ["baseUrl", "token", "username"] {
        let variable = second
            .variables
            .iter()
            .find(|v| v.definition.name == source)
            .unwrap();
        assert!(variable.reused);
        assert_eq!(variable.definition.id, first.variables[source]);
    }
    assert!(second
        .preview
        .variables
        .iter()
        .filter(|v| v.existing)
        .all(|v| !v.has_default));
    // Generated credentials never reuse; they get a numbered name instead.
    assert!(second
        .variables
        .iter()
        .any(|v| v.definition.name == "password_2" && !v.reused));
    assert_eq!(
        second.definitions.variables.len(),
        defs.variables.len() + second.variables.iter().filter(|v| !v.reused).count()
    );
    // An existing default that differs from the Postman default is neither an error nor
    // overwritten.
    let base_key = format!("{}/{}/default", project.data.id, first.variables["baseUrl"]);
    fake.values
        .borrow_mut()
        .insert(base_key.clone(), "https://edited.synthetic.invalid".into());
    storage::apply(root.path(), &project.data.id, &mut second, true, &fake).unwrap();
    assert_eq!(
        fake.values.borrow()[&base_key],
        "https://edited.synthetic.invalid"
    );
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
fn protocol_placeholders_keep_source_names_and_raw_query_null_does_not_drop_queries() {
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
    assert_eq!(request.url, "{{protocol}}://synthetic.invalid/path");
    assert_eq!(request.params.len(), 1);
    assert_eq!(request.params[0].value, "a/b");
    // The second variable is the fixture's root Bearer {{token}} without a collection value.
    let names: Vec<_> = plan
        .variables
        .iter()
        .map(|v| v.definition.name.as_str())
        .collect();
    assert_eq!(names, ["protocol", "token"]);
    assert_eq!(plan.variables[0].value.as_deref(), Some("https"));
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
    assert_eq!(
        plan.collection.requests[3].body.fields[0].value,
        "{{file_file}}"
    );
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

#[test]
fn literal_urls_stay_literal_and_path_parameters_without_values_need_variables() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Literals".into()).unwrap();
    let source = json!({"info":{"name":"Literal API","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"item":[
        {"name":"Test","request":{"method":"GET","url":{"raw":"https://localhost:5002/api/v0/info/Test","protocol":"https","host":["localhost"],"port":"5002","path":["api","v0","info","Test"]}}},
        {"name":"Version","request":{"method":"GET","url":{"raw":"{{baseUrlEpitopId}}/api/v0/info/version","host":["{{baseUrlEpitopId}}"],"path":["api","v0","info","version"]}}},
        {"name":"Item","request":{"method":"GET","url":{"raw":"https://localhost:5002/items/:id","variable":[{"key":"id"}]}}}
    ]});
    let plan = postman::collection(
        &serde_json::to_vec(&source).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    let urls: Vec<_> = plan
        .collection
        .requests
        .iter()
        .map(|r| r.url.as_str())
        .collect();
    assert_eq!(
        urls,
        [
            "https://localhost:5002/api/v0/info/Test",
            "{{baseUrlEpitopId}}/api/v0/info/version",
            "https://localhost:5002/items/{{id}}"
        ]
    );
    let names: Vec<_> = plan
        .preview
        .variables
        .iter()
        .map(|v| v.name.as_str())
        .collect();
    assert_eq!(names, ["baseUrlEpitopId", "id"]);
    assert_eq!(
        plan.preview.variables[0].source_name.as_deref(),
        Some("baseUrlEpitopId")
    );
}

#[test]
fn invalid_or_colliding_source_names_become_unique_valid_variable_names() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Names".into()).unwrap();
    let source = json!({"info":{"name":"Names","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
        "variable":[{"key":"base url","value":"https://synthetic.invalid"},{"key":"base_url","value":"x"}],
        "item":[{"name":"A","request":{"method":"GET","url":"{{base url}}/{{base_url}}/{{1st}}"}}]});
    let plan = postman::collection(
        &serde_json::to_vec(&source).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    assert_eq!(
        plan.collection.requests[0].url,
        "{{base_url}}/{{base_url_2}}/{{_1st}}"
    );
    plan.definitions.validate().unwrap();
}

#[test]
fn postman_preview_rename_creates_reuses_and_detaches_variables() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Rename".into()).unwrap();
    let existing = Definition {
        id: uuid::Uuid::new_v4().to_string(),
        name: "sharedToken".into(),
        secret: None,
    };
    let definitions = Definitions {
        variables: vec![existing.clone()],
        ..Definitions::default()
    };
    let mut plan =
        postman::collection(COLLECTION, &project, definitions, "missing".into()).unwrap();
    let token_id = plan.collection.postman.as_ref().unwrap().variables["token"].clone();
    let bearer = |plan: &super::Plan| match &plan.collection.auth {
        Some(Auth::Bearer { token }) => token.clone(),
        _ => panic!("bearer auth expected"),
    };
    assert_eq!(bearer(&plan), token_id);

    // A free name renames the new variable in place.
    super::rename::variable(&mut plan, "baseUrl", "epitopUrl").unwrap();
    assert!(plan.collection.requests[0]
        .url
        .starts_with("{{epitopUrl}}/"));
    assert!(plan.preview.requests[0].url.starts_with("{{epitopUrl}}/"));
    assert!(plan
        .definitions
        .variables
        .iter()
        .any(|v| v.name == "epitopUrl"));

    // An existing project variable name rebinds references to it; values stay untouched.
    super::rename::variable(&mut plan, "token", "sharedToken").unwrap();
    assert_eq!(bearer(&plan), existing.id);
    assert_eq!(
        plan.collection.postman.as_ref().unwrap().variables["token"],
        existing.id
    );
    assert!(!plan.definitions.variables.iter().any(|v| v.id == token_id));
    let shared = plan
        .variables
        .iter()
        .find(|v| v.definition.id == existing.id)
        .unwrap();
    assert!(shared.reused);
    assert!(plan
        .preview
        .variables
        .iter()
        .any(|v| v.name == "sharedToken" && v.existing && !v.has_default));

    // Renaming a reused variable detaches it into a new import-owned variable.
    super::rename::variable(&mut plan, "sharedToken", "ownToken").unwrap();
    let own = plan
        .variables
        .iter()
        .find(|v| v.definition.name == "ownToken")
        .unwrap();
    assert!(!own.reused);
    assert_ne!(own.definition.id, existing.id);
    assert_eq!(bearer(&plan), own.definition.id);
    assert!(plan
        .definitions
        .variables
        .iter()
        .any(|v| v.id == existing.id && v.name == "sharedToken"));

    // Another new import variable's name is rejected without changing the plan.
    let before = serde_json::to_vec(&plan.collection).unwrap();
    assert!(super::rename::variable(&mut plan, "ownToken", "username").is_err());
    assert!(super::rename::variable(&mut plan, "ownToken", "bad name").is_err());
    assert_eq!(serde_json::to_vec(&plan.collection).unwrap(), before);
    plan.definitions_saved = true;
    assert!(super::rename::variable(&mut plan, "ownToken", "other").is_err());
}

#[test]
fn only_literal_credentials_and_postman_secret_types_are_secret() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Classes".into()).unwrap();
    let mut source: Value = serde_json::from_slice(COLLECTION).unwrap();
    source["variable"][1]["type"] = "secret".into();
    let plan = postman::collection(
        &serde_json::to_vec(&source).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    let secret = |name: &str| {
        plan.definitions
            .variables
            .iter()
            .find(|v| v.name == name)
            .unwrap()
            .secret
    };
    // Postman type: secret, and literal auth credentials.
    for name in ["token", "password", "clientId", "clientSecret"] {
        assert_eq!(secret(name), Some(true), "{name}");
    }
    // Ordinary source variables and non-credential auth inputs.
    for name in ["baseUrl", "username", "scope"] {
        assert_eq!(secret(name), Some(false), "{name}");
    }
    assert!(plan
        .preview
        .variables
        .iter()
        .any(|v| v.name == "baseUrl" && !v.secret));
    assert!(plan.is_secret(&plan.collection.postman.as_ref().unwrap().variables["token"]));

    // Value slots: every new variable with a value, in the default slot.
    let slots: Vec<_> = plan.value_slots().collect();
    let base = &plan.collection.postman.as_ref().unwrap().variables["baseUrl"];
    assert!(slots.contains(&(base.as_str(), "default", "https://synthetic.invalid/api")));
    assert!(slots
        .iter()
        .any(|(_, _, value)| *value == "synthetic-password"));
}

#[test]
fn environment_import_classifies_new_keys_and_keeps_secret_values_out_of_ordinary_variables() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Env classes".into()).unwrap();
    let mut imported = postman::collection(
        COLLECTION,
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    storage::apply(
        root.path(),
        &project.data.id,
        &mut imported,
        false,
        &Fake::default(),
    )
    .unwrap();
    let project = store.reload(&project.handle).unwrap();
    let (defs, revision) = environments::definitions(root.path()).unwrap();
    let source = json!({"name":"Classes","_postman_variable_scope":"environment","values":[
        {"key":"baseUrl","value":"https://secret-typed.invalid","type":"secret","enabled":true},
        {"key":"apiPassword","value":"synthetic-env-secret","type":"secret","enabled":true},
        {"key":"region","value":"eu","type":"default","enabled":true}
    ]});
    let plan = postman::environment(
        &serde_json::to_vec(&source).unwrap(),
        &project,
        defs,
        revision,
        &imported.collection.id,
    )
    .unwrap();
    let find = |name: &str| {
        plan.variables
            .iter()
            .find(|v| v.definition.name == name)
            .unwrap()
    };
    // baseUrl is an ordinary collection variable: its secret-typed value is not adopted.
    assert!(find("baseUrl").value.is_none());
    assert_eq!(find("apiPassword").definition.secret, Some(true));
    assert_eq!(find("region").definition.secret, Some(false));
    let env = plan.definitions.environments[0].id.as_str();
    let slots: Vec<_> = plan.value_slots().collect();
    assert!(slots.iter().all(|(_, environment, _)| *environment == env));
    assert_eq!(slots.len(), 2);
}
