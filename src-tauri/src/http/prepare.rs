use super::model::{ExecutionInput, Scope};
use crate::projects::{
    environments::{self, Definitions, Values},
    model::{Attachment, BodyKind, Request},
    store,
};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, CONTENT_LENGTH, CONTENT_TYPE},
    multipart, Body, Client, Method, Url,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    fs::File,
    path::Path,
};
use tokio_util::{io::ReaderStream, sync::CancellationToken};

/// Only a worker creates this snapshot; no project lock is held during network I/O.
pub struct Prepared {
    pub request: reqwest::Request,
    pub token: Option<super::tokens::Plan>,
    pub display_headers: Vec<super::model::RequestHeader>,
    pub header_names: Vec<String>,
}
impl Prepared {
    pub fn header_summary(&self) -> Vec<super::model::RequestHeader> {
        let mut rows = self.display_headers.clone();
        for (name, _) in self.request.headers() {
            if !self.header_names.iter().any(|key| key == name.as_str()) {
                rows.push(super::model::RequestHeader {
                    name: name.to_string(),
                    value: "••••••••".into(),
                    source: "authentication",
                });
            }
        }
        rows
    }
}
struct CachedValues<'a, V> {
    source: &'a V,
    values: RefCell<HashMap<String, Option<String>>>,
    cancel: &'a CancellationToken,
}
impl<V: Values> Values for CachedValues<'_, V> {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        if self.cancel.is_cancelled() {
            return Err("Request cancelled before dispatch.".into());
        }
        if let Some(value) = self.values.borrow().get(key) {
            return Ok(value.clone());
        }
        let value = self.source.get(key)?;
        self.values.borrow_mut().insert(key.into(), value.clone());
        Ok(value)
    }
    fn set(&self, _: &str, _: &str) -> Result<(), String> {
        Err("Execution cannot modify variable values.".into())
    }
    fn delete(&self, _: &str) -> Result<(), String> {
        Err("Execution cannot delete variable values.".into())
    }
}
fn attachment(root: Option<&Path>, reference: &Attachment) -> Result<(File, u64), String> {
    reference.validate()?;
    let root = root.ok_or("Open the attachment's project before sending.")?;
    store::safe(&root.join("attachments"), true)?;
    let path = root.join(&reference.path);
    store::safe(&path, false)?;
    let file =
        File::open(path).map_err(|_| "Could not open an attached file. Check project access.")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Could not inspect an attached file.")?;
    if !metadata.is_file() {
        return Err("Attachments must be regular files.".into());
    }
    Ok((file, metadata.len()))
}
fn file_body(file: File) -> Body {
    Body::wrap_stream(ReaderStream::new(tokio::fs::File::from_std(file)))
}
fn defaults(headers: &mut HeaderMap, mime: &'static str) {
    headers
        .entry(CONTENT_TYPE)
        .or_insert(HeaderValue::from_static(mime));
}

pub fn prepare(
    input: &ExecutionInput,
    root: Option<&Path>,
    project: &str,
    settings: &Path,
    client: &Client,
    values: &impl Values,
    cancel: &CancellationToken,
) -> Result<Prepared, String> {
    input.request.validate()?;
    let auth = super::auth::effective(input, root)?;
    if !(1_000..=300_000).contains(&input.timeout_ms) {
        return Err("Timeout must be between 1 and 300 seconds.".into());
    }
    if serde_json::to_vec(&input.request)
        .map_err(|_| "Could not validate request data.")?
        .len()
        > 8 * 1024 * 1024
    {
        return Err("Request definitions must not exceed 8 MiB.".into());
    }
    let view = if let Some(root) = root {
        Some(environments::load(root, settings, project)?)
    } else {
        None
    };
    if let (
        Some(view),
        Some(Scope {
            environment,
            revision,
            ..
        }),
    ) = (&view, &input.scope)
    {
        if view.selection_missing
            || &view.selected_environment != environment
            || &view.revision != revision
        {
            return Err(
                "Project variables or environment changed. Reload variables before sending.".into(),
            );
        }
    }
    let definitions = view
        .as_ref()
        .map(|v| v.data.clone())
        .unwrap_or_else(Definitions::default);
    let environment = view
        .as_ref()
        .and_then(|v| v.selected_environment.as_deref());
    let cache = CachedValues {
        source: values,
        values: RefCell::new(HashMap::new()),
        cancel,
    };
    let resolved_bytes = Cell::new(0usize);
    let resolve = |text: &str| {
        let value = environments::resolve(text, project, &definitions, environment, &cache)?;
        let total = resolved_bytes.get().saturating_add(value.len());
        if total > 8 * 1024 * 1024 {
            return Err("Resolved request text exceeds 8 MiB.".into());
        }
        resolved_bytes.set(total);
        Ok::<String, String>(value)
    };
    let request: &Request = &input.request;
    let method =
        Method::from_bytes(request.method.as_bytes()).map_err(|_| "Invalid HTTP method.")?;
    if method == Method::CONNECT {
        return Err("CONNECT tunnelling is not supported.".into());
    }
    let resolved_url = resolve(&request.url)?;
    let mut url = Url::parse(&resolved_url).map_err(|_| "Enter a complete HTTP or HTTPS URL.")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Only HTTP and HTTPS URLs are supported.".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(
            "Use an Authorization header instead of credentials embedded in the URL.".into(),
        );
    }
    url.set_fragment(None);
    for entry in request
        .params
        .iter()
        .filter(|e| e.enabled && !e.key.is_empty())
    {
        url.query_pairs_mut()
            .append_pair(&resolve(&entry.key)?, &resolve(&entry.value)?);
    }
    if url.as_str().len() > 65_536 {
        return Err("Resolved URL and query parameters exceed 64 KiB.".into());
    }
    let mut headers = HeaderMap::new();
    let mut header_bytes = 0usize;
    let mut manual_names = HashMap::new();
    for entry in request
        .headers
        .iter()
        .filter(|e| e.enabled && !e.key.is_empty())
    {
        let name = HeaderName::from_bytes(resolve(&entry.key)?.as_bytes())
            .map_err(|_| "A header name is invalid.")?;
        if matches!(
            name.as_str(),
            "content-length"
                | "transfer-encoding"
                | "connection"
                | "upgrade"
                | "proxy-authorization"
                | "proxy-connection"
                | "te"
                | "trailer"
        ) {
            return Err("Fluxkit manages framing and proxy headers. Remove Content-Length, Transfer-Encoding, Connection, Upgrade, TE, Trailer and proxy headers.".into());
        }
        let value = HeaderValue::from_bytes(resolve(&entry.value)?.as_bytes())
            .map_err(|_| "A header value contains invalid characters.")?;
        header_bytes += name.as_str().len() + value.as_bytes().len();
        if header_bytes > 64 * 1024 || headers.len() >= 1000 {
            return Err("Request headers exceed 64 KiB or 1,000 entries.".into());
        }
        manual_names.insert(
            name.to_string(),
            if entry.key.contains("{{") {
                "[resolved header name]".to_string()
            } else {
                name.to_string()
            },
        );
        headers.append(name, value);
    }

    super::auth::apply(auth.auth.as_ref(), &mut url, &mut headers, client, |id| {
        let variable = definitions
            .variables
            .iter()
            .find(|v| v.id == id)
            .ok_or("An authentication variable is missing. Update authentication settings.")?;
        resolve(&format!("{{{{{}}}}}", variable.name))
    })?;
    let token = super::tokens::Plan::new(
        auth.auth.as_ref(),
        &auth.owner,
        project,
        environment,
        &definitions,
    )?
    .map(|plan| plan.scoped(root))
    .transpose()?
    .filter(|plan| !plan.overridden(&url, &headers));
    if url.as_str().len() > 65_536
        || headers
            .iter()
            .map(|(k, v)| k.as_str().len() + v.as_bytes().len())
            .sum::<usize>()
            > 64 * 1024
        || headers.len() > 1000
    {
        return Err("Authentication exceeds the URL or header size limits.".into());
    }
    // Anything present before defaults came from user data or authentication and stays masked.
    let private_names: std::collections::HashSet<_> =
        headers.keys().map(|key| key.to_string()).collect();
    let pending_header = token.as_ref().and_then(super::tokens::Plan::pending_header);
    for (name, value) in [
        (
            reqwest::header::USER_AGENT,
            concat!("Fluxkit/", env!("CARGO_PKG_VERSION")),
        ),
        (reqwest::header::ACCEPT, "*/*"),
    ] {
        // Defaults must not override a token destined for one of these header names.
        if !pending_header
            .as_ref()
            .is_some_and(|header| header.name.eq_ignore_ascii_case(name.as_str()))
        {
            headers
                .entry(name)
                .or_insert(HeaderValue::from_static(value));
        }
    }
    let mut builder = client.request(method, url);
    match request.body.kind {
        BodyKind::None => {}
        BodyKind::Json | BodyKind::Text => {
            defaults(
                &mut headers,
                if matches!(request.body.kind, BodyKind::Json) {
                    "application/json"
                } else {
                    "text/plain; charset=utf-8"
                },
            );
            builder = builder.body(resolve(&request.body.text)?);
        }
        BodyKind::UrlEncoded => {
            let fields: Vec<_> = request
                .body
                .fields
                .iter()
                .filter(|f| f.enabled && !f.key.is_empty())
                .map(|f| Ok((resolve(&f.key)?, resolve(&f.value)?)))
                .collect::<Result<_, String>>()?;
            builder = builder.form(&fields);
        }
        BodyKind::Multipart => {
            if headers.contains_key(CONTENT_TYPE) {
                return Err("Remove the manual Content-Type header for multipart: Fluxkit generates the boundary.".into());
            }
            let mut form = multipart::Form::new();
            for field in request
                .body
                .fields
                .iter()
                .filter(|f| f.enabled && !f.key.is_empty())
            {
                let name = resolve(&field.key)?;
                form = if let Some(reference) = &field.file {
                    let (file, size) = attachment(root, reference)?;
                    form.part(
                        name,
                        multipart::Part::stream_with_length(file_body(file), size)
                            .file_name(reference.name.clone()),
                    )
                } else {
                    form.text(name, resolve(&field.value)?)
                };
            }
            builder = builder.multipart(form);
        }
        BodyKind::File => {
            let reference = request
                .body
                .file
                .as_ref()
                .ok_or("Choose a body file before sending.")?;
            let (file, size) = attachment(root, reference)?;
            defaults(&mut headers, "application/octet-stream");
            headers.insert(CONTENT_LENGTH, HeaderValue::from(size));
            builder = builder.body(file_body(file));
        }
    }
    if cancel.is_cancelled() {
        return Err("Request cancelled before dispatch.".into());
    }
    let request = builder
        .headers(headers)
        .build()
        .map_err(|_| "Could not build the HTTP request. Check headers and body configuration.")?;
    if request.headers().len() > 1000
        || request
            .headers()
            .iter()
            .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
            .sum::<usize>()
            > 64 * 1024
    {
        return Err("Request headers including defaults exceed 64 KiB or 1,000 entries.".into());
    }
    let display_headers = request
        .headers()
        .iter()
        .map(|(name, value)| super::model::RequestHeader {
            name: manual_names
                .get(name.as_str())
                .cloned()
                .unwrap_or_else(|| name.to_string()),
            value: if private_names.contains(name.as_str()) {
                "••••••••".into()
            } else {
                value.to_str().unwrap_or("[binary]").to_string()
            },
            source: if manual_names.contains_key(name.as_str()) {
                "manual"
            } else if private_names.contains(name.as_str()) {
                "authentication"
            } else {
                "automatic"
            },
        })
        .collect();
    let header_names = request
        .headers()
        .keys()
        .map(|name| name.to_string())
        .collect();
    Ok(Prepared {
        request,
        token,
        display_headers,
        header_names,
    })
}

/// Metadata/renewal commands validate the same authoritative ancestry without needing a valid API URL.
pub fn token_plan(
    input: &ExecutionInput,
    root: &Path,
    project: &str,
    settings: &Path,
) -> Result<super::tokens::Plan, String> {
    input.request.validate()?;
    if !(1_000..=300_000).contains(&input.timeout_ms)
        || serde_json::to_vec(&input.request)
            .map_err(|_| "Invalid request data.")?
            .len()
            > 8 * 1024 * 1024
    {
        return Err("Invalid token operation timeout or request size.".into());
    }
    let view = environments::load(root, settings, project)?;
    let scope = input
        .scope
        .as_ref()
        .ok_or("Token acquisition requires a project.")?;
    if view.selection_missing
        || view.selected_environment != scope.environment
        || view.revision != scope.revision
    {
        return Err(
            "Project variables or environment changed. Reload variables before continuing.".into(),
        );
    }
    let auth = super::auth::effective(input, Some(root))?;
    super::tokens::Plan::new(
        auth.auth.as_ref(),
        &auth.owner,
        project,
        view.selected_environment.as_deref(),
        &view.data,
    )?
    .ok_or_else(|| "Choose OAuth2 Client Credentials or Custom Login first.".to_string())?
    .scoped(Some(root))
}
