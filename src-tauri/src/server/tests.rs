use super::*;
use crate::projects::{
    environments::{ScopedValues, Values},
    store::ProjectStore,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

fn identity() -> ServerIdentity {
    ServerIdentity {
        server_url: "https://synthetic.example.test".into(),
        account_id: uuid::Uuid::new_v4(),
    }
}
fn snapshot(id: uuid::Uuid, seq: i64, name: &str) -> sync::RemoteTree {
    sync::RemoteTree {
        project: sync::RemoteProject {
            id: id.to_string(),
            name: name.into(),
            kind: "personal".into(),
            auth: None,
            revision: 1,
            can_edit: true,
        },
        seq,
        ..Default::default()
    }
}
fn workspace(
    root: &Path,
    identity: &ServerIdentity,
    value: &sync::RemoteTree,
) -> (PathBuf, ProjectStore, crate::projects::store::ProjectView) {
    let scoped = account_root(root, identity).unwrap();
    let path = server_workspace(&scoped, identity, value).unwrap();
    let mut store = ProjectStore::default();
    store.set_server_identity(Some(identity.clone()));
    let view = store.open_server_workspace(&path).unwrap();
    (scoped, store, view)
}

#[test]
fn account_and_server_scopes_never_reuse_another_accounts_offline_data() {
    let root = tempfile::tempdir().unwrap();
    let a = identity();
    let b = ServerIdentity {
        account_id: uuid::Uuid::new_v4(),
        ..a.clone()
    };
    let other_server = ServerIdentity {
        server_url: "https://other.example.test".into(),
        ..a.clone()
    };
    let id = uuid::Uuid::new_v4();
    let value = snapshot(id, 1, "Account A private project");
    let scoped_a = account_root(root.path(), &a).unwrap();
    let cached_a = cache_snapshot(&scoped_a, &a, &value).unwrap();
    let list = serde_json::to_vec(&CachedProjectList {
        server_url: a.server_url.clone(),
        projects: vec![ServerProject {
            id: id.to_string(),
            name: value.project.name.clone(),
            kind: "personal".into(),
            revision: 1,
            can_edit: true,
            can_manage_members: false,
            created_at: None,
            collection_count: None,
        }],
    })
    .unwrap();
    fs::write(scoped_a.join("server-project-list.json"), list).unwrap();
    for identity in [&b, &other_server] {
        let scoped = account_root(root.path(), identity).unwrap();
        assert_ne!(scoped, scoped_a);
        assert!(cached_project_list(&scoped, &identity.server_url)
            .unwrap()
            .is_empty());
        assert!(latest_cached_snapshot(&scoped, identity, id)
            .unwrap()
            .is_none());
        // A mismatching owner marker must also be rejected if a caller supplies the wrong root.
        assert!(cache_snapshot(&scoped_a, identity, &value).is_err());
        assert!(latest_cached_snapshot(&scoped_a, identity, id)
            .unwrap()
            .is_none());
    }
    assert_eq!(
        latest_cached_snapshot(&scoped_a, &a, id).unwrap(),
        Some(cached_a)
    );
}

#[test]
fn logout_and_account_switch_revoke_open_native_handles_but_keep_local_projects_and_files() {
    let root = tempfile::tempdir().unwrap();
    let a = identity();
    let value = snapshot(uuid::Uuid::new_v4(), 1, "Private project");
    let (_, mut store, view) = workspace(root.path(), &a, &value);
    let local_dir = tempfile::tempdir().unwrap();
    let local = store
        .create(local_dir.path(), "Local project".into())
        .unwrap();
    let saved = fs::read(Path::new(&view.directory).join("project.fluxkit.json")).unwrap();
    store.set_server_identity(None);
    assert!(store.reload(&view.handle).is_err());
    assert!(store.with_environment(&view.handle, |_, _| Ok(())).is_err());
    assert!(store.open(Path::new(&view.directory)).is_err());
    assert!(store.reload(&local.handle).is_ok());
    store.set_server_identity(Some(ServerIdentity {
        account_id: uuid::Uuid::new_v4(),
        ..a.clone()
    }));
    assert!(store.ensure_writable(&view.handle).is_err());
    assert!(store.open(Path::new(&view.directory)).is_err());
    assert_eq!(
        fs::read(Path::new(&view.directory).join("project.fluxkit.json")).unwrap(),
        saved
    );
    store.set_server_identity(Some(a));
    assert!(store.reload(&view.handle).is_ok());
}

#[derive(Clone, Default)]
struct FakeValues(Rc<RefCell<HashMap<String, String>>>);
impl Values for FakeValues {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.0.borrow().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.borrow_mut().insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        self.0.borrow_mut().remove(key);
        Ok(())
    }
}
#[test]
fn local_values_and_tokens_do_not_cross_server_accounts_with_the_same_project_id() {
    let root = tempfile::tempdir().unwrap();
    let a = identity();
    let b = ServerIdentity {
        account_id: uuid::Uuid::new_v4(),
        ..a.clone()
    };
    let value = snapshot(uuid::Uuid::new_v4(), 1, "Shared project");
    let (_, _, workspace_a) = workspace(root.path(), &a, &value);
    let (_, _, workspace_b) = workspace(root.path(), &b, &value);
    let fake = FakeValues::default();
    let local = ScopedValues::for_root(fake.clone(), None).unwrap();
    let a_values =
        ScopedValues::for_root(fake.clone(), Some(Path::new(&workspace_a.directory))).unwrap();
    let b_values = ScopedValues::for_root(fake, Some(Path::new(&workspace_b.directory))).unwrap();
    let variable_key = format!("{}/variable/default", value.project.id);
    local.set(&variable_key, "Synthetic local value").unwrap();
    assert!(a_values.get(&variable_key).is_err());
    assert!(a_values
        .set(&variable_key, "Never store server plaintext in OS entries")
        .is_err());
    assert_eq!(
        local.get(&variable_key).unwrap().as_deref(),
        Some("Synthetic local value")
    );
    {
        let key = format!("tokens/v1/{}/project/default/config", value.project.id);
        local.set(&key, "Synthetic local token").unwrap();
        assert!(a_values.get(&key).unwrap().is_none());
        a_values.set(&key, "Synthetic account A token").unwrap();
        assert!(b_values.get(&key).unwrap().is_none());
        b_values.set(&key, "Synthetic account B token").unwrap();
        a_values.delete(&key).unwrap();
        assert_eq!(
            local.get(&key).unwrap().as_deref(),
            Some("Synthetic local token")
        );
        assert_eq!(
            b_values.get(&key).unwrap().as_deref(),
            Some("Synthetic account B token")
        );
    }
}

#[test]
fn legacy_unowned_workspaces_are_preserved_and_never_claimed_by_the_next_account() {
    let local = tempfile::tempdir().unwrap();
    let mut store = ProjectStore::default();
    let view = store
        .create(local.path(), "Legacy saved edits".into())
        .unwrap();
    let bytes = fs::read(local.path().join("project.fluxkit.json")).unwrap();
    fs::write(local.path().join(".fluxkit/server-workspace.json"), serde_json::json!({"serverUrl": "https://example.test", "serverProjectId": view.data.id, "revision": 1}).to_string()).unwrap();
    store.set_server_identity(Some(identity()));
    assert!(store.open(local.path()).unwrap_err().contains("legacy"));
    assert!(store.reload(&view.handle).is_err());
    assert_eq!(
        fs::read(local.path().join("project.fluxkit.json")).unwrap(),
        bytes
    );
}

#[test]
fn incomplete_snapshot_staging_is_ignored_and_a_retry_creates_a_complete_immutable_revision() {
    let root = tempfile::tempdir().unwrap();
    let owner = identity();
    let id = uuid::Uuid::new_v4();
    let scoped = account_root(root.path(), &owner).unwrap();
    let value = snapshot(id, 1, "Saved");
    let cached = cache_snapshot(&scoped, &owner, &value).unwrap();
    fs::create_dir(
        scoped
            .join("server-snapshots")
            .join(id.to_string())
            .join(".snapshot-interrupted"),
    )
    .unwrap();
    assert_eq!(
        latest_cached_snapshot(&scoped, &owner, id).unwrap(),
        Some(cached.clone())
    );
    let next = snapshot(id, 2, "Next");
    let next_path = cache_snapshot(&scoped, &owner, &next).unwrap();
    assert_eq!(
        latest_cached_snapshot(&scoped, &owner, id).unwrap(),
        Some(next_path)
    );
    assert_eq!(
        crate::projects::store::load_project(&cached)
            .unwrap()
            .0
            .name,
        "Saved"
    );
}

#[cfg(unix)]
#[test]
fn cache_paths_cannot_follow_symlinks_into_another_accounts_data() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(external.path(), root.path().join("server-accounts")).unwrap();
    assert!(account_root(root.path(), &identity()).is_err());
    assert!(fs::read_dir(external.path()).unwrap().next().is_none());
}

#[test]
fn history_and_environment_selection_are_account_local_even_for_the_same_shared_project() {
    use crate::projects::{environments, store::local_state_id};
    let root = tempfile::tempdir().unwrap();
    let owner_a = identity();
    let owner_b = ServerIdentity {
        account_id: uuid::Uuid::new_v4(),
        ..owner_a.clone()
    };
    let project = uuid::Uuid::new_v4();
    let environment = uuid::Uuid::new_v4().to_string();
    let mut value = snapshot(project, 1, "Shared");
    value.environments.push(sync::Remote {
        id: environment.clone(),
        revision: 1,
        data: sync::EnvironmentData {
            name: "Test".into(),
            position: "g".into(),
        },
    });
    let (_, _, a) = workspace(root.path(), &owner_a, &value);
    let (_, _, b) = workspace(root.path(), &owner_b, &value);
    let a_root = Path::new(&a.directory);
    let b_root = Path::new(&b.directory);
    let settings = root.path().join("selection");
    environments::apply(
        a_root,
        &settings,
        &value.project.id,
        environments::Action::Select {
            environment: Some(environment.clone()),
        },
        &FakeValues::default(),
    )
    .unwrap();
    assert_eq!(
        environments::load(a_root, &settings, &value.project.id)
            .unwrap()
            .selected_environment,
        Some(environment)
    );
    assert!(environments::load(b_root, &settings, &value.project.id)
        .unwrap()
        .selected_environment
        .is_none());
    let a_id = local_state_id(a_root, &value.project.id).unwrap();
    let b_id = local_state_id(b_root, &value.project.id).unwrap();
    let history = crate::history::Store::new(root.path().join("history"));
    history.set_recording(&a_id, true, 100).unwrap();
    assert!(history.view(&a_id, 100).unwrap().recording);
    assert!(!history.view(&b_id, 100).unwrap().recording);
    assert!(!history.view(&value.project.id, 100).unwrap().recording);
}

#[test]
fn offline_access_opens_the_last_server_workspace_read_only() {
    let root = tempfile::tempdir().unwrap();
    let owner = identity();
    let id = uuid::Uuid::new_v4();
    let (scoped, mut store, _) =
        workspace(root.path(), &owner, &snapshot(id, 1, "Last online state"));
    let offline = offline_project(&scoped, &owner, id).unwrap();
    let offline_view = store.open(&offline).unwrap();
    assert_eq!(offline_view.data.name, "Last online state");
    assert!(offline_view.read_only);
    assert!(store.ensure_writable(&offline_view.handle).is_err());
}

#[test]
fn reopening_online_rewrites_the_mirror_from_the_server_tree() {
    let root = tempfile::tempdir().unwrap();
    let owner = identity();
    let id = uuid::Uuid::new_v4();
    let (scoped, _, view) = workspace(root.path(), &owner, &snapshot(id, 1, "Before"));
    let mut changed = snapshot(id, 5, "Renamed by a teammate");
    changed.project.revision = 2;
    let again = server_workspace(&scoped, &owner, &changed).unwrap();
    assert_eq!(
        again.canonicalize().unwrap(),
        Path::new(&view.directory).canonicalize().unwrap()
    );
    assert_eq!(
        crate::projects::store::load_project(&again).unwrap().0.name,
        "Renamed by a teammate"
    );
    assert_eq!(workspace::read_tree(&again).unwrap().seq, 5);
    let stranger = ServerIdentity {
        account_id: uuid::Uuid::new_v4(),
        ..owner
    };
    assert!(server_workspace(&scoped, &stranger, &changed).is_err());
}

#[test]
fn read_only_copies_are_immutable_per_feed_position() {
    let root = tempfile::tempdir().unwrap();
    let owner = identity();
    let id = uuid::Uuid::new_v4();
    let scoped = account_root(root.path(), &owner).unwrap();
    let mut first = snapshot(id, 3, "Viewer copy");
    first.project.can_edit = false;
    let path = cache_snapshot(&scoped, &owner, &first).unwrap();
    assert_eq!(cache_snapshot(&scoped, &owner, &first).unwrap(), path);
    let mut store = ProjectStore::default();
    store.set_server_identity(Some(owner));
    assert!(store.open(&path).unwrap().read_only);
}

#[test]
fn deleting_a_project_removes_only_its_local_copies() {
    let root = tempfile::tempdir().unwrap();
    let owner = identity();
    let id = uuid::Uuid::new_v4();
    let other = uuid::Uuid::new_v4();
    let (scoped, _, view) = workspace(root.path(), &owner, &snapshot(id, 1, "Doomed"));
    let (_, _, kept) = workspace(root.path(), &owner, &snapshot(other, 1, "Kept"));
    let mut viewer = snapshot(id, 2, "Doomed");
    viewer.project.can_edit = false;
    cache_snapshot(&scoped, &owner, &viewer).unwrap();
    remove_local_copies(&scoped, id).unwrap();
    assert!(!Path::new(&view.directory).exists());
    assert!(!scoped
        .join("server-snapshots")
        .join(id.to_string())
        .exists());
    assert!(!scoped
        .join("server-workspaces")
        .join(format!("{id}.json"))
        .exists());
    assert!(Path::new(&kept.directory).exists());
    remove_local_copies(&scoped, id).unwrap();
}

#[test]
fn server_errors_name_the_status_and_a_safe_redirect_target() {
    assert_eq!(
        super::status_error(400, None),
        "The Fluxkit server rejected the request (HTTP 400)."
    );
    assert_eq!(
        super::status_error(301, Some("https://other.example.test/")),
        "The server answered with a redirect to https://other.example.test/ (HTTP 301). Enter the final HTTPS server URL without a path, and check the reverse proxy."
    );
    assert!(super::status_error(302, Some("https://x.test/\nInjected"))
        .starts_with("The server answered with a redirect (HTTP 302)."));
    assert!(super::status_error(401, None).ends_with("approve your account (HTTP 401)."));
    assert!(super::status_error(502, None).ends_with("(HTTP 502)."));
}

#[test]
fn sessions_refresh_shortly_before_expiry_and_stay_one_login_across_renewals() {
    let identity = identity();
    let legacy: Session = serde_json::from_value(serde_json::json!({
        "identity": identity,
        "accessToken": "synthetic-old-token"
    }))
    .unwrap();
    assert!(
        !legacy.needs_refresh(1_000),
        "pre-refresh sessions keep their old behaviour"
    );

    let login = uuid::Uuid::new_v4();
    let session = Session {
        identity: identity.clone(),
        access_token: "synthetic-access-1".into(),
        refresh_token: Some("synthetic-refresh-1".into()),
        expires_at: Some(10_000),
        login_id: Some(login),
    };
    assert!(!session.needs_refresh(10_000 - REFRESH_MARGIN_SECONDS - 1));
    assert!(session.needs_refresh(10_000 - REFRESH_MARGIN_SECONDS + 1));
    assert!(Session {
        expires_at: None,
        ..session.clone()
    }
    .needs_refresh(0));

    let renewed = Session {
        access_token: "synthetic-access-2".into(),
        refresh_token: Some("synthetic-refresh-2".into()),
        expires_at: Some(13_600),
        ..session.clone()
    };
    assert!(
        renewed.same_login(&session),
        "renewal keeps open projects valid"
    );
    let other_login = Session {
        login_id: Some(uuid::Uuid::new_v4()),
        ..renewed.clone()
    };
    assert!(
        !other_login.same_login(&session),
        "a new sign-in invalidates old handles"
    );

    let stored = serde_json::to_value(&renewed).unwrap();
    assert_eq!(stored["refreshToken"], "synthetic-refresh-2");
    let restored: Session = serde_json::from_value(stored).unwrap();
    assert!(restored.same_login(&session));
}

#[test]
fn stored_sign_ins_without_refresh_token_expire_with_their_access_token() {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let token = |exp: i64| {
        format!(
            "e30.{}.c2lnbmF0dXJl",
            URL_SAFE_NO_PAD
                .encode(serde_json::json!({ "sub": "synthetic", "exp": exp }).to_string())
        )
    };
    assert_eq!(jwt_expiry(&token(5_000)), Some(5_000));
    assert_eq!(jwt_expiry("not-a-jwt"), None);
    let legacy = Session {
        identity: identity(),
        access_token: token(5_000),
        refresh_token: None,
        expires_at: None,
        login_id: None,
    };
    assert!(!legacy.expired(4_999));
    assert!(legacy.expired(5_000));
    assert!(
        !legacy.needs_refresh(5_000),
        "without a refresh token only a new sign-in helps"
    );
    let renewable = Session {
        refresh_token: Some("synthetic-refresh".into()),
        ..legacy.clone()
    };
    assert!(!renewable.expired(9_999));
    assert!(renewable.needs_refresh(5_000 - REFRESH_MARGIN_SECONDS + 1));
}
