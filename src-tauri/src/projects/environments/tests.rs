use super::*;
use std::{cell::RefCell, collections::BTreeMap};
use tempfile::tempdir;
use uuid::Uuid;

#[derive(Default)]
struct Fake {
    values: RefCell<BTreeMap<String, String>>,
    fail: RefCell<bool>,
    reads: RefCell<Vec<String>>,
    delete_budget: RefCell<Option<usize>>,
}
impl Values for Fake {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        self.reads.borrow_mut().push(key.to_string());
        if *self.fail.borrow() {
            return Err(STORE_ERROR.into());
        }
        Ok(self.values.borrow().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        if *self.fail.borrow() {
            return Err(STORE_ERROR.into());
        }
        self.values.borrow_mut().insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        if *self.fail.borrow() {
            return Err(STORE_ERROR.into());
        }
        if let Some(remaining) = self.delete_budget.borrow_mut().as_mut() {
            if *remaining == 0 {
                return Err(STORE_ERROR.into());
            }
            *remaining -= 1;
        }
        self.values.borrow_mut().remove(key);
        Ok(())
    }
}
fn definition(name: &str) -> Definition {
    Definition {
        id: Uuid::new_v4().to_string(),
        name: name.into(),
        secret: None,
    }
}
fn fixture() -> Definitions {
    Definitions {
        schema_version: 1,
        environments: vec![definition("Testserver"), definition("Prodserver")],
        variables: vec![definition("baseUrl"), definition("username")],
    }
}
fn save(
    root: &Path,
    settings: &Path,
    project: &str,
    data: Definitions,
    values: &impl Values,
) -> Reply {
    if !root.join("project.fluxkit.json").exists() {
        let manifest = serde_json::json!({"schemaVersion":1,"id":Uuid::new_v4().to_string(),"name":"Synthetic project"});
        std::fs::write(
            root.join("project.fluxkit.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        std::fs::create_dir(root.join("collections")).unwrap();
    }
    std::fs::create_dir_all(root.join(".fluxkit")).unwrap();
    let expected = definitions(root).unwrap().1;
    apply(
        root,
        settings,
        project,
        Action::Save { data, expected },
        values,
    )
    .unwrap()
}
#[test]
fn resolution_precedence_empty_values_missing_and_denied_store() {
    let data = fixture();
    let values = Fake::default();
    let id = &data.variables[0].id;
    values.set(&key("p", id, None), "default").unwrap();
    assert_eq!(
        resolve(
            "{{baseUrl}}/path",
            "p",
            &data,
            Some(&data.environments[0].id),
            &values
        )
        .unwrap(),
        "default/path"
    );
    values
        .set(&key("p", id, Some(&data.environments[0].id)), "")
        .unwrap();
    assert_eq!(
        resolve(
            "{{baseUrl}}",
            "p",
            &data,
            Some(&data.environments[0].id),
            &values
        )
        .unwrap(),
        ""
    );
    assert!(resolve("{{username}}", "p", &data, None, &values).is_err());
    assert!(resolve("{{unknown}}", "p", &data, None, &values).is_err());
    assert!(resolve("{{baseUrl", "p", &data, None, &values).is_err());
    *values.fail.borrow_mut() = true;
    assert_eq!(
        resolve(
            "{{baseUrl}}",
            "p",
            &data,
            Some(&data.environments[0].id),
            &values
        )
        .unwrap_err(),
        STORE_ERROR
    );
}
#[test]
fn rename_preserves_value_identity_and_resolution_is_single_pass() {
    let mut data = fixture();
    let values = Fake::default();
    values
        .set(
            &key("p", &data.variables[0].id, None),
            "{{literal}} synthetic-secret",
        )
        .unwrap();
    data.variables[0].name = "host".into();
    data.environments[0].name = "Staging".into();
    assert_eq!(
        resolve("{{host}}", "p", &data, None, &values).unwrap(),
        "{{literal}} synthetic-secret"
    );
    assert!(resolve("{{host}}", "other-project", &data, None, &values).is_err());
}
#[test]
fn optional_file_roundtrip_selection_restart_and_project_isolation() {
    let root = tempdir().unwrap();
    let settings = tempdir().unwrap();
    let values = Fake::default();
    let data = fixture();
    assert_eq!(
        load(root.path(), settings.path(), "p").unwrap().revision,
        "missing"
    );
    save(root.path(), settings.path(), "p", data.clone(), &values);
    apply(
        root.path(),
        settings.path(),
        "p",
        Action::Select {
            environment: Some(data.environments[0].id.clone()),
        },
        &values,
    )
    .unwrap();
    assert_eq!(
        load(root.path(), settings.path(), "p")
            .unwrap()
            .selected_environment,
        Some(data.environments[0].id.clone())
    );
    assert_eq!(
        load(root.path(), settings.path(), "q")
            .unwrap()
            .selected_environment,
        None
    );
    assert!(load(root.path(), settings.path(), "p")
        .unwrap()
        .data
        .validate()
        .is_ok());
    assert!(!std::fs::read_to_string(root.path().join(FILE))
        .unwrap()
        .contains("selectedEnvironment"));
}
#[test]
fn values_never_enter_files_and_stale_writes_do_not_touch_store() {
    let root = tempdir().unwrap();
    let settings = tempdir().unwrap();
    let values = Fake::default();
    let data = fixture();
    let saved = save(root.path(), settings.path(), "p", data.clone(), &values);
    let action = || Action::SetValue {
        variable: data.variables[0].id.clone(),
        environment: None,
        value: Some("synthetic-private".into()),
        expected: saved.view.revision.clone(),
    };
    apply(root.path(), settings.path(), "p", action(), &values).unwrap();
    let bytes = std::fs::read(root.path().join(FILE)).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-private"));
    save(
        root.path(),
        settings.path(),
        "p",
        {
            let mut d = data.clone();
            d.variables[0].name = "renamed".into();
            d
        },
        &values,
    );
    assert!(apply(root.path(), settings.path(), "p", action(), &values).is_err());
    assert_eq!(
        values
            .get(&key("p", &data.variables[0].id, None))
            .unwrap()
            .as_deref(),
        Some("synthetic-private")
    );
    let invalid = Action::SetValue {
        variable: Uuid::new_v4().to_string(),
        environment: None,
        value: Some("forbidden".into()),
        expected: definitions(root.path()).unwrap().1,
    };
    assert!(apply(root.path(), settings.path(), "p", invalid, &values).is_err());
}
#[test]
fn removal_is_scoped_and_failed_deletion_keeps_definitions_for_retry() {
    let root = tempdir().unwrap();
    let settings = tempdir().unwrap();
    let values = Fake::default();
    let data = fixture();
    save(root.path(), settings.path(), "p", data.clone(), &values);
    let variable = &data.variables[0].id;
    let env = &data.environments[0].id;
    for project in ["p", "q"] {
        for environment in [None, Some(env.as_str())] {
            values
                .set(&key(project, variable, environment), "synthetic")
                .unwrap();
        }
    }
    let mut removed = data.clone();
    removed.environments.remove(0);
    let expected = definitions(root.path()).unwrap().1;
    *values.fail.borrow_mut() = true;
    assert!(apply(
        root.path(),
        settings.path(),
        "p",
        Action::Save {
            data: removed.clone(),
            expected: expected.clone()
        },
        &values
    )
    .is_err());
    assert_eq!(definitions(root.path()).unwrap().1, expected);
    *values.fail.borrow_mut() = false;
    save(root.path(), settings.path(), "p", removed.clone(), &values);
    assert!(values
        .get(&key("p", variable, Some(env)))
        .unwrap()
        .is_none());
    assert!(values.get(&key("p", variable, None)).unwrap().is_some());
    assert!(values
        .get(&key("q", variable, Some(env)))
        .unwrap()
        .is_some());
    removed.variables.remove(0);
    save(root.path(), settings.path(), "p", removed, &values);
    assert!(values.get(&key("p", variable, None)).unwrap().is_none());
    assert!(root
        .path()
        .join(".fluxkit/backups/environments.json")
        .exists());
}
#[test]
fn rejects_invalid_future_and_unknown_definitions_without_rewriting() {
    let root = tempdir().unwrap();
    let mut data = fixture();
    data.variables[1].name = data.variables[0].name.clone();
    assert!(data.validate().is_err());
    for content in [
        "{",
        "{\"schemaVersion\":2,\"variables\":[],\"environments\":[]}",
        "{\"schemaVersion\":1,\"variables\":[],\"environments\":[],\"values\":\"private\"}",
    ] {
        std::fs::write(root.path().join(FILE), content).unwrap();
        assert!(definitions(root.path()).is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join(FILE)).unwrap(),
            content
        );
    }
}
#[test]
#[ignore = "explicit native integration: creates and removes one isolated synthetic OS entry"]
fn os_credential_roundtrip() {
    let key = format!("integration/{}/synthetic/default", Uuid::new_v4());
    let values = OsValues;
    values.set(&key, "fluxkit-synthetic-integration").unwrap();
    let result = values.get(&key);
    let cleanup = values.delete(&key);
    assert_eq!(
        result.unwrap().as_deref(),
        Some("fluxkit-synthetic-integration")
    );
    cleanup.unwrap();
    assert!(values.get(&key).unwrap().is_none());
}

#[test]
fn check_uses_persisted_project_selection_without_returning_values() {
    let root = tempdir().unwrap();
    let settings = tempdir().unwrap();
    let values = Fake::default();
    let data = fixture();
    save(root.path(), settings.path(), "p", data.clone(), &values);
    values
        .set(
            &key("p", &data.variables[0].id, Some(&data.environments[0].id)),
            "synthetic-only-in-store",
        )
        .unwrap();
    let action = || Action::Check {
        text: "{{baseUrl}}".into(),
    };
    assert!(apply(root.path(), settings.path(), "p", action(), &values).is_err());
    apply(
        root.path(),
        settings.path(),
        "p",
        Action::Select {
            environment: Some(data.environments[0].id.clone()),
        },
        &values,
    )
    .unwrap();
    let reply = apply(root.path(), settings.path(), "p", action(), &values).unwrap();
    let encoded = serde_json::to_string(&reply).unwrap();
    assert!(!encoded.contains("synthetic-only-in-store"));
    assert!(reply.value.is_none());
    assert!(apply(root.path(), settings.path(), "q", action(), &values).is_err());
    let mut external = data;
    external.environments.clear();
    std::fs::write(root.path().join(FILE), store::encode(&external).unwrap()).unwrap();
    assert!(
        load(root.path(), settings.path(), "p")
            .unwrap()
            .selection_missing
    );
    assert!(apply(root.path(), settings.path(), "p", action(), &values).is_err());
}

#[test]
fn failed_file_write_after_deletion_explains_recovery_and_keeps_definition() {
    let root = tempdir().unwrap();
    let settings = tempdir().unwrap();
    let values = Fake::default();
    let data = fixture();
    let reply = save(root.path(), settings.path(), "p", data.clone(), &values);
    values
        .set(&key("p", &data.variables[0].id, None), "synthetic")
        .unwrap();
    // A regular file in place of the recovery directory prevents replacement.
    std::fs::write(root.path().join(".fluxkit/backups"), "blocking fixture").unwrap();
    let mut removed = data.clone();
    removed.variables.remove(0);
    let error = apply(
        root.path(),
        settings.path(),
        "p",
        Action::Save {
            data: removed,
            expected: reply.view.revision,
        },
        &values,
    )
    .err()
    .unwrap();
    assert!(error.contains("may already be removed"));
    assert_eq!(definitions(root.path()).unwrap().0.variables.len(), 2);
    assert!(values
        .get(&key("p", &data.variables[0].id, None))
        .unwrap()
        .is_none());
}

#[test]
fn partial_credential_deletion_can_be_retried_without_removing_other_variables() {
    let root = tempdir().unwrap();
    let settings = tempdir().unwrap();
    let values = Fake::default();
    let data = fixture();
    let initial = save(root.path(), settings.path(), "p", data.clone(), &values);
    let id = &data.variables[0].id;
    let env = &data.environments[0].id;
    for environment in [None, Some(env.as_str())] {
        values.set(&key("p", id, environment), "synthetic").unwrap();
    }
    let retained = key("p", &data.variables[1].id, None);
    values.set(&retained, "retained").unwrap();
    let mut removed = data.clone();
    removed.variables.remove(0);
    *values.delete_budget.borrow_mut() = Some(1);
    assert!(apply(
        root.path(),
        settings.path(),
        "p",
        Action::Save {
            data: removed.clone(),
            expected: initial.view.revision.clone()
        },
        &values
    )
    .is_err());
    assert!(values.get(&key("p", id, None)).unwrap().is_none());
    assert!(values.get(&key("p", id, Some(env))).unwrap().is_some());
    assert_eq!(definitions(root.path()).unwrap().1, initial.view.revision);
    *values.delete_budget.borrow_mut() = None;
    save(root.path(), settings.path(), "p", removed, &values);
    assert!(values.get(&key("p", id, Some(env))).unwrap().is_none());
    assert_eq!(values.get(&retained).unwrap().as_deref(), Some("retained"));
}

#[test]
fn hover_preview_resolves_one_variable_and_rejects_stale_scope_before_reading() {
    let root = tempdir().unwrap();
    let settings = tempdir().unwrap();
    let values = Fake::default();
    let data = fixture();
    let saved = save(root.path(), settings.path(), "p", data.clone(), &values);
    let variable = &data.variables[0].id;
    let env = &data.environments[0].id;
    values
        .set(&key("p", variable, None), "synthetic-default")
        .unwrap();
    apply(
        root.path(),
        settings.path(),
        "p",
        Action::Select {
            environment: Some(env.clone()),
        },
        &values,
    )
    .unwrap();
    let preview = |environment: Option<String>, expected: String| {
        serde_json::from_value::<Action>(serde_json::json!({
            "kind": "preview", "variable": variable, "environment": environment,
            "expected": expected,
        }))
        .unwrap()
    };
    let reply = apply(
        root.path(),
        settings.path(),
        "p",
        preview(Some(env.clone()), saved.view.revision.clone()),
        &values,
    )
    .unwrap();
    assert_eq!(reply.value.as_deref(), Some("synthetic-default"));
    assert_eq!(
        *values.reads.borrow(),
        vec![key("p", variable, Some(env)), key("p", variable, None)]
    );
    values.reads.borrow_mut().clear();
    assert!(apply(
        root.path(),
        settings.path(),
        "p",
        preview(None, saved.view.revision.clone()),
        &values
    )
    .is_err());
    assert!(apply(
        root.path(),
        settings.path(),
        "p",
        preview(Some(env.clone()), "outdated".into()),
        &values
    )
    .is_err());
    assert!(values.reads.borrow().is_empty());
    *values.fail.borrow_mut() = true;
    let error = apply(
        root.path(),
        settings.path(),
        "p",
        preview(Some(env.clone()), saved.view.revision),
        &values,
    )
    .err()
    .unwrap();
    assert_eq!(error, STORE_ERROR);
}

#[test]
fn removing_an_auth_referenced_variable_keeps_definitions_and_os_values() {
    let root = tempdir().unwrap();
    let settings = tempdir().unwrap();
    let values = Fake::default();
    let data = fixture();
    let saved = save(root.path(), settings.path(), "p", data.clone(), &values);
    let variable = &data.variables[0].id;
    values
        .set(&key("p", variable, None), "synthetic-value")
        .unwrap();
    let path = root.path().join("project.fluxkit.json");
    let mut project: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    project["auth"] = serde_json::json!({"kind":"bearer","token":variable});
    std::fs::write(&path, serde_json::to_vec(&project).unwrap()).unwrap();
    let mut removed = data.clone();
    removed.variables.remove(0);
    let error = apply(
        root.path(),
        settings.path(),
        "p",
        Action::Save {
            data: removed.clone(),
            expected: saved.view.revision,
        },
        &values,
    )
    .err()
    .unwrap();
    assert!(error.contains("still referenced"));
    assert_eq!(
        values.get(&key("p", variable, None)).unwrap().as_deref(),
        Some("synthetic-value")
    );
    assert_eq!(
        load(root.path(), settings.path(), "p")
            .unwrap()
            .data
            .variables
            .len(),
        2
    );
    project.as_object_mut().unwrap().remove("auth");
    std::fs::write(&path, serde_json::to_vec(&project).unwrap()).unwrap();
    save(root.path(), settings.path(), "p", removed, &values);
    assert!(values.get(&key("p", variable, None)).unwrap().is_none());
}
