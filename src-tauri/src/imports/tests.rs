use super::{document, mapping, storage};
use crate::projects::{
    environments::{self, Definitions, Values},
    model::{Auth, BodyKind},
    store::ProjectStore,
};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};
use tempfile::tempdir;
const YAML: &[u8] = include_bytes!("../../tests/fixtures/openapi/pets-3.0.yaml");
const JSON: &[u8] = include_bytes!("../../tests/fixtures/openapi/items-3.1.json");
#[derive(Default)]
pub(super) struct Fake {
    pub(super) values: RefCell<HashMap<String, String>>,
    pub(super) remaining: Cell<Option<usize>>,
    pub(super) reads: Cell<usize>,
}
impl Values for Fake {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        self.reads.set(self.reads.get() + 1);
        Ok(self.values.borrow().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        if let Some(left) = self.remaining.get() {
            if left == 0 {
                return Err("synthetic failure".into());
            }
            self.remaining.set(Some(left - 1));
        }
        self.values.borrow_mut().insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        self.values.borrow_mut().remove(key);
        Ok(())
    }
}
#[test]
fn supported_versions_json_yaml_refs_and_metadata_are_mapped_without_writes() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Import target".into()).unwrap();
    for version in [
        "3.0.0", "3.0.1", "3.0.2", "3.0.3", "3.0.4", "3.1.0", "3.1.1", "3.1.2",
    ] {
        let mut value = document::parse(JSON).unwrap();
        value["openapi"] = version.into();
        let plan = mapping::plan(
            &serde_json::to_vec(&value).unwrap(),
            &project,
            Definitions::default(),
            "missing".into(),
        )
        .unwrap();
        assert_eq!(plan.preview.version, version);
        assert!(plan.preview.blockers.is_empty());
    }
    let plan = mapping::plan(YAML, &project, Definitions::default(), "missing".into()).unwrap();
    assert_eq!(plan.preview.request_count, 2);
    assert_eq!(plan.collection.folders.len(), 1);
    let get = plan
        .collection
        .requests
        .iter()
        .find(|r| r.method == "GET")
        .unwrap();
    assert!(get.url.starts_with("{{baseUrl}}"));
    assert!(get.url.ends_with("/pets/a%2Fb"));
    assert_eq!(get.params.len(), 1);
    assert_eq!(get.params[0].value, "synthetic search");
    assert!(plan.variables.iter().all(
        |v| !v.definition.name.starts_with("query_") && !v.definition.name.starts_with("path_")
    ));
    assert!(matches!(get.auth, Some(Auth::Bearer { .. })));
    let post = plan
        .collection
        .requests
        .iter()
        .find(|r| r.method == "POST")
        .unwrap();
    assert!(matches!(post.auth, Some(Auth::None)));
    assert!(matches!(post.body.kind, BodyKind::Json));
    assert!(post.body.text.contains("Synthetic pet"));
    let metadata = &plan.collection.open_api.as_ref().unwrap().document;
    assert!(metadata
        .pointer("/components/schemas/Pet/properties/default")
        .is_some());
    assert_eq!(
        metadata
            .pointer("/components/schemas/Pet/properties/parent/$ref")
            .and_then(Value::as_str),
        Some("#/components/schemas/Pet")
    );
    assert!(metadata.get("servers").is_none());
    assert!(!serde_json::to_string(metadata)
        .unwrap()
        .contains("synthetic search"));
    assert_eq!(
        std::fs::read_dir(root.path().join("collections"))
            .unwrap()
            .count(),
        0
    );
    assert!(!root.path().join("environments.fluxkit.json").exists());
}
#[test]
fn invalid_external_cyclic_and_hostile_sources_are_rejected_without_source_details() {
    for version in ["2.0", "3.2.0", "4.0.0", "3.1", "3.1.bad"] {
        let mut value = document::parse(JSON).unwrap();
        value["openapi"] = version.into();
        assert!(document::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    let mut value = document::parse(JSON).unwrap();
    for reference in [
        "https://private.example.test/schema.json",
        "../private.yaml#/Secret",
        "#/missing",
        "#/components/schemas/a%2Fb",
    ] {
        value["components"]["schemas"]["bad"] = json!({"$ref":reference});
        let error = document::parse(&serde_json::to_vec(&value).unwrap()).unwrap_err();
        assert!(!error.contains("private.example"));
    }
    for source in [
        "openapi: 3.1.2\nopenapi: 3.0.4",
        "!include /private/secret",
        "---\nopenapi: 3.1.2\n---\nopenapi: 3.1.2",
        "{\"openapi\":\"3.1.2\",\"openapi\":\"3.0.4\"}",
    ] {
        assert!(document::parse(source.as_bytes()).is_err());
    }
    assert!(document::parse(&vec![b' '; document::INPUT_LIMIT + 1]).is_err());
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Target".into()).unwrap();
    let cyclic = json!({"openapi":"3.1.2","info":{"title":"Cycle","version":"1"},"paths":{"/x":{"parameters":[{"$ref":"#/components/parameters/X"}],"get":{"responses":{"200":{"description":"ok"}}}}},"components":{"parameters":{"X":{"$ref":"#/components/parameters/X"}}}});
    assert!(mapping::plan(
        &serde_json::to_vec(&cyclic).unwrap(),
        &project,
        Definitions::default(),
        "missing".into()
    )
    .is_err());
}
#[test]
fn import_adds_new_owned_values_and_preserves_existing_data_and_schemas() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Target".into()).unwrap();
    let values = Fake::default();
    let mut plan = mapping::plan(JSON, &project, Definitions::default(), "missing".into()).unwrap();
    assert!(plan.preview.blockers.is_empty());
    assert!(matches!(
        plan.collection.requests[0].body.kind,
        BodyKind::UrlEncoded
    ));
    storage::apply(root.path(), &project.data.id, &mut plan, true, &values).unwrap();
    let reloaded = store.reload(&project.handle).unwrap();
    assert_eq!(reloaded.collections.len(), 1);
    assert!(reloaded.collections[0]
        .data
        .open_api
        .as_ref()
        .unwrap()
        .document
        .pointer("/components/schemas/a~1b~0c/const/default")
        .is_some());
    let files = std::fs::read_to_string(root.path().join("environments.fluxkit.json")).unwrap();
    assert!(!files.contains("synthetic-item"));
    assert!(values
        .values
        .borrow()
        .values()
        .all(|v| v != "synthetic-item"));
    assert_eq!(
        reloaded.collections[0].data.requests[0].body.fields[0].value,
        "synthetic-item"
    );
    let (definitions, revision) = environments::definitions(root.path()).unwrap();
    let duplicate = mapping::plan(JSON, &reloaded, definitions, revision).unwrap();
    assert_eq!(
        duplicate.preview.collection_name,
        "Fixture Items (import 2)"
    );
    assert!(duplicate.variables.iter().all(|v| !plan
        .variables
        .iter()
        .any(|old| old.definition.id == v.definition.id)));
    assert!(storage::apply(root.path(), &project.data.id, &mut plan, true, &values).is_err());
    assert_eq!(store.reload(&project.handle).unwrap().collections.len(), 1);
}
#[test]
fn credential_failure_is_recoverable_and_retries_do_not_duplicate_definitions() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Target".into()).unwrap();
    let values = Fake::default();
    values.remaining.set(Some(1));
    let mut source = document::parse(JSON).unwrap();
    source["components"]["securitySchemes"]["key"] = json!({"type":"oauth2","flows":{"clientCredentials":{"tokenUrl":"https://synthetic.invalid/token","scopes":{}}}});
    let mut plan = mapping::plan(
        &serde_json::to_vec(&source).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    let error =
        storage::apply(root.path(), &project.data.id, &mut plan, true, &values).unwrap_err();
    assert!(error.contains("remain visible"));
    assert!(plan.definitions_saved);
    assert_eq!(store.reload(&project.handle).unwrap().collections.len(), 0);
    let count = environments::definitions(root.path())
        .unwrap()
        .0
        .variables
        .len();
    assert_eq!(count, plan.variables.len());
    assert_eq!(values.values.borrow().len(), 1);
    values.remaining.set(None);
    storage::apply(root.path(), &project.data.id, &mut plan, true, &values).unwrap();
    assert_eq!(
        environments::definitions(root.path())
            .unwrap()
            .0
            .variables
            .len(),
        count
    );
    assert_eq!(store.reload(&project.handle).unwrap().collections.len(), 1);
}
#[test]
fn definitions_only_import_never_touches_the_credential_store_and_stale_preview_writes_nothing() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Target".into()).unwrap();
    let values = Fake::default();
    values.remaining.set(Some(0));
    let mut plan = mapping::plan(JSON, &project, Definitions::default(), "missing".into()).unwrap();
    let original = std::fs::read(root.path().join("project.fluxkit.json")).unwrap();
    store
        .rename(
            &project.handle,
            "Changed externally".into(),
            &project.revision,
        )
        .unwrap();
    assert!(
        storage::apply(root.path(), &project.data.id, &mut plan, false, &values)
            .unwrap_err()
            .contains("changed after preview")
    );
    assert!(!root.path().join("environments.fluxkit.json").exists());
    let current = store.reload(&project.handle).unwrap();
    let mut fresh =
        mapping::plan(JSON, &current, Definitions::default(), "missing".into()).unwrap();
    storage::apply(root.path(), &current.data.id, &mut fresh, false, &values).unwrap();
    assert!(values.values.borrow().is_empty());
    assert_eq!(values.reads.get(), 0);
    assert_ne!(
        original,
        std::fs::read(root.path().join("project.fluxkit.json")).unwrap()
    );
}
#[test]
fn unsupported_security_and_serialization_are_visible_blockers() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Target".into()).unwrap();
    let mut source = document::parse(JSON).unwrap();
    source["paths"]["/items"]["post"]["security"] = json!([{"key":[],"other":[]}]);
    let mut plan = mapping::plan(
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
    source["paths"]["/items"]["post"]["security"] = json!([]);
    source["paths"]["/items"]["post"]["parameters"] =
        json!([{"name":"search","in":"query","schema":{"type":"array"}}]);
    let plan = mapping::plan(
        &serde_json::to_vec(&source).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    assert!(plan
        .preview
        .blockers
        .iter()
        .any(|s| s.contains("Array/object")));
}
#[test]
fn oauth_client_credentials_and_reference_pointer_escapes_are_supported() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Target".into()).unwrap();
    let mut source = document::parse(JSON).unwrap();
    source["components"]["securitySchemes"]["key"] = json!({"type":"oauth2","flows":{"clientCredentials":{"tokenUrl":"https://fixture.example.test/token","scopes":{"read":"Read"}}}});
    source["paths"]["/items"]["post"]["security"] = json!([{"key":["read"]}]);
    source["paths"]["/items"]["post"]["responses"]["201"]["content"] =
        json!({"application/json":{"schema":{"$ref":"#/components/schemas/a~1b~0c"}}});
    let plan = mapping::plan(
        &serde_json::to_vec(&source).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    assert!(plan.preview.blockers.is_empty());
    assert!(matches!(
        plan.collection.requests[0].auth,
        Some(Auth::Oauth2 { .. })
    ));
}
#[cfg(unix)]
#[test]
fn managed_symlinks_never_enable_import_writes_outside_the_project() {
    use std::os::unix::fs::symlink;
    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Target".into()).unwrap();
    let mut plan = mapping::plan(JSON, &project, Definitions::default(), "missing".into()).unwrap();
    std::fs::remove_dir(root.path().join("collections")).unwrap();
    symlink(outside.path(), root.path().join("collections")).unwrap();
    assert!(storage::apply(
        root.path(),
        &project.data.id,
        &mut plan,
        false,
        &Fake::default()
    )
    .is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn imported_literals_cannot_capture_existing_project_placeholders() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Target".into()).unwrap();
    let mut source = document::parse(JSON).unwrap();
    source["paths"]["/items"]["post"]["parameters"] = json!([{"name":"{{existingSecret}}","in":"query","required":true,"schema":{"type":"string"}}]);
    source["paths"]["/items"]["post"]["requestBody"] =
        json!({"content":{"application/json":{"example":{"value":"{{existingSecret}}"}}}});
    let plan = mapping::plan(
        &serde_json::to_vec(&source).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    assert!(plan
        .preview
        .blockers
        .iter()
        .any(|s| s.contains("must not bind existing")));
    assert!(plan.collection.requests[0].body.text.is_empty());
    assert!(plan
        .preview
        .warnings
        .iter()
        .any(|s| s.contains("body example containing Fluxkit")));
}
#[test]
fn parameter_literals_keep_optional_values_and_review_missing_or_unsafe_required_fields() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Target".into()).unwrap();
    let source = json!({
        "openapi":"3.1.0", "info":{"title":"Literal values", "version":"1"},
        "servers":[{"url":"https://example.invalid"}],
        "paths":{"/items":{"post":{
            "responses":{"200":{"description":"OK"}},
            "parameters":[
                {"in":"query","name":"optional","schema":{"type":"string","default":"literal"}},
                {"in":"query","name":"empty","required":true,"example":""},
                {"in":"query","name":"missing","required":true,"schema":{"type":"string"}},
                {"in":"header","name":"X-Unsafe","required":true,"example":"{{existingSecret}}"}
            ],
            "requestBody":{"content":{"application/x-www-form-urlencoded":{"schema":{
                "type":"object", "required":["unsafe", "missing"],
                "properties":{
                    "unsafe":{"type":"string","default":"{{existingSecret}}"},
                    "missing":{"type":"string"},
                    "optional":{"type":"string","default":"literal form"}
                }
            }}}}
        }}}
    });
    let plan = mapping::plan(
        &serde_json::to_vec(&source).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    assert!(plan.preview.blockers.is_empty());
    assert_eq!(plan.variables.len(), 1);
    assert_eq!(plan.preview.review_count, 1);
    let request = &plan.collection.requests[0];
    let optional = request
        .params
        .iter()
        .find(|row| row.key == "optional")
        .unwrap();
    assert!(!optional.enabled);
    assert_eq!(optional.value, "literal");
    assert_eq!(request.headers[0].value, "");
    assert!(request
        .import_issues
        .iter()
        .any(|note| note.contains("required query parameter: missing")));
    assert!(!request
        .import_issues
        .iter()
        .any(|note| note.contains("query parameter: empty")));
    assert!(request
        .body
        .fields
        .iter()
        .filter(|row| row.key != "optional")
        .all(|row| row.value.is_empty()));
    assert!(request
        .body
        .fields
        .iter()
        .any(|row| row.key == "optional" && row.value == "literal form" && !row.enabled));
    let encoded = serde_json::to_string(&plan.collection).unwrap();
    assert!(!encoded.contains("{{existingSecret}}"));
}

#[test]
fn escaped_reference_uris_and_schema_property_names_preserve_meaning() {
    let mut source = document::parse(JSON).unwrap();
    source["paths"]["/items"]["post"]["responses"]["201"]["content"] =
        json!({"application/json":{"schema":{"$ref":"#%2Fcomponents%2Fschemas%2Fa~1b~0c"}}});
    assert!(document::parse(&serde_json::to_vec(&source).unwrap()).is_ok());
    let schema = json!({"type":"object","properties":{"examples":{"type":"string","default":"private-default"},"servers":{"type":"array"},"x-custom":{"type":"string"}},"const":{"example":"literal constraint"}});
    let metadata = document::metadata(&schema);
    assert!(metadata.pointer("/properties/examples/type").is_some());
    assert!(metadata.pointer("/properties/examples/default").is_none());
    assert!(metadata.pointer("/properties/x-custom").is_some());
    assert_eq!(metadata["const"]["example"], "literal constraint");
}

#[test]
fn large_import_bounds_preview_rendering_and_reports_mapping_measurement() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Target".into()).unwrap();
    let mut source = document::parse(JSON).unwrap();
    let mut paths = serde_json::Map::new();
    for i in 0..500 {
        paths.insert(
            format!("/items/{i}"),
            json!({"get":{"responses":{"200":{"description":"Synthetic response"}},"security":[]}}),
        );
    }
    source["paths"] = Value::Object(paths);
    let bytes = serde_json::to_vec(&source).unwrap();
    let start = std::time::Instant::now();
    let plan = mapping::plan(&bytes, &project, Definitions::default(), "missing".into()).unwrap();
    let elapsed = start.elapsed();
    assert_eq!(plan.preview.request_count, 500);
    assert_eq!(plan.preview.requests.len(), 200);
    assert_eq!(plan.variables.len(), 1);
    println!(
        "Synthetic OpenAPI mapping: {} input bytes, 500 operations, {} ms (debug test build).",
        bytes.len(),
        elapsed.as_millis()
    );
}

#[test]
fn a_locally_edited_import_value_is_not_overwritten_on_retry() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Target".into()).unwrap();
    let values = Fake::default();
    values.remaining.set(Some(0));
    let mut plan = mapping::plan(JSON, &project, Definitions::default(), "missing".into()).unwrap();
    assert!(storage::apply(root.path(), &project.data.id, &mut plan, true, &values).is_err());
    let variable = plan.variables.iter().find(|v| v.value.is_some()).unwrap();
    let key = format!("{}/{}/default", project.data.id, variable.definition.id);
    values
        .values
        .borrow_mut()
        .insert(key.clone(), "edited-local-value".into());
    values.remaining.set(None);
    let error =
        storage::apply(root.path(), &project.data.id, &mut plan, true, &values).unwrap_err();
    assert!(error.contains("not overwritten"));
    assert_eq!(
        values.values.borrow().get(&key).unwrap(),
        "edited-local-value"
    );
    assert!(store
        .reload(&project.handle)
        .unwrap()
        .collections
        .is_empty());
}

#[test]
fn basic_auth_mapping_and_operation_origins_survive_request_renames() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Target".into()).unwrap();
    let mut source = document::parse(JSON).unwrap();
    source["components"]["securitySchemes"]["key"] = json!({"type":"http","scheme":"basic"});
    let mut plan = mapping::plan(
        &serde_json::to_vec(&source).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    let request = &mut plan.collection.requests[0];
    assert!(matches!(request.auth, Some(Auth::Basic { .. })));
    request.name = "Renamed imported operation".into();
    let metadata = plan.collection.open_api.as_ref().unwrap();
    let pointer = metadata.operations.get(&request.id).unwrap();
    assert_eq!(pointer, "#/paths/~1items/post");
    assert!(metadata.document.pointer(&pointer[1..]).is_some());
    storage::apply(
        root.path(),
        &project.data.id,
        &mut plan,
        false,
        &Fake::default(),
    )
    .unwrap();
    let reopened = store.reload(&project.handle).unwrap();
    let metadata = reopened.collections[0].data.open_api.as_ref().unwrap();
    assert_eq!(metadata.operations.len(), 1);
}

#[test]
fn incomplete_response_definitions_warn_without_blocking_request_migration() {
    let root = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store
        .create(root.path(), "Response-free migration".into())
        .unwrap();
    for version in ["3.0.0", "3.0.4", "3.1.2"] {
        for responses in [None, Some(Value::Null), Some(json!({}))] {
            let mut source = json!({"openapi":version,"info":{"title":"Incomplete API","version":"1"},"paths":{"/items":{"get":{"summary":"List items"}}}});
            if let Some(responses) = responses {
                source["paths"]["/items"]["get"]["responses"] = responses;
            }
            let plan = mapping::plan(
                &serde_json::to_vec(&source).unwrap(),
                &project,
                Definitions::default(),
                "missing".into(),
            )
            .unwrap();
            assert!(plan.preview.blockers.is_empty());
            assert_eq!(plan.collection.requests.len(), 1);
            assert!(plan
                .preview
                .warnings
                .iter()
                .any(|w| w.contains("no response definitions")));
            assert!(plan
                .collection
                .open_api
                .as_ref()
                .unwrap()
                .document
                .pointer("/paths/~1items/get/responses/200")
                .is_none());
        }
    }
    for malformed in [json!([]), json!(true), json!("invalid")] {
        let source = json!({"openapi":"3.0.0","info":{"title":"Invalid API","version":"1"},"paths":{"/items":{"get":{"responses":malformed}}}});
        assert!(mapping::plan(
            &serde_json::to_vec(&source).unwrap(),
            &project,
            Definitions::default(),
            "missing".into()
        )
        .is_err());
    }
    let source = json!({"openapi":"3.0.0","info":{"title":"No response examples","version":"1"},"paths":{"/items":{"get":{}}}});
    let mut plan = mapping::plan(
        &serde_json::to_vec(&source).unwrap(),
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
    let loaded = store.reload(&project.handle).unwrap();
    assert_eq!(loaded.collections.len(), 1);
    assert_eq!(loaded.collections[0].data.requests[0].method, "GET");
}

#[test]
fn sixty_routes_keep_only_server_variables_and_independent_parameter_literals() {
    let root = tempdir().unwrap();
    let project = ProjectStore::default()
        .create(root.path(), "Synthetic".into())
        .unwrap();
    let mut source = json!({"openapi":"3.0.0","info":{"title":"Synthetic migration","version":"1"},
        "servers":[{"url":"https://synthetic.example.test/v1"}], "paths":{}});
    let parameters = json!([
        {"in":"path","name":"id","required":true,"schema":{"type":"string"}},
        {"in":"query","name":"limit","schema":{"type":"integer","default":20}},
        {"in":"header","name":"X-Tenant","schema":{"type":"string","default":"synthetic-tenant"}}
    ]);
    for i in 0..60 {
        source["paths"][format!("/items/{i}/{{id}}")] = json!({"get":{"parameters":parameters,"responses":{"200":{"description":"Synthetic"}}}});
    }
    let mut definitions = Definitions::default();
    let existing = environments::Definition {
        id: uuid::Uuid::new_v4().to_string(),
        name: "baseUrl".into(),
        secret: None,
    };
    definitions.variables.push(existing.clone());
    let bytes = serde_json::to_vec(&source).unwrap();
    let mut plan = mapping::plan(&bytes, &project, definitions, "missing".into()).unwrap();
    assert_eq!(plan.variables.len(), 1);
    assert_eq!(plan.variables[0].definition.name, "baseUrl_2");
    assert!(plan
        .collection
        .requests
        .iter()
        .all(|r| r.url.starts_with("{{baseUrl_2}}") && r.url.ends_with("/{id}")));
    assert!(plan
        .collection
        .requests
        .iter()
        .all(|r| r.params[0].value == "20" && r.headers[0].value == "synthetic-tenant"));
    assert_eq!(plan.preview.review_count, 60);
    assert!(plan
        .collection
        .requests
        .iter()
        .all(|r| !r.import_issues.is_empty()));
    assert!(plan
        .preview
        .variables
        .iter()
        .all(|v| v.source_name.is_some() && !v.name.starts_with("oas_")));
    let id = plan.variables[0].definition.id.clone();
    let metadata = plan.collection.open_api.clone().unwrap().document;
    super::rename::variable(&mut plan, "baseUrl_2", "apiServer").unwrap();
    assert_eq!(plan.variables[0].definition.id, id);
    assert_eq!(plan.definitions.variables[0].name, existing.name);
    assert!(plan
        .collection
        .requests
        .iter()
        .all(|r| r.url.starts_with("{{apiServer}}")));
    assert!(plan
        .preview
        .requests
        .iter()
        .all(|r| r.url.starts_with("{{apiServer}}")));
    assert_eq!(
        plan.collection.open_api.as_ref().unwrap().document,
        metadata
    );
    let preview = serde_json::to_string(&plan.preview).unwrap();
    assert!(!preview.contains("synthetic-tenant"));
    assert!(!preview.contains("https://synthetic.example.test"));
    assert!(!root.path().join("environments.fluxkit.json").exists());
    source["paths"]["/items/0/{id}"]["get"]["parameters"][1]["schema"]["default"] = 50.into();
    let changed = mapping::plan(
        &serde_json::to_vec(&source).unwrap(),
        &project,
        Definitions::default(),
        "missing".into(),
    )
    .unwrap();
    assert_eq!(changed.variables.len(), 1);
    assert_eq!(
        changed
            .collection
            .requests
            .iter()
            .filter(|r| r.params[0].value == "50")
            .count(),
        1
    );
    assert_eq!(
        changed
            .collection
            .requests
            .iter()
            .filter(|r| r.params[0].value == "20")
            .count(),
        59
    );
}

#[test]
fn preview_renaming_preserves_auth_ids_values_and_plan_on_rejection() {
    let root = tempdir().unwrap();
    let project = ProjectStore::default()
        .create(root.path(), "Synthetic".into())
        .unwrap();
    let mut plan = mapping::plan(YAML, &project, Definitions::default(), "missing".into()).unwrap();
    let auth = serde_json::to_value(&plan.collection.requests[0].auth).unwrap();
    let token = plan
        .variables
        .iter()
        .find(|v| v.definition.name == "token_token")
        .unwrap()
        .definition
        .id
        .clone();
    super::rename::variable(&mut plan, "token_token", "accessToken").unwrap();
    assert_eq!(
        plan.variables
            .iter()
            .find(|v| v.definition.name == "accessToken")
            .unwrap()
            .definition
            .id,
        token
    );
    assert_eq!(
        serde_json::to_value(&plan.collection.requests[0].auth).unwrap(),
        auth
    );
    let original = serde_json::to_vec(&plan.collection).unwrap();
    for name in ["", "baseUrl", "bad name", "{{capture}}"] {
        assert!(super::rename::variable(&mut plan, "accessToken", name).is_err());
        assert_eq!(serde_json::to_vec(&plan.collection).unwrap(), original);
    }
    plan.definitions_saved = true;
    assert!(super::rename::variable(&mut plan, "accessToken", "otherToken").is_err());
}

#[test]
fn preview_rename_updates_server_templates_and_bounds_growth() {
    let root = tempdir().unwrap();
    let project = ProjectStore::default()
        .create(root.path(), "Synthetic".into())
        .unwrap();
    let mut plan = mapping::plan(YAML, &project, Definitions::default(), "missing".into()).unwrap();
    super::rename::variable(&mut plan, "baseUrl", "petServer").unwrap();
    assert!(plan
        .collection
        .requests
        .iter()
        .all(|r| r.url.starts_with("{{petServer}}")));
    super::rename::variable(&mut plan, "petServer", "baseUrl").unwrap();
    plan.collection.requests[0].body.text = "{{baseUrl}}".repeat(300_000);
    let before = plan.collection.requests[0].body.text.clone();
    assert!(super::rename::variable(&mut plan, "baseUrl", &"a".repeat(200)).is_err());
    assert_eq!(plan.collection.requests[0].body.text, before);
    assert_eq!(
        plan.variables
            .iter()
            .find(|v| v.definition.name == "baseUrl")
            .unwrap()
            .definition
            .name,
        "baseUrl"
    );
}
