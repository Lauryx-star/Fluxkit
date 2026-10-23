use super::{
    model::*,
    prepare,
    transfer::{self, Download, PREVIEW_LIMIT},
};
use crate::projects::{
    environments::{self, Action, Definition, Definitions, Values},
    model::*,
};
use reqwest::Client;
use std::{collections::HashMap, path::Path, time::Duration};
use tempfile::tempdir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct Fake(HashMap<String, String>);
impl Values for Fake {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        Ok(self.0.get(key).cloned())
    }
    fn set(&self, _: &str, _: &str) -> Result<(), String> {
        Err("read-only fixture".into())
    }
    fn delete(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}
fn client() -> Client {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .build()
        .unwrap()
}
fn input(url: &str) -> ExecutionInput {
    ExecutionInput {
        request: Request {
            auth: None,
            id: Uuid::new_v4().to_string(),
            name: "Synthetic request".into(),
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
        },
        scope: None,
        timeout_ms: 60_000,
        save_response: false,
    }
}
async fn server(
    response: Vec<u8>,
    delay: Duration,
) -> (
    String,
    oneshot::Receiver<Vec<u8>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = vec![];
        let mut buffer = [0; 8192];
        loop {
            let count = socket.read(&mut buffer).await.unwrap();
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(split) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..split]).to_ascii_lowercase();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length: ")
                            .and_then(|s| s.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if bytes.len() >= split + 4 + length {
                    break;
                }
            }
        }
        let _ = tx.send(bytes);
        tokio::time::sleep(delay).await;
        let _ = socket.write_all(&response).await;
    });
    (format!("http://{address}"), rx, task)
}
fn response(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
        body.len()
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    bytes
}
async fn send(
    input: &ExecutionInput,
    client: &Client,
    root: Option<&Path>,
    project: &str,
    settings: &Path,
    values: &impl Values,
    download: Option<Download>,
) -> ExecutionResult {
    let token = CancellationToken::new();
    let prepared =
        prepare::prepare(input, root, project, settings, client, values, &token).unwrap();
    transfer::execute(client, prepared, Duration::from_secs(3), &token, download).await
}

#[tokio::test]
async fn sends_method_query_headers_body_and_resolves_values_before_dispatch() {
    let (url, captured, task) = server(
        response(
            "201 Created",
            "Content-Type: application/json\r\nX-Example: synthetic\r\n",
            br#"{"ok":true}"#,
        ),
        Duration::ZERO,
    )
    .await;
    let root = tempdir().unwrap();
    let settings = tempdir().unwrap();
    let project = Uuid::new_v4().to_string();
    let variable = Uuid::new_v4().to_string();
    let token_var = Uuid::new_v4().to_string();
    let environment = Uuid::new_v4().to_string();
    let definitions = Definitions {
        schema_version: 1,
        environments: vec![Definition {
            id: environment.clone(),
            name: "Testserver".into(),
        }],
        variables: vec![
            Definition {
                id: variable.clone(),
                name: "baseUrl".into(),
            },
            Definition {
                id: token_var.clone(),
                name: "token".into(),
            },
        ],
    };
    let values = Fake(HashMap::from([
        (format!("{project}/{variable}/default"), url),
        (
            format!("{project}/{token_var}/default"),
            "default-token-must-not-be-sent".into(),
        ),
        (
            format!("{project}/{token_var}/{environment}"),
            "synthetic-token".into(),
        ),
    ]));
    let view = environments::apply(
        root.path(),
        settings.path(),
        &project,
        Action::Save {
            data: definitions,
            expected: "missing".into(),
        },
        &values,
    )
    .unwrap()
    .view;
    environments::apply(
        root.path(),
        settings.path(),
        &project,
        Action::Select {
            environment: Some(environment.clone()),
        },
        &values,
    )
    .unwrap();
    let mut input = input("{{baseUrl}}/items?existing=1");
    let manifest =
        serde_json::to_vec(&serde_json::json!({"schemaVersion":1,"id":project,"name":"Synthetic"}))
            .unwrap();
    std::fs::write(root.path().join("project.fluxkit.json"), &manifest).unwrap();
    input.scope = Some(Scope {
        handle: "authorized-by-command".into(),
        project_revision: crate::projects::store::revision(&manifest),
        collection_id: None,
        collection_revision: None,
        environment: Some(environment),
        revision: view.revision,
    });
    input.request.method = "POST".into();
    input.request.params = vec![
        Entry {
            id: 1,
            enabled: true,
            key: "name".into(),
            value: "A B&Ü".into(),
        },
        Entry {
            id: 2,
            enabled: false,
            key: "disabled".into(),
            value: "{{missing}}".into(),
        },
    ];
    input.request.auth = Some(Auth::Bearer { token: token_var });
    input.request.body.kind = BodyKind::Json;
    input.request.body.text = r#"{"value":"{{token}}"}"#.into();
    let result = send(
        &input,
        &client(),
        Some(root.path()),
        &project,
        settings.path(),
        &values,
        None,
    )
    .await;
    let raw = String::from_utf8(captured.await.unwrap()).unwrap();
    task.await.unwrap();
    assert!(raw.starts_with("POST /items?existing=1&name=A+B%26%C3%9C HTTP/1.1"));
    assert!(raw
        .to_ascii_lowercase()
        .contains("authorization: bearer synthetic-token"));
    assert!(raw.contains(r#"{"value":"synthetic-token"}"#));
    assert!(!raw.contains("disabled"));
    assert!(!raw.contains("default-token-must-not-be-sent"));
    assert_eq!(result.status, Some(201));
    assert_eq!(result.outcome, Outcome::Response);
    assert_eq!(result.text.as_deref(), Some(r#"{"ok":true}"#));
    assert!(result.json_text.as_deref().unwrap().contains('\n'));
    assert!(result.headers.iter().any(|h| h.name == "x-example"));
    let value = serde_json::to_value(&result).unwrap();
    assert!(value.get("elapsedMs").is_some());
    assert!(value.get("byteCount").is_some());
}
#[tokio::test]
async fn redirects_are_returned_without_forwarding_credentials_and_cookies_are_not_reused() {
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = destination.local_addr().unwrap();
    let (url, _, task) = server(
        response(
            "302 Found",
            &format!("Location: http://{target}/other\r\nSet-Cookie: session=synthetic\r\n"),
            b"",
        ),
        Duration::ZERO,
    )
    .await;
    let settings = tempdir().unwrap();
    let mut request = input(&url);
    request.request.headers.push(Entry {
        id: 1,
        enabled: true,
        key: "Authorization".into(),
        value: "Bearer synthetic".into(),
    });
    let client = client();
    let result = send(
        &request,
        &client,
        None,
        "session",
        settings.path(),
        &Fake(HashMap::new()),
        None,
    )
    .await;
    assert_eq!(result.status, Some(302));
    task.await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(60), destination.accept())
            .await
            .is_err()
    );
    let (url, captured, task) = server(response("204 No Content", "", b""), Duration::ZERO).await;
    let result = send(
        &input(&url),
        &client,
        None,
        "session",
        settings.path(),
        &Fake(HashMap::new()),
        None,
    )
    .await;
    assert!(!String::from_utf8_lossy(&captured.await.unwrap())
        .to_ascii_lowercase()
        .contains("cookie:"));
    assert_eq!(result.byte_count, 0);
    assert!(matches!(result.content_kind, ContentKind::Empty));
    task.await.unwrap();
}
#[tokio::test]
async fn text_preview_is_bounded_and_binary_download_streams_exact_bytes() {
    let text = vec![b'x'; PREVIEW_LIMIT + 128];
    let (url, _, task) = server(
        response("200 OK", "Content-Type: text/plain\r\n", &text),
        Duration::ZERO,
    )
    .await;
    let settings = tempdir().unwrap();
    let result = send(
        &input(&url),
        &client(),
        None,
        "session",
        settings.path(),
        &Fake(HashMap::new()),
        None,
    )
    .await;
    assert_eq!(result.byte_count, text.len() as u64);
    assert_eq!(result.text.unwrap().len(), PREVIEW_LIMIT);
    assert!(result.truncated);
    task.await.unwrap();
    let bytes: Vec<u8> = (0..2 * PREVIEW_LIMIT).map(|n| (n % 251) as u8).collect();
    let (url, _, task) = server(
        response(
            "200 OK",
            "Content-Type: application/octet-stream\r\n",
            &bytes,
        ),
        Duration::ZERO,
    )
    .await;
    let output = tempdir().unwrap();
    let target = output.path().join("synthetic download.bin");
    let result = send(
        &input(&url),
        &client(),
        None,
        "session",
        settings.path(),
        &Fake(HashMap::new()),
        Some(Download::new(target.clone()).unwrap()),
    )
    .await;
    assert!(result.saved);
    assert!(matches!(result.content_kind, ContentKind::Binary));
    assert!(result.text.is_none());
    assert_eq!(std::fs::read(target).unwrap(), bytes);
    task.await.unwrap();
}
#[tokio::test]
async fn cancellation_timeout_and_transport_failure_are_distinct_and_preserve_existing_download() {
    let settings = tempdir().unwrap();
    let output = tempdir().unwrap();
    let target = output.path().join("existing.bin");
    std::fs::write(&target, b"previous").unwrap();
    let (url, received, task) =
        server(response("200 OK", "", b"late"), Duration::from_secs(5)).await;
    let client = client();
    let token = CancellationToken::new();
    let prepared = prepare::prepare(
        &input(&url),
        None,
        "session",
        settings.path(),
        &client,
        &Fake(HashMap::new()),
        &token,
    )
    .unwrap();
    let execution = transfer::execute(
        &client,
        prepared,
        Duration::from_secs(3),
        &token,
        Some(Download::new(target.clone()).unwrap()),
    );
    let cancel = async {
        received.await.unwrap();
        token.cancel();
    };
    let (result, _) = tokio::join!(execution, cancel);
    task.abort();
    assert_eq!(result.outcome, Outcome::Cancelled);
    assert!(!result.saved);
    assert_eq!(std::fs::read(&target).unwrap(), b"previous");
    assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 1);
    let (url, _, task) = server(response("200 OK", "", b"late"), Duration::from_secs(5)).await;
    let prepared = prepare::prepare(
        &input(&url),
        None,
        "session",
        settings.path(),
        &client,
        &Fake(HashMap::new()),
        &token.clone().child_token(),
    )
    .err()
    .unwrap();
    assert!(prepared.contains("cancelled"));
    task.abort();
    let (url, _, task) = server(response("200 OK", "", b"late"), Duration::from_secs(5)).await;
    let token = CancellationToken::new();
    let prepared = prepare::prepare(
        &input(&url),
        None,
        "session",
        settings.path(),
        &client,
        &Fake(HashMap::new()),
        &token,
    )
    .unwrap();
    let result =
        transfer::execute(&client, prepared, Duration::from_millis(25), &token, None).await;
    task.abort();
    assert_eq!(result.outcome, Outcome::Timeout);
    assert_eq!(result.status, None);
    let (url, _, task) = server(Vec::new(), Duration::ZERO).await;
    let result = send(
        &input(&format!("{url}/sensitive?token=synthetic-private")),
        &client,
        None,
        "session",
        settings.path(),
        &Fake(HashMap::new()),
        None,
    )
    .await;
    assert_eq!(result.outcome, Outcome::NetworkError);
    assert_eq!(result.status, None);
    assert!(!result.message.unwrap().contains("synthetic-private"));
    task.await.unwrap();
}
#[tokio::test]
async fn form_multipart_and_file_bodies_are_sent_and_unsafe_inputs_are_blocked() {
    let settings = tempdir().unwrap();
    let root = tempdir().unwrap();
    std::fs::create_dir(root.path().join("attachments")).unwrap();
    let reference = Attachment {
        path: format!("attachments/{}.bin", Uuid::new_v4()),
        name: "synthetic.txt".into(),
    };
    std::fs::write(root.path().join(&reference.path), b"file-content").unwrap();
    for kind in [BodyKind::UrlEncoded, BodyKind::Multipart, BodyKind::File] {
        let (url, captured, task) = server(response("200 OK", "", b"done"), Duration::ZERO).await;
        let mut request = input(&url);
        request.request.method = "POST".into();
        request.request.body.kind = kind.clone();
        request.request.body.fields = vec![
            FormField {
                id: 1,
                enabled: true,
                key: "name".into(),
                value: "A B".into(),
                file: None,
            },
            FormField {
                id: 2,
                enabled: true,
                key: "upload".into(),
                value: String::new(),
                file: if matches!(kind, BodyKind::Multipart) {
                    Some(reference.clone())
                } else {
                    None
                },
            },
        ];
        request.request.body.file = Some(reference.clone());
        let result = send(
            &request,
            &client(),
            Some(root.path()),
            "project",
            settings.path(),
            &Fake(HashMap::new()),
            None,
        )
        .await;
        assert_eq!(result.outcome, Outcome::Response);
        let bytes = captured.await.unwrap();
        let raw = String::from_utf8_lossy(&bytes);
        match kind {
            BodyKind::UrlEncoded => assert!(raw.contains("name=A+B")),
            BodyKind::Multipart => {
                assert!(raw.contains("multipart/form-data; boundary="));
                assert!(raw.contains("file-content"));
            }
            BodyKind::File => assert!(raw.ends_with("file-content")),
            _ => unreachable!(),
        }
        task.await.unwrap();
    }
    for url in [
        "file:///private/secret",
        "http://user:pass@127.0.0.1",
        "{{missing}}",
    ] {
        assert!(prepare::prepare(
            &input(url),
            None,
            "session",
            settings.path(),
            &client(),
            &Fake(HashMap::new()),
            &CancellationToken::new()
        )
        .is_err());
    }
    let mut malformed = input("http://127.0.0.1");
    malformed.request.headers.push(Entry {
        id: 1,
        enabled: true,
        key: "X-Test".into(),
        value: "injected\r\nOther: bad".into(),
    });
    assert!(prepare::prepare(
        &malformed,
        None,
        "session",
        settings.path(),
        &client(),
        &Fake(HashMap::new()),
        &CancellationToken::new()
    )
    .is_err());
}

#[tokio::test]
async fn interrupted_bodies_preserve_received_status_and_changed_destinations_are_not_replaced() {
    let settings = tempdir().unwrap();
    let (url, _, task) = server(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 100\r\nConnection: close\r\n\r\nabc".to_vec(), Duration::ZERO).await;
    let result = send(
        &input(&url),
        &client(),
        None,
        "session",
        settings.path(),
        &Fake(HashMap::new()),
        None,
    )
    .await;
    assert_eq!(result.outcome, Outcome::NetworkError);
    assert_eq!(result.status, Some(200));
    assert_eq!(result.byte_count, 3);
    assert!(result.truncated);
    task.await.unwrap();
    let output = tempdir().unwrap();
    let target = output.path().join("changed.bin");
    std::fs::write(&target, b"old").unwrap();
    let download = Download::new(target.clone()).unwrap();
    std::fs::write(&target, b"changed externally").unwrap();
    assert!(download.commit().is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"changed externally");
    assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn inherited_authentication_uses_variable_ids_and_manual_fields_win() {
    use crate::projects::store::ProjectStore;
    let root = tempdir().unwrap();
    let settings = tempdir().unwrap();
    let mut store = ProjectStore::default();
    let project = store.create(root.path(), "Auth fixture".into()).unwrap();
    let user = Uuid::new_v4().to_string();
    let password = Uuid::new_v4().to_string();
    let token = Uuid::new_v4().to_string();
    let values = Fake(HashMap::from([
        (
            format!("{}/{user}/default", project.data.id),
            "synthetic-user".into(),
        ),
        (
            format!("{}/{password}/default", project.data.id),
            "synthetic-password".into(),
        ),
        (
            format!("{}/{token}/default", project.data.id),
            "synthetic-key /+".into(),
        ),
    ]));
    let definitions = Definitions {
        schema_version: 1,
        environments: vec![],
        variables: vec![
            Definition {
                id: user.clone(),
                name: "renamedUsername".into(),
            },
            Definition {
                id: password.clone(),
                name: "password".into(),
            },
            Definition {
                id: token.clone(),
                name: "token".into(),
            },
        ],
    };
    let env = environments::apply(
        root.path(),
        settings.path(),
        &project.data.id,
        Action::Save {
            data: definitions,
            expected: "missing".into(),
        },
        &values,
    )
    .unwrap()
    .view;
    let parent = Uuid::new_v4().to_string();
    let child = Uuid::new_v4().to_string();
    let collection = Collection {
        schema_version: 1,
        id: Uuid::new_v4().to_string(),
        name: "API".into(),
        auth: None,
        folders: vec![
            Folder {
                id: parent.clone(),
                name: "Private".into(),
                parent_id: None,
                auth: Some(Auth::Basic {
                    username: user.clone(),
                    password: password.clone(),
                }),
            },
            Folder {
                id: child.clone(),
                name: "Nested".into(),
                parent_id: Some(parent),
                auth: None,
            },
        ],
        requests: vec![],
    };
    let collection = store
        .save_collection(&project.handle, collection, None)
        .unwrap();
    let project = store
        .save_auth(
            &project.handle,
            Some(Auth::Bearer {
                token: token.clone(),
            }),
            &project.revision,
        )
        .unwrap();
    let (url, received, task) = server(response("200 OK", "", b""), Duration::ZERO).await;
    let mut input = input(&url);
    input.request.folder_id = Some(child.clone());
    input.scope = Some(Scope {
        handle: project.handle.clone(),
        environment: None,
        revision: env.revision,
        project_revision: project.revision.clone(),
        collection_id: Some(collection.data.id.clone()),
        collection_revision: Some(collection.revision.clone()),
    });
    let client = client();
    let cancel = CancellationToken::new();
    let prepare = |input: &ExecutionInput| {
        prepare::prepare(
            input,
            Some(root.path()),
            &project.data.id,
            settings.path(),
            &client,
            &values,
            &cancel,
        )
    };
    let result = transfer::execute(
        &client,
        prepare(&input).unwrap(),
        Duration::from_secs(2),
        &cancel,
        None,
    )
    .await;
    assert_eq!(result.status, Some(200));
    let received = String::from_utf8(received.await.unwrap())
        .unwrap()
        .to_ascii_lowercase();
    assert!(received.contains("authorization: basic c3ludghldgljlxvzzxi6c3ludghldgljlxbhc3n3b3jk"));
    task.await.unwrap();
    // An explicit None on the request stops inherited folder/project authentication.
    input.request.auth = Some(Auth::None);
    assert!(!prepare(&input)
        .unwrap()
        .request
        .headers()
        .contains_key("authorization"));
    // Manual Authorization is case-insensitive and does not require the overridden token.
    input.request.auth = Some(Auth::Bearer {
        token: Uuid::new_v4().to_string(),
    });
    input.request.headers.push(Entry {
        id: 1,
        enabled: true,
        key: "aUtHoRiZaTiOn".into(),
        value: "manual-fixture".into(),
    });
    assert_eq!(
        prepare(&input).unwrap().request.headers()["authorization"],
        "manual-fixture"
    );
    input.request.headers.clear();
    assert!(prepare(&input)
        .err()
        .unwrap()
        .contains("variable is missing"));
    input.request.auth = Some(Auth::ApiKey {
        name: "access_key".into(),
        value: token.clone(),
        placement: KeyPlacement::Query,
    });
    let prepared = prepare(&input).unwrap();
    assert_eq!(
        prepared
            .request
            .url()
            .query_pairs()
            .find(|(key, _)| key == "access_key")
            .unwrap()
            .1,
        "synthetic-key /+"
    );
    // A literal URL query value wins, including an explicitly empty value.
    input.request.url = format!("{url}?access_key=");
    input.request.auth = Some(Auth::ApiKey {
        name: "access_key".into(),
        value: Uuid::new_v4().to_string(),
        placement: KeyPlacement::Query,
    });
    assert_eq!(
        prepare(&input)
            .unwrap()
            .request
            .url()
            .query_pairs()
            .filter(|(key, _)| key == "access_key")
            .count(),
        1
    );
    input.request.auth = Some(Auth::ApiKey {
        name: "X-API-Key".into(),
        value: token,
        placement: KeyPlacement::Header,
    });
    assert_eq!(
        prepare(&input).unwrap().request.headers()["x-api-key"],
        "synthetic-key /+"
    );
    // Stale ancestry blocks even explicit request auth: the UI must reload its context.
    let mut changed = collection.data.clone();
    changed
        .folders
        .iter_mut()
        .find(|f| f.id == child)
        .unwrap()
        .auth = Some(Auth::None);
    let updated = store
        .save_collection(&project.handle, changed, Some(collection.revision))
        .unwrap();
    assert!(prepare(&input)
        .err()
        .unwrap()
        .contains("Collection authentication changed"));
    input.scope.as_mut().unwrap().collection_revision = Some(updated.revision);
    input.request.auth = None;
    assert!(!prepare(&input)
        .unwrap()
        .request
        .headers()
        .contains_key("authorization"));
    store
        .save_auth(&project.handle, Some(Auth::None), &project.revision)
        .unwrap();
    assert!(prepare(&input)
        .err()
        .unwrap()
        .contains("Project authentication changed"));
}

#[test]
fn auth_rejects_invalid_credentials_without_leaking_values_or_reading_overrides() {
    use super::auth;
    use std::cell::Cell;
    let mut url = reqwest::Url::parse("https://example.test/?key=manual").unwrap();
    let mut headers = reqwest::header::HeaderMap::new();
    let client = client();
    let reads = Cell::new(0);
    let denied = |_: &str| {
        reads.set(reads.get() + 1);
        Err("Credential store access was denied.".into())
    };
    headers.insert(
        "authorization",
        reqwest::header::HeaderValue::from_static("manual"),
    );
    auth::apply(
        Some(&Auth::Basic {
            username: "user".into(),
            password: "password".into(),
        }),
        &mut url,
        &mut headers,
        &client,
        denied,
    )
    .unwrap();
    auth::apply(
        Some(&Auth::ApiKey {
            name: "key".into(),
            value: "key".into(),
            placement: KeyPlacement::Query,
        }),
        &mut url,
        &mut headers,
        &client,
        denied,
    )
    .unwrap();
    assert_eq!(reads.get(), 0);
    headers.clear();
    assert!(auth::apply(
        Some(&Auth::Bearer {
            token: "token".into()
        }),
        &mut url,
        &mut headers,
        &client,
        denied
    )
    .unwrap_err()
    .contains("denied"));
    let error = auth::apply(
        Some(&Auth::Basic {
            username: "user".into(),
            password: "password".into(),
        }),
        &mut url,
        &mut headers,
        &client,
        |_| Ok("private:fixture".into()),
    )
    .unwrap_err();
    assert!(!error.contains("private"));
    let error = auth::apply(
        Some(&Auth::Bearer {
            token: "token".into(),
        }),
        &mut url,
        &mut headers,
        &client,
        |_| Ok("private\r\nInjected: value".into()),
    )
    .unwrap_err();
    assert!(!error.contains("private"));
    assert!(serde_json::from_value::<Auth>(
        serde_json::json!({"kind":"bearer","token":"fixture","secret":"plaintext"})
    )
    .is_err());
    assert!(validate_auth(Some(&Auth::Bearer {
        token: "literal-token-is-not-an-id".into()
    }))
    .is_err());
    assert!(validate_auth(Some(&Auth::ApiKey {
        name: "Host".into(),
        value: Uuid::new_v4().to_string(),
        placement: KeyPlacement::Header
    }))
    .is_err());
}
