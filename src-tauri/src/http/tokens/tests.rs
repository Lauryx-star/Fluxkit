use super::*;
use crate::{
    http::{
        tests::{client, input, response, server},
        transfer,
    },
    projects::{model::*, token_config::*},
};
use std::sync::atomic::{AtomicBool, AtomicUsize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
};
use uuid::Uuid;

#[tokio::test]
#[ignore = "explicit native integration: creates and removes isolated synthetic variable and token entries"]
async fn os_token_roundtrip() {
    let (url, _, task) = server(oauth("synthetic-os-token"), Duration::ZERO).await;
    let (plan, fake) = fixture(&url, ClientAuthentication::Basic);
    let source = Arc::new(environments::OsValues);
    let initial: Vec<_> = fake
        .data
        .lock()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let setup = initial.iter().try_for_each(|(k, v)| source.set(k, v));
    let result = if setup.is_ok() {
        Manager::default()
            .ensure(
                &plan,
                &client(),
                source.clone(),
                false,
                &CancellationToken::new(),
                Duration::from_secs(3),
            )
            .await
            .map(|record| record.token == "synthetic-os-token")
    } else {
        Err("Could not prepare isolated OS values.".into())
    };
    // Always attempt cleanup before asserting, including failed setup/acquisition.
    let cleanup: Vec<_> = initial
        .iter()
        .map(|(k, _)| k)
        .chain(std::iter::once(&plan.key))
        .map(|key| (source.delete(key), source.get(key)))
        .collect();
    task.abort();
    assert!(result.unwrap());
    for (deleted, remaining) in cleanup {
        deleted.unwrap();
        assert!(remaining.unwrap().is_none());
    }
}

#[derive(Default)]
struct Store {
    data: Mutex<HashMap<String, String>>,
    denied: AtomicBool,
    write_denied: AtomicBool,
    reads: AtomicUsize,
}
impl Values for Store {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.denied.load(Ordering::SeqCst) {
            return Err("synthetic-private-provider-detail".into());
        }
        Ok(self.data.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        if self.write_denied.load(Ordering::SeqCst) {
            return Err("synthetic-private-write-detail".into());
        }
        self.data.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        self.data.lock().unwrap().remove(key);
        Ok(())
    }
}
fn text(value: &str) -> Composition {
    Composition {
        parts: vec![Part::Literal { text: value.into() }],
        separator: String::new(),
        prefix: String::new(),
        suffix: String::new(),
        base64: false,
    }
}
fn fixture(url: &str, method: ClientAuthentication) -> (Plan, Arc<Store>) {
    let project = Uuid::new_v4().to_string();
    let user = Uuid::new_v4().to_string();
    let secret = Uuid::new_v4().to_string();
    let definitions = Definitions {
        schema_version: 1,
        variables: vec![
            environments::Definition {
                id: user.clone(),
                name: "client".into(),
                secret: None,
            },
            environments::Definition {
                id: secret.clone(),
                name: "secret".into(),
                secret: None,
            },
        ],
        environments: vec![],
    };
    let config = Auth::Oauth2 {
        config: Box::new(OAuth2 {
            token_url: text(url),
            client_id: user.clone(),
            client_secret: secret.clone(),
            scope: text("read write"),
            client_authentication: method,
        }),
    };
    let plan = Plan::new(Some(&config), "project", &project, None, &definitions)
        .unwrap()
        .unwrap();
    let values = Arc::new(Store::default());
    values
        .set(&format!("{project}/{user}/default"), "synthetic:client +")
        .unwrap();
    values
        .set(&format!("{project}/{secret}/default"), "synthetic/secret:")
        .unwrap();
    (plan, values)
}
async fn ensure(
    manager: &Manager,
    plan: &Plan,
    values: Arc<Store>,
    force: bool,
) -> Result<Record, String> {
    manager
        .ensure(
            plan,
            &client(),
            values,
            force,
            &CancellationToken::new(),
            Duration::from_secs(3),
        )
        .await
}
fn oauth(token: &str) -> Vec<u8> {
    response(
        "200 OK",
        "Content-Type: application/json\r\n",
        format!("{{\"access_token\":\"{token}\",\"token_type\":\"Bearer\",\"expires_in\":3600}}")
            .as_bytes(),
    )
}
async fn sequence(
    replies: Vec<Vec<u8>>,
) -> (
    String,
    mpsc::UnboundedReceiver<String>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        for response in replies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buf = [0; 4096];
                let count = socket.read(&mut buf).await.unwrap();
                if count == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..count]);
                if let Some(split) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..split]).to_ascii_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length: ")
                                .and_then(|v| v.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= split + 4 + length {
                        break;
                    }
                }
            }
            let _ = tx.send(String::from_utf8(bytes).unwrap());
            socket.write_all(&response).await.unwrap();
        }
    });
    (url, rx, task)
}

#[tokio::test]
async fn oauth_basic_is_form_encoded_and_tokens_persist_only_in_store() {
    let (url, captured, task) = server(oauth("synthetic-access"), Duration::ZERO).await;
    let (plan, values) = fixture(&url, ClientAuthentication::Basic);
    let manager = Manager::default();
    let record = ensure(&manager, &plan, values.clone(), false)
        .await
        .unwrap();
    assert_eq!(record.token, "synthetic-access");
    let request = String::from_utf8(captured.await.unwrap()).unwrap();
    assert!(request.to_lowercase().contains(
        &format!(
            "authorization: basic {}",
            STANDARD.encode("synthetic%3Aclient+%2B:synthetic%2Fsecret%3A")
        )
        .to_lowercase()
    ));
    assert!(request.contains("grant_type=client_credentials&scope=read+write"));
    assert!(!request.contains("client_secret="));
    task.await.unwrap();
    // No endpoint remains: this send can only succeed by reusing the OS-store record.
    assert_eq!(
        ensure(&manager, &plan, values.clone(), false)
            .await
            .unwrap()
            .token,
        record.token
    );
    let status =
        serde_json::to_string(&manager.status(&plan, values.clone()).await.unwrap()).unwrap();
    assert!(!status.contains("synthetic"));
    assert!(status.contains("valid"));
    assert!(!serde_json::to_string(&plan.config)
        .unwrap()
        .contains("synthetic"));
    let raw = values.get(&plan.key).unwrap().unwrap();
    assert!(raw.contains("synthetic-access"));
    manager.forget(&plan, values.clone()).await.unwrap();
    assert!(values.get(&plan.key).unwrap().is_none());
    assert!(values
        .data
        .lock()
        .unwrap()
        .values()
        .any(|v| v == "synthetic/secret:"));
}

#[tokio::test]
async fn oauth_body_unknown_expiry_and_isolation_by_owner_environment_and_configuration() {
    let (url, captured, task) = server(
        response(
            "200 OK",
            "",
            br#"{"access_token":"opaque-synthetic","token_type":"bearer"}"#,
        ),
        Duration::ZERO,
    )
    .await;
    let (plan, values) = fixture(&url, ClientAuthentication::Body);
    let manager = Manager::default();
    let record = ensure(&manager, &plan, values.clone(), false)
        .await
        .unwrap();
    assert_eq!(record.status(now().unwrap()).state, "unknownExpiry");
    let request = String::from_utf8(captured.await.unwrap()).unwrap();
    assert!(!request.to_lowercase().contains("authorization:"));
    assert!(
        request.contains("client_id=synthetic%3Aclient+%2B&client_secret=synthetic%2Fsecret%3A")
    );
    task.await.unwrap();
    for (owner, environment, project) in [
        ("collection/other", None, plan.project.clone()),
        (
            "project",
            Some(Uuid::new_v4().to_string()),
            plan.project.clone(),
        ),
        ("project", None, Uuid::new_v4().to_string()),
    ] {
        let other = Plan::new(
            Some(&plan.config),
            owner,
            &project,
            environment.as_deref(),
            &plan.definitions,
        )
        .unwrap()
        .unwrap();
        assert_ne!(plan.key, other.key);
        assert_eq!(
            manager.status(&other, values.clone()).await.unwrap().state,
            "missing"
        );
    }
    let mut changed = plan.config.clone();
    if let Auth::Oauth2 { config } = &mut changed {
        config.scope = text("new scope");
    }
    assert_ne!(
        Plan::new(
            Some(&changed),
            "project",
            &plan.project,
            None,
            &plan.definitions
        )
        .unwrap()
        .unwrap()
        .key,
        plan.key
    );
    let mut renamed = plan.definitions.clone();
    renamed.variables[0].name = "renamed".into();
    assert_eq!(
        Plan::new(Some(&plan.config), "project", &plan.project, None, &renamed)
            .unwrap()
            .unwrap()
            .key,
        plan.key
    );
}

#[tokio::test]
async fn concurrent_acquisition_shares_success_and_redacted_failure() {
    for reply in [
        oauth("shared-synthetic"),
        response("400 Bad Request", "", b"private-token-response"),
    ] {
        let (url, captured, task) = server(reply, Duration::from_millis(50)).await;
        let (plan, values) = fixture(&url, ClientAuthentication::Basic);
        let manager = Manager::default();
        let (a, b, c) = tokio::join!(
            ensure(&manager, &plan, values.clone(), true),
            ensure(&manager, &plan, values.clone(), true),
            ensure(&manager, &plan, values.clone(), true)
        );
        assert_eq!(a.is_ok(), b.is_ok());
        assert_eq!(a.is_ok(), c.is_ok());
        if let Err(error) = a {
            assert!(!error.contains("private"));
            assert!(error.contains("400"));
            assert_eq!(b.err(), Some(error));
        }
        captured.await.unwrap();
        task.await.unwrap();
    }
}

#[tokio::test]
async fn cancelling_a_waiter_does_not_cancel_the_acquisition_leader() {
    let (url, _, task) = server(oauth("leader-synthetic"), Duration::from_millis(60)).await;
    let (plan, values) = fixture(&url, ClientAuthentication::Basic);
    let manager = Manager::default();
    let waiter = CancellationToken::new();
    let client = client();
    let leader = CancellationToken::new();
    let (first, second, _) = tokio::join!(
        manager.ensure(
            &plan,
            &client,
            values.clone(),
            false,
            &leader,
            Duration::from_secs(2)
        ),
        manager.ensure(
            &plan,
            &client,
            values.clone(),
            false,
            &waiter,
            Duration::from_secs(2)
        ),
        async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            waiter.cancel();
        },
    );
    assert_eq!(first.unwrap().token, "leader-synthetic");
    assert_eq!(second.err().unwrap(), CANCELLED);
    assert_eq!(manager.status(&plan, values).await.unwrap().state, "valid");
    task.await.unwrap();
}

#[tokio::test]
async fn expiry_reacquires_and_failed_renewal_keeps_previous_record() {
    let (url, mut captured, task) = sequence(vec![
        oauth("first-synthetic"),
        oauth("replacement-synthetic"),
        response(
            "200 OK",
            "",
            br#"{"access_token":"bad-synthetic","token_type":"Bearer","expires_in":0}"#,
        ),
    ])
    .await;
    let (plan, values) = fixture(&url, ClientAuthentication::Body);
    let manager = Manager::default();
    let mut record = ensure(&manager, &plan, values.clone(), false)
        .await
        .unwrap();
    captured.recv().await.unwrap();
    record.acquired_at = now().unwrap() - 100;
    record.expires_at = Some(record.acquired_at + 50);
    values
        .set(&plan.key, &serde_json::to_string(&record).unwrap())
        .unwrap();
    let record = ensure(&manager, &plan, values.clone(), false)
        .await
        .unwrap();
    assert_eq!(record.token, "replacement-synthetic");
    captured.recv().await.unwrap();
    let previous = values.get(&plan.key).unwrap();
    let error = ensure(&manager, &plan, values.clone(), true)
        .await
        .err()
        .unwrap();
    assert!(!error.contains("bad-synthetic"));
    assert_eq!(values.get(&plan.key).unwrap(), previous);
    assert_eq!(
        ensure(&manager, &plan, values.clone(), false)
            .await
            .unwrap()
            .token,
        record.token
    );
    task.await.unwrap();
    let time = now().unwrap();
    let record = Record {
        schema_version: 1,
        token: "x".into(),
        renewal_token: None,
        acquired_at: time - 95,
        expires_at: Some(time + 5),
    };
    assert_eq!(record.status(time).state, "renewalDue");
    assert_eq!(record.status(time + 5).state, "expired");
    assert_eq!(record.status(time - 96).state, "expired");
}

#[tokio::test]
async fn custom_composition_and_separate_renewal_extract_only_accepted_complete_responses() {
    let (url, mut captured, task) = sequence(vec![
        response(
            "201 Created",
            "X-Renewal: renewal-one\r\n",
            br#"{"session":{"token":"access-one"},"seconds":60}"#,
        ),
        response(
            "200 OK",
            "X-Token: access-two\r\nX-Renewal: renewal-two\r\n",
            b"ignored",
        ),
    ])
    .await;
    let (mut plan, values) = fixture(&url, ClientAuthentication::Basic);
    let id = plan.definitions.variables[0].id.clone();
    let mut composed = text("");
    composed.parts = vec![
        Part::Variable { id },
        Part::Literal {
            text: "tail".into(),
        },
    ];
    composed.separator = ":".into();
    composed.prefix = "Basic ".into();
    composed.base64 = true;
    let login = LoginRequest {
        url: text(&url),
        method: "POST".into(),
        params: vec![LoginEntry {
            name: "q".into(),
            value: text("a +&"),
        }],
        headers: vec![LoginEntry {
            name: "X-Composed".into(),
            value: composed,
        }],
        body: LoginBody::Json {
            value: text("{\"synthetic\":true}"),
        },
        success_statuses: vec![201],
        token: Extraction::Json {
            pointer: "/session/token".into(),
        },
        renewal_token: Some(Extraction::Header {
            name: "X-Renewal".into(),
        }),
        expires_in: Some(Extraction::Json {
            pointer: "/seconds".into(),
        }),
        lifetime_seconds: None,
    };
    let mut renewal = login.clone();
    renewal.params.clear();
    renewal.headers.clear();
    renewal.success_statuses = vec![200];
    renewal.body = LoginBody::UrlEncoded {
        fields: vec![LoginEntry {
            name: "renew".into(),
            value: Composition {
                parts: vec![Part::RenewalToken, Part::AccessToken],
                separator: "|".into(),
                ..text("")
            },
        }],
    };
    renewal.token = Extraction::Header {
        name: "X-Token".into(),
    };
    renewal.expires_in = None;
    renewal.lifetime_seconds = Some(120);
    let auth = Auth::CustomLogin {
        config: Box::new(CustomLogin {
            login,
            renewal: Some(renewal),
            placement: TokenPlacement {
                name: "access".into(),
                placement: KeyPlacement::Query,
                prefix: "Session ".into(),
            },
        }),
    };
    plan = Plan::new(
        Some(&auth),
        "project",
        &plan.project,
        None,
        &plan.definitions,
    )
    .unwrap()
    .unwrap();
    let manager = Manager::default();
    let first = ensure(&manager, &plan, values.clone(), false)
        .await
        .unwrap();
    assert_eq!(first.token, "access-one");
    let request = captured.recv().await.unwrap();
    assert!(request.contains("/?q=a+%2B%26"));
    assert!(request.to_lowercase().contains(
        &format!(
            "x-composed: Basic {}",
            STANDARD.encode("synthetic:client +:tail")
        )
        .to_lowercase()
    ));
    assert!(request.ends_with("{\"synthetic\":true}"));
    let second = ensure(&manager, &plan, values.clone(), true).await.unwrap();
    assert_eq!(second.renewal_token.as_deref(), Some("renewal-two"));
    assert!(captured
        .recv()
        .await
        .unwrap()
        .ends_with("renew=renewal-one%7Caccess-one"));
    let mut request = client().get("https://example.test/api").build().unwrap();
    plan.apply(&mut request, &second).unwrap();
    assert_eq!(
        request
            .url()
            .query_pairs()
            .find(|(n, _)| n == "access")
            .unwrap()
            .1,
        "Session access-two"
    );
    task.await.unwrap();
}

#[tokio::test]
async fn extraction_storage_and_response_limits_preserve_usable_tokens() {
    let huge_token = format!(
        "{{\"access_token\":\"{}\",\"token_type\":\"Bearer\"}}",
        "x".repeat(2600)
    );
    let (url, _, task) = sequence(vec![
        oauth("kept-synthetic"),
        response("200 OK", "", b"invalid json private"),
        response("200 OK", "", huge_token.as_bytes()),
        oauth("not-stored"),
    ])
    .await;
    let (plan, values) = fixture(&url, ClientAuthentication::Basic);
    let manager = Manager::default();
    ensure(&manager, &plan, values.clone(), false)
        .await
        .unwrap();
    let previous = values.get(&plan.key).unwrap();
    for _ in 0..2 {
        let error = ensure(&manager, &plan, values.clone(), true)
            .await
            .err()
            .unwrap();
        assert!(!error.contains("private"));
        assert_eq!(values.get(&plan.key).unwrap(), previous);
    }
    values.write_denied.store(true, Ordering::SeqCst);
    assert!(ensure(&manager, &plan, values.clone(), true)
        .await
        .err()
        .unwrap()
        .contains("Could not store"));
    assert_eq!(values.get(&plan.key).unwrap(), previous);
    task.await.unwrap();
    values.denied.store(true, Ordering::SeqCst);
    let error = manager.status(&plan, values).await.err().unwrap();
    assert!(!error.contains("private"));
    assert!(error.contains("OS token store"));
    assert_eq!(
        extract(
            &Extraction::Text,
            b"  synthetic-text\n",
            &HeaderMap::new(),
            false
        )
        .unwrap(),
        "synthetic-text"
    );
    assert!(extract(
        &Extraction::Json {
            pointer: "/missing".into()
        },
        b"{}",
        &HeaderMap::new(),
        false
    )
    .is_err());
    let (url, _, task) = server(
        response("200 OK", "", &vec![b'x'; RESPONSE_LIMIT + 1]),
        Duration::ZERO,
    )
    .await;
    let (plan, values) = fixture(&url, ClientAuthentication::Basic);
    assert!(ensure(&manager, &plan, values.clone(), false)
        .await
        .err()
        .unwrap()
        .contains("1 MiB"));
    assert!(values.get(&plan.key).unwrap().is_none());
    task.await.unwrap();
}

#[tokio::test]
async fn cancellation_timeout_manual_priority_and_api_rejection_do_not_replay() {
    let (url, _, task) = server(oauth("delayed"), Duration::from_millis(100)).await;
    let (plan, values) = fixture(&url, ClientAuthentication::Basic);
    let manager = Manager::default();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        manager
            .ensure(
                &plan,
                &client(),
                values.clone(),
                false,
                &cancel,
                Duration::from_secs(1)
            )
            .await
            .err()
            .unwrap(),
        CANCELLED
    );
    assert_eq!(values.reads.load(Ordering::SeqCst), 0);
    assert_eq!(
        manager
            .ensure(
                &plan,
                &client(),
                values.clone(),
                false,
                &CancellationToken::new(),
                Duration::from_millis(10)
            )
            .await
            .err()
            .unwrap(),
        TIMED_OUT
    );
    task.await.unwrap();
    let (url, captured, task) = server(
        response(
            "401 Unauthorized",
            "Content-Type: text/plain\r\n",
            b"original API rejection",
        ),
        Duration::ZERO,
    )
    .await;
    let record = Record {
        schema_version: 1,
        token: "synthetic".into(),
        renewal_token: None,
        acquired_at: now().unwrap(),
        expires_at: None,
    };
    let mut request = client()
        .get(&url)
        .header("Authorization", "manual")
        .build()
        .unwrap();
    assert!(plan.overridden(request.url(), request.headers()));
    plan.apply(&mut request, &record).unwrap();
    assert_eq!(request.headers()["authorization"], "manual");
    let prepared = crate::http::prepare::Prepared {
        display_headers: vec![],
        header_names: vec![],
        request,
        token: None,
    };
    let result = transfer::execute(
        &client(),
        prepared,
        Duration::from_secs(1),
        &CancellationToken::new(),
        None,
    )
    .await;
    assert_eq!(result.status, Some(401));
    assert_eq!(result.text.as_deref(), Some("original API rejection"));
    captured.await.unwrap();
    task.await.unwrap();
    assert_eq!(values.get(&plan.key).unwrap(), None);
}

#[test]
fn configuration_validation_and_prepare_check_scope_before_token_access() {
    assert!(endpoint("http://example.test/token").is_err());
    assert!(endpoint("https://user:private@example.test/token").is_err());
    assert!(endpoint("http://127.0.0.1/token").is_ok());
    assert!(endpoint("http://[::1]/token").is_ok());
    let (plan, values) = fixture("https://example.test/token", ClientAuthentication::Basic);
    let root = tempfile::tempdir().unwrap();
    let settings = tempfile::tempdir().unwrap();
    let mut store = crate::projects::store::ProjectStore::default();
    let project = store.create(root.path(), "Synthetic".into()).unwrap();
    let project = store
        .save_auth(
            &project.handle,
            Some(plan.config.clone()),
            &project.revision,
        )
        .unwrap();
    let mut input = input("https://example.test/api");
    input.scope = Some(crate::http::model::Scope {
        handle: project.handle.clone(),
        environment: None,
        revision: "missing".into(),
        project_revision: project.revision,
        collection_id: None,
        collection_revision: None,
    });
    let prepared = crate::http::prepare::prepare(
        &input,
        Some(root.path()),
        &project.data.id,
        settings.path(),
        &client(),
        values.as_ref(),
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(prepared.token.is_some());
    assert_eq!(values.reads.load(Ordering::SeqCst), 0);
    input.request.headers.push(Entry {
        id: 1,
        enabled: true,
        key: "Authorization".into(),
        value: "manual".into(),
    });
    assert!(crate::http::prepare::prepare(
        &input,
        Some(root.path()),
        &project.data.id,
        settings.path(),
        &client(),
        values.as_ref(),
        &CancellationToken::new()
    )
    .unwrap()
    .token
    .is_none());
    input.scope.as_mut().unwrap().project_revision = "stale".into();
    assert!(crate::http::prepare::token_plan(
        &input,
        root.path(),
        &project.data.id,
        settings.path()
    )
    .is_err());
    assert_eq!(values.reads.load(Ordering::SeqCst), 0);
    let encoded = serde_json::to_value(&plan.config).unwrap();
    assert!(serde_json::from_value::<Auth>(encoded.clone()).is_ok());
    let mut invalid = encoded;
    invalid["config"]["secret"] = serde_json::json!("literal private");
    assert!(serde_json::from_value::<Auth>(invalid).is_err());
    let ids = plan.config.variable_ids();
    assert!(ids.contains(&plan.definitions.variables[0].id.as_str()));
    let mut expression = text("safe");
    expression.parts = vec![Part::AccessToken];
    let mut config = match plan.config {
        Auth::Oauth2 { config } => config,
        _ => unreachable!(),
    };
    config.token_url = expression;
    assert!(config.validate().is_err());
}

#[tokio::test]
async fn authentication_test_acquires_fresh_token_without_reading_or_mutating_stored_tokens() {
    let (url, captured, task) = server(oauth("synthetic-test-only-token"), Duration::ZERO).await;
    let (plan, values) = fixture(&url, ClientAuthentication::Basic);
    values.set(&plan.key, "not-a-valid-stored-record").unwrap();
    values.write_denied.store(true, Ordering::SeqCst);
    let before = values.data.lock().unwrap().clone();
    let status = test(
        plan.clone(),
        client(),
        values.clone(),
        CancellationToken::new(),
        Duration::from_secs(2),
    )
    .await
    .unwrap();
    assert_ne!(status.state, "missing");
    let raw = String::from_utf8(captured.await.unwrap())
        .unwrap()
        .to_ascii_lowercase();
    task.await.unwrap();
    assert!(raw.contains("grant_type=client_credentials"));
    assert!(raw.contains(concat!("user-agent: fluxkit/", env!("CARGO_PKG_VERSION"))));
    assert_eq!(*values.data.lock().unwrap(), before);
    assert!(!serde_json::to_string(&status)
        .unwrap()
        .contains("synthetic-test-only-token"));
}

#[tokio::test]
async fn authentication_tests_report_rejection_timeout_and_cancel_without_changing_values() {
    for (reply, delay, timeout, cancelled) in [
        (
            response("401 Unauthorized", "", b"synthetic-private-response"),
            Duration::ZERO,
            Duration::from_secs(1),
            false,
        ),
        (
            oauth("late-token"),
            Duration::from_millis(200),
            Duration::from_millis(30),
            false,
        ),
        (
            oauth("cancelled-token"),
            Duration::ZERO,
            Duration::from_secs(1),
            true,
        ),
    ] {
        let (url, _, task) = server(reply, delay).await;
        let (plan, values) = fixture(&url, ClientAuthentication::Body);
        let before = values.data.lock().unwrap().clone();
        let cancel = CancellationToken::new();
        if cancelled {
            cancel.cancel();
        }
        let error = test(plan, client(), values.clone(), cancel, timeout)
            .await
            .err()
            .unwrap();
        assert!(!error.contains("synthetic-private-response"));
        assert!(!error.contains("late-token"));
        assert_eq!(*values.data.lock().unwrap(), before);
        task.abort();
    }
}

/// Exercise saved auth, selected-environment preparation, token storage and actual wire headers.
#[tokio::test]
async fn saved_request_switches_test_prod_test_without_cross_environment_tokens() {
    for custom_login in [false, true] {
        for prod_rejected in [false, true] {
            let mut replies = vec![
                oauth("synthetic-test-token"),
                response("204 No Content", "", b""),
            ];
            replies.push(if prod_rejected {
                response("401 Unauthorized", "", b"synthetic rejection")
            } else {
                oauth("synthetic-prod-token")
            });
            if !prod_rejected {
                replies.push(response("204 No Content", "", b""));
            }
            replies.push(response("204 No Content", "", b""));
            let (url, mut captured, task) = sequence(replies).await;
            let root = tempfile::tempdir().unwrap();
            let settings = tempfile::tempdir().unwrap();
            let mut store = crate::projects::store::ProjectStore::default();
            let project = store
                .create(root.path(), "Environment isolation".into())
                .unwrap();
            let variable = |name: &str| environments::Definition {
                id: Uuid::new_v4().to_string(),
                name: name.into(),
                secret: None,
            };
            let client_id = variable("clientId");
            let secret = variable("secret");
            let endpoint = variable("tokenUrl");
            let api = variable("baseUrl");
            let test_environment = variable("Test");
            let prod_environment = variable("Prod");
            let definitions = Definitions {
                schema_version: 1,
                variables: vec![
                    client_id.clone(),
                    secret.clone(),
                    endpoint.clone(),
                    api.clone(),
                ],
                environments: vec![test_environment.clone(), prod_environment.clone()],
            };
            let composition = |id: &str| Composition {
                parts: vec![Part::Variable { id: id.into() }],
                ..text("")
            };
            let auth = if custom_login {
                Auth::CustomLogin {
                    config: Box::new(CustomLogin {
                        login: LoginRequest {
                            url: composition(&endpoint.id),
                            method: "POST".into(),
                            params: vec![],
                            headers: vec![],
                            body: LoginBody::UrlEncoded {
                                fields: vec![
                                    LoginEntry {
                                        name: "client_id".into(),
                                        value: composition(&client_id.id),
                                    },
                                    LoginEntry {
                                        name: "client_secret".into(),
                                        value: composition(&secret.id),
                                    },
                                ],
                            },
                            success_statuses: vec![200],
                            token: Extraction::Json {
                                pointer: "/access_token".into(),
                            },
                            renewal_token: None,
                            expires_in: Some(Extraction::Json {
                                pointer: "/expires_in".into(),
                            }),
                            lifetime_seconds: None,
                        },
                        renewal: None,
                        placement: TokenPlacement {
                            name: "Authorization".into(),
                            placement: KeyPlacement::Header,
                            prefix: "Bearer ".into(),
                        },
                    }),
                }
            } else {
                Auth::Oauth2 {
                    config: Box::new(OAuth2 {
                        token_url: composition(&endpoint.id),
                        client_id: client_id.id.clone(),
                        client_secret: secret.id.clone(),
                        scope: text(""),
                        client_authentication: ClientAuthentication::Body,
                    }),
                }
            };
            let values = Arc::new(Store::default());
            environments::apply(
                root.path(),
                settings.path(),
                &project.data.id,
                environments::Action::Save {
                    data: definitions,
                    expected: "missing".into(),
                },
                values.as_ref(),
            )
            .unwrap();
            for (environment, name) in [
                (&test_environment.id, "test"),
                (&prod_environment.id, "prod"),
            ] {
                for (id, value) in [
                    (&client_id.id, format!("{name}-client")),
                    (&secret.id, format!("{name}-secret")),
                    (&endpoint.id, format!("{url}/{name}/token")),
                    (&api.id, format!("{url}/{name}/api")),
                ] {
                    values
                        .set(&format!("{}/{id}/{environment}", project.data.id), &value)
                        .unwrap();
                }
            }
            // Poison defaults ensure the selected environment wins for every input.
            for definition in [&client_id, &secret, &endpoint, &api] {
                values
                    .set(
                        &format!("{}/{}/default", project.data.id, definition.id),
                        "shared-default-must-not-be-used",
                    )
                    .unwrap();
            }
            let mut saved = input("{{baseUrl}}").request;
            saved.auth = None;
            let collection = Collection {
                schema_version: 1,
                id: Uuid::new_v4().to_string(),
                name: "Saved API".into(),
                auth: Some(auth),
                folders: vec![],
                requests: vec![saved],
                open_api: None,
                postman: None,
            };
            let collection = store
                .save_collection(&project.handle, collection, None)
                .unwrap();
            let transport = client();
            let manager = Manager::default();
            let restarted = Manager::default();
            let mut test_key = String::new();
            let mut test_record = None;
            let mut previous_scope = None;
            for (index, (environment, name)) in [
                (&test_environment.id, "test"),
                (&prod_environment.id, "prod"),
                (&test_environment.id, "test"),
            ]
            .into_iter()
            .enumerate()
            {
                let view = environments::apply(
                    root.path(),
                    settings.path(),
                    &project.data.id,
                    environments::Action::Select {
                        environment: Some(environment.clone()),
                    },
                    values.as_ref(),
                )
                .unwrap()
                .view;
                let mut request = input("unused");
                request.request = collection.data.requests[0].clone();
                request.scope = Some(crate::http::model::Scope {
                    handle: project.handle.clone(),
                    environment: Some(environment.clone()),
                    revision: view.revision,
                    project_revision: project.revision.clone(),
                    collection_id: Some(collection.data.id.clone()),
                    collection_revision: Some(collection.revision.clone()),
                });
                if index == 1 {
                    let reads = values.reads.load(Ordering::SeqCst);
                    let mut stale = input("unused");
                    stale.request = request.request.clone();
                    stale.scope = previous_scope.take();
                    assert!(crate::http::prepare::prepare(
                        &stale,
                        Some(root.path()),
                        &project.data.id,
                        settings.path(),
                        &transport,
                        values.as_ref(),
                        &CancellationToken::new()
                    )
                    .is_err());
                    assert_eq!(values.reads.load(Ordering::SeqCst), reads);
                }
                let mut prepared = crate::http::prepare::prepare(
                    &request,
                    Some(root.path()),
                    &project.data.id,
                    settings.path(),
                    &transport,
                    values.as_ref(),
                    &CancellationToken::new(),
                )
                .unwrap();
                previous_scope = request.scope.take();
                assert!(prepared
                    .request
                    .url()
                    .path()
                    .starts_with(&format!("/{name}/")));
                let plan = prepared.token.take().unwrap();
                if index == 0 {
                    test_key = plan.key.clone();
                }
                if index == 1 {
                    assert_ne!(test_key, plan.key);
                    assert_eq!(
                        manager.status(&plan, values.clone()).await.unwrap().state,
                        "missing"
                    );
                }
                let result = ensure(
                    if index == 2 { &restarted } else { &manager },
                    &plan,
                    values.clone(),
                    false,
                )
                .await;
                if index < 2 {
                    let login = tokio::time::timeout(Duration::from_secs(3), captured.recv())
                        .await
                        .unwrap()
                        .unwrap();
                    assert!(login.starts_with(&format!("POST /{name}/token ")));
                    assert!(login.contains(&format!("client_id={name}-client")));
                    assert!(login.contains(&format!("client_secret={name}-secret")));
                    assert!(!login.contains(if name == "test" {
                        "prod-secret"
                    } else {
                        "test-secret"
                    }));
                }
                if index == 1 && prod_rejected {
                    assert!(result.err().unwrap().contains("401"));
                    assert_eq!(
                        manager.status(&plan, values.clone()).await.unwrap().state,
                        "missing"
                    );
                    assert_eq!(values.get(&test_key).unwrap(), test_record);
                    continue;
                }
                let record = result.unwrap();
                assert_eq!(record.token, format!("synthetic-{name}-token"));
                plan.apply(&mut prepared.request, &record).unwrap();
                let result = transfer::execute(
                    &transport,
                    prepared,
                    Duration::from_secs(3),
                    &CancellationToken::new(),
                    None,
                )
                .await;
                assert_eq!(result.status, Some(204));
                let api = tokio::time::timeout(Duration::from_secs(3), captured.recv())
                    .await
                    .unwrap()
                    .unwrap();
                assert!(api.starts_with(&format!("GET /{name}/api ")));
                assert!(api
                    .to_ascii_lowercase()
                    .contains(&format!("authorization: bearer synthetic-{name}-token")));
                assert!(!api.contains(if name == "test" {
                    "synthetic-prod-token"
                } else {
                    "synthetic-test-token"
                }));
                if index == 0 {
                    test_record = values.get(&test_key).unwrap();
                }
                if index == 2 {
                    assert_eq!(values.get(&test_key).unwrap(), test_record);
                }
            }
            tokio::time::timeout(Duration::from_secs(3), task)
                .await
                .unwrap()
                .unwrap();
            let token_count = values
                .data
                .lock()
                .unwrap()
                .keys()
                .filter(|key| key.starts_with("tokens/v1/"))
                .count();
            assert_eq!(token_count, if prod_rejected { 1 } else { 2 });
        }
    }
}

#[tokio::test]
async fn overlapping_environment_acquisitions_do_not_share_credentials_or_tokens() {
    let (test_url, test_capture, test_task) =
        server(oauth("synthetic-test-overlap"), Duration::from_millis(80)).await;
    let (prod_url, prod_capture, prod_task) =
        server(oauth("synthetic-prod-overlap"), Duration::ZERO).await;
    let (original, values) = fixture(&test_url, ClientAuthentication::Body);
    let test_id = Uuid::new_v4().to_string();
    let prod_id = Uuid::new_v4().to_string();
    let endpoint = Uuid::new_v4().to_string();
    let mut definitions = original.definitions.clone();
    definitions.variables.push(environments::Definition {
        id: endpoint.clone(),
        name: "tokenUrl".into(),
        secret: None,
    });
    definitions.environments = vec![
        environments::Definition {
            id: test_id.clone(),
            name: "Test".into(),
            secret: None,
        },
        environments::Definition {
            id: prod_id.clone(),
            name: "Prod".into(),
            secret: None,
        },
    ];
    let mut auth = original.config.clone();
    if let Auth::Oauth2 { config } = &mut auth {
        config.token_url = Composition {
            parts: vec![Part::Variable {
                id: endpoint.clone(),
            }],
            ..text("")
        };
    }
    for (environment, name, url) in [(&test_id, "test", test_url), (&prod_id, "prod", prod_url)] {
        for (id, value) in [
            (&definitions.variables[0].id, format!("{name}-client")),
            (&definitions.variables[1].id, format!("{name}-secret")),
            (&endpoint, url),
        ] {
            values
                .set(&format!("{}/{id}/{environment}", original.project), &value)
                .unwrap();
        }
    }
    let test = Plan::new(
        Some(&auth),
        "project",
        &original.project,
        Some(&test_id),
        &definitions,
    )
    .unwrap()
    .unwrap();
    let prod = Plan::new(
        Some(&auth),
        "project",
        &original.project,
        Some(&prod_id),
        &definitions,
    )
    .unwrap()
    .unwrap();
    let manager = Manager::default();
    let (a, b) = tokio::join!(
        ensure(&manager, &test, values.clone(), false),
        ensure(&manager, &prod, values.clone(), false)
    );
    assert_eq!(a.unwrap().token, "synthetic-test-overlap");
    assert_eq!(b.unwrap().token, "synthetic-prod-overlap");
    let test_wire = String::from_utf8(test_capture.await.unwrap()).unwrap();
    let prod_wire = String::from_utf8(prod_capture.await.unwrap()).unwrap();
    assert!(test_wire.contains("client_id=test-client&client_secret=test-secret"));
    assert!(!test_wire.contains("prod-secret"));
    assert!(prod_wire.contains("client_id=prod-client&client_secret=prod-secret"));
    assert!(!prod_wire.contains("test-secret"));
    assert_ne!(
        values.get(&test.key).unwrap(),
        values.get(&prod.key).unwrap()
    );
    test_task.await.unwrap();
    prod_task.await.unwrap();
}
