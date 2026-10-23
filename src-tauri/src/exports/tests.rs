use super::{mapping, storage, Target};
use crate::projects::{environments::Definitions, model::*, store::ProjectStore};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tempfile::tempdir;
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn request(name: &str, url: &str) -> Request {
    Request {
        import_issues: vec![],
        id: id(),
        name: name.into(),
        auth: None,
        folder_id: None,
        method: "GET".into(),
        url: url.into(),
        params: vec![],
        headers: vec![],
        body: Body {
            kind: BodyKind::None,
            text: String::new(),
            fields: vec![],
            file: None,
        },
    }
}
fn collection() -> Collection {
    Collection {
        schema_version: 1,
        id: id(),
        name: "Synthetic API".into(),
        auth: None,
        open_api: None,
        postman: None,
        folders: vec![],
        requests: vec![request(
            "Get item",
            "https://synthetic.invalid/items/{{itemId}}?token=SECRET_QUERY",
        )],
    }
}
fn document(plan: &super::Plan) -> Value {
    crate::imports::document::parse_bounded(plan.documents[0].yaml.as_bytes()).unwrap()
}
fn output(plan: &super::Plan, name: &str) {
    if let Some(path) = std::env::var_os("FLUXKIT_EXPORT_FIXTURE_DIR") {
        let path = std::path::Path::new(&path);
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join(name), &plan.documents[0].yaml).unwrap();
    }
}
#[test]
fn saved_export_excludes_payload_values_and_describes_inherited_authentication() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let p = store
        .create(root.path(), "Synthetic project".into())
        .unwrap();
    let mut c = collection();
    c.auth = Some(Auth::Basic {
        username: id(),
        password: id(),
    });
    let r = &mut c.requests[0];
    r.headers = vec![
        Entry {
            id: 1,
            enabled: true,
            key: "X-Example".into(),
            value: "SECRET_HEADER".into(),
        },
        Entry {
            id: 2,
            enabled: true,
            key: "Cookie".into(),
            value: "SECRET_COOKIE".into(),
        },
    ];
    r.body = Body {
        kind: BodyKind::Json,
        text: "{\"secret\":\"SECRET_BODY\"}".into(),
        fields: vec![],
        file: None,
    };
    store.save_collection(&p.handle, c.clone(), None).unwrap();
    let p = store.reload(&p.handle).unwrap();
    let plan = mapping::plan(
        p,
        Definitions::default(),
        "missing".into(),
        Target::Collection {
            collection: c.id.clone(),
        },
    )
    .unwrap();
    assert!(
        plan.preview.blockers.is_empty(),
        "{:?}",
        plan.preview.blockers
    );
    let doc = document(&plan);
    assert_eq!(doc["openapi"], "3.1.0");
    let op = &doc["paths"]["/items/{itemId}"]["get"];
    assert_eq!(op["servers"][0]["url"], "https://synthetic.invalid");
    assert_eq!(
        doc["components"]["securitySchemes"]["auth1"]["scheme"],
        "basic"
    );
    assert_eq!(op["parameters"][0]["required"], true);
    assert!(op["requestBody"]["content"]["application/json"]["schema"].is_object());
    for omitted in [
        "SECRET_QUERY",
        "SECRET_HEADER",
        "SECRET_COOKIE",
        "SECRET_BODY",
    ] {
        assert!(!plan.documents[0].yaml.contains(omitted));
    }
    assert_eq!(plan.preview.files[0].operations, 1);
    output(&plan, "basic.yml");
}
#[test]
fn request_folder_subtree_and_project_selections_use_saved_identities() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let p = store.create(root.path(), "Scopes".into()).unwrap();
    let mut c = collection();
    let parent = id();
    let child = id();
    c.folders = vec![
        Folder {
            id: parent.clone(),
            name: "Parent".into(),
            auth: Some(Auth::Bearer { token: id() }),
            parent_id: None,
        },
        Folder {
            id: child.clone(),
            name: "Child".into(),
            auth: Some(Auth::None),
            parent_id: Some(parent.clone()),
        },
    ];
    c.requests[0].folder_id = Some(parent.clone());
    let mut nested = request("Nested", "{{baseUrl}}/nested");
    nested.folder_id = Some(child.clone());
    c.requests.push(nested.clone());
    c.requests
        .push(request("Other", "https://synthetic.invalid/other"));
    store.save_collection(&p.handle, c.clone(), None).unwrap();
    let mut c2 = collection();
    c2.name = "Another".into();
    store.save_collection(&p.handle, c2, None).unwrap();
    let p = store.reload(&p.handle).unwrap();
    let single = mapping::plan(
        p.clone(),
        Definitions::default(),
        "missing".into(),
        Target::Request {
            collection: c.id.clone(),
            request: nested.id.clone(),
        },
    )
    .unwrap();
    assert_eq!(single.preview.files[0].operations, 1);
    assert_eq!(
        document(&single)["paths"]["/nested"]["get"]["security"],
        json!([])
    );
    let subtree = mapping::plan(
        p.clone(),
        Definitions::default(),
        "missing".into(),
        Target::Folder {
            collection: c.id.clone(),
            folder: parent,
        },
    )
    .unwrap();
    assert_eq!(subtree.preview.files[0].operations, 2);
    assert!(document(&subtree)["paths"].get("/other").is_none());
    assert_eq!(
        document(&subtree)["components"]["securitySchemes"]["auth1"]["scheme"],
        "bearer"
    );
    output(&subtree, "subtree.yml");
    let all = mapping::plan(p, Definitions::default(), "missing".into(), Target::Project).unwrap();
    assert!(all.preview.directory);
    assert_eq!(all.documents.len(), 2);
    let destination = tempdir().unwrap();
    storage::write(&all, destination.path()).unwrap();
    let folder = destination.path().join(&all.preview.folder_name);
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2);
    assert!(storage::write(&all, destination.path()).is_err());
    assert!(mapping::plan(
        single.snapshot.clone(),
        Definitions::default(),
        "missing".into(),
        Target::Request {
            collection: c.id,
            request: id()
        }
    )
    .is_err());
}
#[test]
fn collisions_unknown_url_structures_and_unsupported_methods_block_without_writing() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let p = store.create(root.path(), "Conflicts".into()).unwrap();
    for mode in 0..5 {
        let mut c = collection();
        match mode {
            0 => c.requests.push(request(
                "Duplicate",
                "https://another.invalid/items/{{otherId}}",
            )),
            1 => {
                let mut second = request("Equivalent", "https://synthetic.invalid/items/{{name}}");
                second.method = "POST".into();
                c.requests.push(second);
            }
            2 => c.requests[0].url = "{{wholeUrl}}".into(),
            3 => c.requests[0].url = "https://user:SECRET_PASSWORD@synthetic.invalid/path".into(),
            _ => c.requests[0].method = "CONNECT".into(),
        }
        let saved = store.save_collection(&p.handle, c.clone(), None).unwrap();
        let current = store.reload(&p.handle).unwrap();
        let plan = mapping::plan(
            current,
            Definitions::default(),
            "missing".into(),
            Target::Collection {
                collection: c.id.clone(),
            },
        )
        .unwrap();
        assert!(!plan.preview.blockers.is_empty());
        let dest = root.path().join(format!("blocked-{mode}.yml"));
        assert!(storage::write(&plan, &dest).is_err());
        assert!(!dest.exists());
        assert!(!plan.documents[0].yaml.contains("SECRET_PASSWORD"));
        assert!(!saved.revision.is_empty());
    }
}
#[test]
fn metadata_schemas_upgrade_and_original_response_descriptions_survive_renames() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let p = store.create(root.path(), "Metadata".into()).unwrap();
    let mut c = collection();
    c.requests[0].name = "Renamed operation".into();
    c.requests[0].url = "{{baseUrl}}/items/{{itemId}}".into();
    let source = json!({"openapi":"3.0.3","info":{"title":"Original","version":"1"},"paths":{"/old":{"get":{"description":"Saved API description","responses":{"200":{"description":"Known API shape","content":{"application/json":{"schema":{"$ref":"#/components/schemas/Item"},"example":"SECRET_EXAMPLE"}}}}}}},"components":{"schemas":{"Item":{"type":"object","nullable":true,"properties":{"value":{"type":"number","minimum":0,"exclusiveMinimum":true},"nullable":{"type":"string","nullable":true}},"example":"SECRET_EXAMPLE"}}}});
    c.open_api = Some(OpenApiMetadata {
        version: "3.0.3".into(),
        document: source,
        operations: BTreeMap::from([(c.requests[0].id.clone(), "#/paths/~1old/get".into())]),
    });
    store.save_collection(&p.handle, c.clone(), None).unwrap();
    let p = store.reload(&p.handle).unwrap();
    let plan = mapping::plan(
        p,
        Definitions::default(),
        "missing".into(),
        Target::Collection { collection: c.id },
    )
    .unwrap();
    assert!(
        plan.preview.blockers.is_empty(),
        "{:?}",
        plan.preview.blockers
    );
    let doc = document(&plan);
    assert_eq!(
        doc["components"]["schemas"]["Item"]["type"],
        json!(["object", "null"])
    );
    assert_eq!(
        doc["components"]["schemas"]["Item"]["properties"]["value"]["exclusiveMinimum"],
        0
    );
    assert_eq!(
        doc["components"]["schemas"]["Item"]["properties"]["nullable"]["type"],
        json!(["string", "null"])
    );
    assert_eq!(
        doc["paths"]["/items/{itemId}"]["get"]["responses"]["200"]["description"],
        "Known API shape"
    );
    assert!(!plan.documents[0].yaml.contains("SECRET_EXAMPLE"));
    output(&plan, "upgraded.yml");
}
#[test]
fn new_files_are_atomic_never_clobber_and_stale_project_state_is_rejected() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let p = store.create(root.path(), "Writes".into()).unwrap();
    let c = collection();
    store.save_collection(&p.handle, c.clone(), None).unwrap();
    let p = store.reload(&p.handle).unwrap();
    let plan = mapping::plan(
        p.clone(),
        Definitions::default(),
        "missing".into(),
        Target::Collection { collection: c.id },
    )
    .unwrap();
    storage::current(&store, &plan).unwrap();
    let path = root.path().join("export.yml");
    storage::write(&plan, &path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(storage::write(&plan, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    store
        .rename(&p.handle, "Changed".into(), &p.revision)
        .unwrap();
    assert!(storage::current(&store, &plan).is_err());
    #[cfg(unix)]
    {
        let external = tempdir().unwrap();
        let link = root.path().join("linked.yml");
        std::os::unix::fs::symlink(external.path().join("outside.yml"), &link).unwrap();
        assert!(storage::write(&plan, &link).is_err());
        assert!(!external.path().join("outside.yml").exists());
    }
}
#[test]
fn manual_auth_precedence_and_oauth_custom_login_omissions_are_visible() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let p = store.create(root.path(), "Auth".into()).unwrap();
    let mut c = collection();
    c.auth = Some(Auth::Bearer { token: id() });
    c.requests[0].headers.push(Entry {
        id: 1,
        key: "Authorization".into(),
        value: "SECRET_MANUAL".into(),
        enabled: true,
    });
    store.save_collection(&p.handle, c.clone(), None).unwrap();
    let current = store.reload(&p.handle).unwrap();
    let plan = mapping::plan(
        current,
        Definitions::default(),
        "missing".into(),
        Target::Collection { collection: c.id },
    )
    .unwrap();
    let doc = document(&plan);
    assert_eq!(
        doc["paths"]["/items/{itemId}"]["get"]["security"],
        json!([])
    );
    assert!(!plan.documents[0].yaml.contains("SECRET_MANUAL"));
    assert!(plan
        .preview
        .warnings
        .iter()
        .any(|s| s.contains("override inherited")));
    output(&plan, "manual.yml");
}
#[test]
fn large_saved_export_is_bounded_deterministic_and_measured() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let p = store.create(root.path(), "Large".into()).unwrap();
    let mut c = collection();
    c.requests = (0..500)
        .map(|i| {
            request(
                &format!("Operation {i}"),
                &format!("https://synthetic.invalid/items/{i}"),
            )
        })
        .collect();
    store.save_collection(&p.handle, c.clone(), None).unwrap();
    let p = store.reload(&p.handle).unwrap();
    let start = std::time::Instant::now();
    let plan = mapping::plan(
        p.clone(),
        Definitions::default(),
        "missing".into(),
        Target::Collection {
            collection: c.id.clone(),
        },
    )
    .unwrap();
    println!(
        "Export debug mapping: 500 operations, {} YAML bytes, {} ms",
        plan.documents[0].yaml.len(),
        start.elapsed().as_millis()
    );
    assert_eq!(plan.preview.files[0].operations, 500);
    let second = mapping::plan(
        p,
        Definitions::default(),
        "missing".into(),
        Target::Collection { collection: c.id },
    )
    .unwrap();
    assert_eq!(plan.documents[0].yaml, second.documents[0].yaml);
    output(&plan, "large.yml");
}

#[test]
fn oauth_api_keys_and_custom_login_export_descriptions_without_token_inputs() {
    use crate::projects::token_config::*;
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let p = store.create(root.path(), "Token auth".into()).unwrap();
    let mut c = collection();
    let expression = Composition {
        parts: vec![Part::Literal {
            text: "SECRET_TOKEN_INPUT".into(),
        }],
        separator: String::new(),
        prefix: String::new(),
        suffix: String::new(),
        base64: false,
    };
    c.requests[0].auth = Some(Auth::Oauth2 {
        config: Box::new(OAuth2 {
            token_url: expression.clone(),
            client_id: id(),
            client_secret: id(),
            scope: expression.clone(),
            client_authentication: ClientAuthentication::Basic,
        }),
    });
    let mut api = request("API key", "https://synthetic.invalid/key");
    api.auth = Some(Auth::ApiKey {
        name: "X-API-Key".into(),
        value: id(),
        placement: KeyPlacement::Header,
    });
    c.requests.push(api);
    let mut custom = request("Custom", "https://synthetic.invalid/custom");
    custom.auth = Some(Auth::CustomLogin {
        config: Box::new(CustomLogin {
            login: LoginRequest {
                url: expression,
                method: "POST".into(),
                params: vec![],
                headers: vec![],
                body: LoginBody::None,
                success_statuses: vec![200],
                token: Extraction::Text,
                renewal_token: None,
                expires_in: None,
                lifetime_seconds: None,
            },
            renewal: None,
            placement: TokenPlacement {
                name: "Authorization".into(),
                placement: KeyPlacement::Header,
                prefix: "Bearer ".into(),
            },
        }),
    });
    c.requests.push(custom);
    store.save_collection(&p.handle, c.clone(), None).unwrap();
    let p = store.reload(&p.handle).unwrap();
    let plan = mapping::plan(
        p,
        Definitions::default(),
        "missing".into(),
        Target::Collection { collection: c.id },
    )
    .unwrap();
    assert!(plan.preview.blockers.is_empty());
    assert!(!plan.documents[0].yaml.contains("SECRET_TOKEN_INPUT"));
    let doc = document(&plan);
    assert_eq!(
        doc["components"]["securitySchemes"]["auth1"]["flows"]["clientCredentials"]["tokenUrl"],
        "https://example.invalid/oauth/token"
    );
    assert_eq!(doc["paths"]["/custom"]["get"]["security"], json!([]));
    assert!(plan
        .preview
        .warnings
        .iter()
        .any(|w| w.contains("Custom Login")));
    output(&plan, "token-auth.yml");
}
#[test]
fn missing_source_references_and_invalid_responses_are_visible_blockers() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let p = store
        .create(root.path(), "Invalid metadata".into())
        .unwrap();
    for response in [
        json!({"200":{"$ref":"#/components/responses/Missing"}}),
        json!({"200":{"content":{}}}),
        json!({"not-a-code":{"description":"Invalid status"}}),
    ] {
        let mut c = collection();
        c.open_api = Some(OpenApiMetadata {
            version: "3.1.0".into(),
            document: json!({"openapi":"3.1.0","paths":{"/old":{"get":{"responses":response}}}}),
            operations: BTreeMap::from([(c.requests[0].id.clone(), "#/paths/~1old/get".into())]),
        });
        store.save_collection(&p.handle, c.clone(), None).unwrap();
        let p = store.reload(&p.handle).unwrap();
        let plan = mapping::plan(
            p,
            Definitions::default(),
            "missing".into(),
            Target::Collection { collection: c.id },
        )
        .unwrap();
        assert!(!plan.preview.blockers.is_empty());
    }
}
