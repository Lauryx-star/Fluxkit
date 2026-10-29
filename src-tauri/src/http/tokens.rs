//! Bounded, non-recursive token acquisition. Secrets only enter the OS-store adapter.
use crate::projects::{
    environments::{self, Definitions, Values},
    model::{validate_auth, validate_id, Auth, KeyPlacement},
    token_config::{
        ClientAuthentication, Composition, Extraction, LoginBody, Part, TokenPlacement,
    },
};
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Client, Method, Url,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio_util::sync::CancellationToken;

const STORE_ERROR: &str = "The OS token store could not be accessed. Unlock the credential store and retry. No plaintext fallback is used.";
const INVALID_RECORD: &str =
    "Stored token data is invalid. Forget this local token and acquire it again.";
const RESPONSE_LIMIT: usize = 1024 * 1024;
const RECORD_LIMIT: usize = 2560;
pub const CANCELLED: &str = "Token acquisition cancelled. No API request was sent. An OS write already in progress may still finish.";
pub const TIMED_OUT: &str = "Token acquisition timed out. No API request was sent.";

#[derive(Clone)]
pub struct Plan {
    key: String,
    config: Auth,
    project: String,
    environment: Option<String>,
    definitions: Definitions,
    placement: TokenPlacement,
}
impl Plan {
    pub fn scoped(mut self, root: Option<&std::path::Path>) -> Result<Self, String> {
        if let Some(identity) = root
            .map(crate::projects::store::server_owner)
            .transpose()?
            .flatten()
        {
            self.key = format!("server/{}/{}", identity.namespace(), self.key);
        }
        Ok(self)
    }

    pub fn new(
        auth: Option<&Auth>,
        owner: &str,
        project: &str,
        environment: Option<&str>,
        definitions: &Definitions,
    ) -> Result<Option<Self>, String> {
        let placement = match auth {
            Some(Auth::Oauth2 { .. }) => TokenPlacement {
                name: "Authorization".into(),
                placement: KeyPlacement::Header,
                prefix: "Bearer ".into(),
            },
            Some(Auth::CustomLogin { config }) => config.placement.clone(),
            _ => return Ok(None),
        };
        validate_id(project)
            .map_err(|_| "Token acquisition requires an open project.".to_string())?;
        validate_auth(auth)?;
        let config = auth.ok_or("Choose a token authentication method.")?.clone();
        let bytes =
            serde_json::to_vec(&config).map_err(|_| "Could not identify token configuration.")?;
        if bytes.len() > 256 * 1024 {
            return Err("Token configuration exceeds 256 KiB.".into());
        }
        let digest = format!("{:x}", Sha256::digest(bytes));
        Ok(Some(Self {
            key: format!(
                "tokens/v1/{project}/{owner}/{}/{digest}",
                environment.unwrap_or("default")
            ),
            config,
            project: project.into(),
            environment: environment.map(String::from),
            definitions: definitions.clone(),
            placement,
        }))
    }
    pub fn pending_header(&self) -> Option<super::model::RequestHeader> {
        (self.placement.placement == KeyPlacement::Header).then(|| super::model::RequestHeader {
            name: self.placement.name.clone(),
            value: "•••••••• (token acquired on send)".into(),
            source: "authentication",
        })
    }
    pub fn overridden(&self, url: &Url, headers: &HeaderMap) -> bool {
        match self.placement.placement {
            KeyPlacement::Header => headers.contains_key(&self.placement.name),
            KeyPlacement::Query => url.query_pairs().any(|(key, _)| key == self.placement.name),
        }
    }
    fn placed_value(&self, token: &str) -> Result<String, String> {
        if token.is_empty() || token.chars().any(char::is_control) {
            return Err("Token extraction must produce a non-empty value without control characters. Existing token was kept.".into());
        }
        if matches!(self.config, Auth::Oauth2 { .. }) && token.chars().any(char::is_whitespace) {
            return Err("OAuth2 returned an invalid Bearer token. Existing token was kept.".into());
        }
        let value = format!("{}{token}", self.placement.prefix);
        if self.placement.placement == KeyPlacement::Header {
            HeaderValue::from_bytes(value.as_bytes()).map_err(|_| {
                "Extracted token cannot be used in the configured header. Existing token was kept."
            })?;
        }
        Ok(value)
    }
    pub fn apply(&self, request: &mut reqwest::Request, record: &Record) -> Result<(), String> {
        if record.status(now()?).state == "expired" {
            return Err(
                "Token expired before API dispatch. Acquire a token again explicitly.".into(),
            );
        }
        if !self.overridden(request.url(), request.headers()) {
            let value = self.placed_value(&record.token)?;
            match self.placement.placement {
                KeyPlacement::Header => {
                    let name = HeaderName::from_bytes(self.placement.name.as_bytes())
                        .map_err(|_| "Invalid token header name.")?;
                    let mut value = HeaderValue::from_bytes(value.as_bytes())
                        .map_err(|_| "Invalid token header value.")?;
                    value.set_sensitive(true);
                    request.headers_mut().insert(name, value);
                }
                KeyPlacement::Query => {
                    request
                        .url_mut()
                        .query_pairs_mut()
                        .append_pair(&self.placement.name, &value);
                }
            }
        }
        if request.url().as_str().len() > 65_536
            || request.headers().len() > 1000
            || request
                .headers()
                .iter()
                .map(|(n, v)| n.as_str().len() + v.as_bytes().len())
                .sum::<usize>()
                > 65_536
        {
            return Err("Authentication exceeds the URL or header size limits.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    schema_version: u32,
    token: String,
    renewal_token: Option<String>,
    acquired_at: u64,
    expires_at: Option<u64>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: &'static str,
    pub expires_at: Option<u64>,
}
impl Record {
    pub fn status_now(&self) -> Result<Status, String> {
        Ok(self.status(now()?))
    }
    fn status(&self, now: u64) -> Status {
        let state = if self.acquired_at > now || self.expires_at.is_some_and(|end| end <= now) {
            "expired"
        } else if let Some(end) = self.expires_at {
            let margin = ((end - self.acquired_at) / 10).min(30);
            if end.saturating_sub(margin) <= now {
                "renewalDue"
            } else {
                "valid"
            }
        } else {
            "unknownExpiry"
        };
        Status {
            state,
            expires_at: self.expires_at,
        }
    }
    fn reusable(&self, now: u64) -> bool {
        matches!(self.status(now).state, "valid" | "unknownExpiry")
    }
}
fn now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| "System clock is invalid. Check the clock before acquiring tokens.".into())
}
async fn worker<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| "Token preparation could not finish. Retry explicitly.".to_string())?
}
async fn read<V: Values + Send + Sync + 'static>(
    plan: &Plan,
    values: Arc<V>,
) -> Result<Option<Record>, String> {
    let key = plan.key.clone();
    let record = worker(move || {
        let value = values.get(&key).map_err(|_| STORE_ERROR)?;
        value
            .map(|v| {
                if v.len() > RECORD_LIMIT {
                    return Err(INVALID_RECORD.into());
                }
                let record: Record = serde_json::from_str(&v).map_err(|_| INVALID_RECORD)?;
                if record.schema_version != 1
                    || record.acquired_at > 253_402_300_799
                    || record.expires_at.is_some_and(|v| {
                        v <= record.acquired_at
                            || v > 253_402_300_799
                            || v - record.acquired_at > 315_360_000
                    })
                {
                    return Err(INVALID_RECORD.into());
                }
                Ok(record)
            })
            .transpose()
    })
    .await?;
    if let Some(record) = &record {
        plan.placed_value(&record.token)?;
    }
    Ok(record)
}

#[derive(Default)]
struct Flight {
    result: tokio::sync::Mutex<Option<Result<Record, String>>>,
    generation: AtomicU64,
}
#[derive(Default)]
pub struct Manager {
    flights: Mutex<HashMap<String, Weak<Flight>>>,
}
impl Manager {
    fn flight(&self, key: &str) -> Result<Arc<Flight>, String> {
        let mut flights = self
            .flights
            .lock()
            .map_err(|_| "Token state is unavailable.")?;
        flights.retain(|_, v| v.strong_count() > 0);
        if let Some(flight) = flights.get(key).and_then(Weak::upgrade) {
            return Ok(flight);
        }
        let flight = Arc::new(Flight::default());
        flights.insert(key.into(), Arc::downgrade(&flight));
        Ok(flight)
    }
    pub async fn status<V: Values + Send + Sync + 'static>(
        &self,
        plan: &Plan,
        values: Arc<V>,
    ) -> Result<Status, String> {
        let flight = self.flight(&plan.key)?;
        let _guard = flight.result.lock().await;
        let time = now()?;
        Ok(read(plan, values)
            .await?
            .map(|r| r.status(time))
            .unwrap_or(Status {
                state: "missing",
                expires_at: None,
            }))
    }
    pub async fn forget<V: Values + Send + Sync + 'static>(
        &self,
        plan: &Plan,
        values: Arc<V>,
    ) -> Result<Status, String> {
        let flight = self.flight(&plan.key)?;
        let mut guard = flight.result.lock().await;
        let key = plan.key.clone();
        worker(move || values.delete(&key).map_err(|_| STORE_ERROR.into())).await?;
        *guard = Some(Err(
            "Local token was forgotten. Acquire it again explicitly.".into(),
        ));
        flight.generation.fetch_add(1, Ordering::SeqCst);
        Ok(Status {
            state: "missing",
            expires_at: None,
        })
    }
    pub async fn ensure<V: Values + Send + Sync + 'static>(
        &self,
        plan: &Plan,
        client: &Client,
        values: Arc<V>,
        force: bool,
        cancel: &CancellationToken,
        timeout: Duration,
    ) -> Result<Record, String> {
        let deadline = tokio::time::Instant::now() + timeout;
        let flight = self.flight(&plan.key)?;
        let generation = flight.generation.load(Ordering::SeqCst);
        let mut guard = tokio::select! { biased;
            _ = cancel.cancelled() => return Err(CANCELLED.into()),
            _ = tokio::time::sleep_until(deadline) => return Err(TIMED_OUT.into()),
            guard = flight.result.lock() => guard,
        };
        if flight.generation.load(Ordering::SeqCst) != generation {
            if let Some(result) = &*guard {
                return result.clone();
            }
        }
        let acquire = async {
            let old = read(plan, values.clone()).await?;
            if !force {
                if let Some(record) = &old {
                    if record.reusable(now()?) {
                        return Ok(record.clone());
                    }
                }
            }
            acquire(plan.clone(), client.clone(), values, old, cancel.clone()).await
        };
        let result = tokio::select! { biased;
            _ = cancel.cancelled() => Err(CANCELLED.into()),
            _ = tokio::time::sleep_until(deadline) => Err(TIMED_OUT.into()),
            result = acquire => result,
        };
        *guard = Some(result.clone());
        flight.generation.fetch_add(1, Ordering::SeqCst);
        result
    }
}

struct Inputs<'a, V> {
    plan: &'a Plan,
    values: &'a V,
    old: Option<&'a Record>,
    bytes: usize,
    cache: HashMap<String, String>,
    cancel: &'a CancellationToken,
}
impl<V: Values> Inputs<'_, V> {
    fn variable(&mut self, id: &str) -> Result<String, String> {
        if self.cancel.is_cancelled() {
            return Err("Token preparation cancelled.".into());
        }
        if let Some(value) = self.cache.get(id) {
            return Ok(value.clone());
        }
        let variable = self
            .plan
            .definitions
            .variables
            .iter()
            .find(|v| v.id == id)
            .ok_or("Token login uses a deleted variable. Choose another variable in the authentication settings.")?;
        let value = environments::resolve(
            &format!("{{{{{}}}}}", variable.name),
            &self.plan.project,
            &self.plan.definitions,
            self.plan.environment.as_deref(),
            self.values,
        )?;
        self.cache.insert(id.into(), value.clone());
        Ok(value)
    }
    fn compose(&mut self, expression: &Composition) -> Result<String, String> {
        let parts = expression
            .parts
            .iter()
            .map(|part| match part {
                Part::Literal { text } => Ok(text.clone()),
                Part::Variable { id } => self.variable(id),
                Part::AccessToken => self
                    .old
                    .map(|v| v.token.clone())
                    .ok_or("No stored access token is available for renewal.".into()),
                Part::RenewalToken => self.old.and_then(|v| v.renewal_token.clone()).ok_or(
                    "No stored renewal value is available. Forget the local token to log in again."
                        .into(),
                ),
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut result = parts.join(&expression.separator);
        if expression.base64 {
            result = STANDARD.encode(result.as_bytes());
        }
        let result = format!("{}{result}{}", expression.prefix, expression.suffix);
        self.bytes = self.bytes.saturating_add(result.len());
        if self.bytes > RESPONSE_LIMIT {
            return Err("Resolved token inputs exceed 1 MiB.".into());
        }
        Ok(result)
    }
}
fn endpoint(value: &str) -> Result<Url, String> {
    let mut url = Url::parse(value).map_err(|_| "Enter a complete token endpoint URL.")?;
    let loopback = url
        .host_str()
        .and_then(|host| {
            host.trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .ok()
        })
        .is_some_and(|ip| ip.is_loopback());
    if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Token endpoints require HTTPS without embedded credentials; HTTP is allowed only for numeric loopback addresses.".into());
    }
    url.set_fragment(None);
    Ok(url)
}
fn form_component(value: &str) -> Result<String, String> {
    // RFC 6749 §2.3.1 applies form encoding to both Basic credentials before Base64.
    let url = reqwest::Url::parse_with_params("https://example.invalid", [("v", value)])
        .map_err(|_| "Could not encode OAuth2 client authentication.")?;
    Ok(url
        .query()
        .ok_or("Could not encode OAuth2 client authentication.")?
        .trim_start_matches("v=")
        .to_owned())
}
fn build<V: Values>(
    plan: &Plan,
    client: &Client,
    values: &V,
    old: Option<&Record>,
    cancel: &CancellationToken,
) -> Result<reqwest::Request, String> {
    let mut inputs = Inputs {
        plan,
        values,
        old,
        bytes: 0,
        cache: HashMap::new(),
        cancel,
    };
    let mut request = match &plan.config {
        Auth::Oauth2 { config } => {
            let url = endpoint(&inputs.compose(&config.token_url)?)?;
            let id = inputs.variable(&config.client_id)?;
            let secret = inputs.variable(&config.client_secret)?;
            if id.is_empty() || secret.is_empty() {
                return Err("OAuth2 client credentials must not be empty.".into());
            }
            let mut fields = vec![("grant_type", "client_credentials".to_owned())];
            let scope = inputs.compose(&config.scope)?;
            if !scope.is_empty() {
                fields.push(("scope", scope));
            }
            let mut builder = client.post(url);
            match config.client_authentication {
                ClientAuthentication::Basic => {
                    builder =
                        builder.basic_auth(form_component(&id)?, Some(form_component(&secret)?))
                }
                ClientAuthentication::Body => {
                    fields.push(("client_id", id));
                    fields.push(("client_secret", secret));
                }
            }
            builder.form(&fields).build()
        }
        Auth::CustomLogin { config } => {
            let definition = if old.is_some() {
                config.renewal.as_ref().unwrap_or(&config.login)
            } else {
                &config.login
            };
            let mut url = endpoint(&inputs.compose(&definition.url)?)?;
            for entry in &definition.params {
                url.query_pairs_mut()
                    .append_pair(&entry.name, &inputs.compose(&entry.value)?);
            }
            let method = Method::from_bytes(definition.method.as_bytes())
                .map_err(|_| "Invalid login HTTP method.")?;
            let mut headers = HeaderMap::new();
            for entry in &definition.headers {
                let name = HeaderName::from_bytes(entry.name.as_bytes())
                    .map_err(|_| "Invalid login header name.")?;
                if matches!(
                    name.as_str(),
                    "host"
                        | "content-length"
                        | "transfer-encoding"
                        | "connection"
                        | "upgrade"
                        | "proxy-authorization"
                        | "proxy-connection"
                        | "te"
                        | "trailer"
                ) {
                    return Err("Login requests cannot override transport-managed headers.".into());
                }
                let mut value = HeaderValue::from_bytes(inputs.compose(&entry.value)?.as_bytes())
                    .map_err(|_| "Invalid login header value.")?;
                value.set_sensitive(true);
                headers.append(name, value);
            }
            let mut builder = client.request(method, url);
            match &definition.body {
                LoginBody::None => {}
                LoginBody::Text { value } | LoginBody::Json { value } => {
                    headers.entry(reqwest::header::CONTENT_TYPE).or_insert(
                        HeaderValue::from_static(
                            if matches!(definition.body, LoginBody::Json { .. }) {
                                "application/json"
                            } else {
                                "text/plain; charset=utf-8"
                            },
                        ),
                    );
                    builder = builder.body(inputs.compose(value)?);
                }
                LoginBody::UrlEncoded { fields } => {
                    let fields = fields
                        .iter()
                        .map(|f| Ok((f.name.clone(), inputs.compose(&f.value)?)))
                        .collect::<Result<Vec<_>, String>>()?;
                    builder = builder.form(&fields);
                }
            }
            builder.headers(headers).build()
        }
        _ => return Err("Choose a token authentication method.".into()),
    }
    .map_err(|_| "Could not build the token request. Check its fields.")?;
    request
        .headers_mut()
        .entry(reqwest::header::USER_AGENT)
        .or_insert(HeaderValue::from_static(concat!(
            "Fluxkit/",
            env!("CARGO_PKG_VERSION")
        )));
    request
        .headers_mut()
        .entry(reqwest::header::ACCEPT)
        .or_insert(HeaderValue::from_static("*/*"));
    if request.url().as_str().len() > 65_536
        || request.headers().len() > 1000
        || request
            .headers()
            .iter()
            .map(|(n, v)| n.as_str().len() + v.as_bytes().len())
            .sum::<usize>()
            > 65_536
    {
        return Err("Token request URL or headers exceed their size limits.".into());
    }
    Ok(request)
}
fn extract(
    extraction: &Extraction,
    body: &[u8],
    headers: &HeaderMap,
    numeric: bool,
) -> Result<String, String> {
    let result = match extraction {
        Extraction::Text => std::str::from_utf8(body).ok().map(|v| v.trim().to_owned()),
        Extraction::Header { name } => {
            let mut values = headers.get_all(name).iter();
            let first = values
                .next()
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            if values.next().is_some() {
                None
            } else {
                first
            }
        }
        Extraction::Json { pointer } => {
            let json: serde_json::Value = serde_json::from_slice(body)
                .map_err(|_| "Token response is not valid JSON. Existing token was kept.")?;
            json.pointer(pointer).and_then(|v| {
                v.as_str().map(str::to_owned).or_else(|| {
                    if numeric {
                        v.as_u64().map(|v| v.to_string())
                    } else {
                        None
                    }
                })
            })
        }
    };
    result
        .filter(|v| !v.is_empty())
        .ok_or("Required token response extraction failed. Existing token was kept.".into())
}
fn lifetime(value: &str) -> Result<u64, String> {
    value.parse::<u64>().ok().filter(|v| *v > 0 && *v <= 315_360_000).ok_or("Extracted lifetime must be a positive integer number of seconds, at most ten years. Existing token was kept.".into())
}
/// Verify a fresh login without reading, replacing or retaining a stored token.
pub async fn test<V: Values + Send + Sync + 'static>(
    plan: Plan,
    client: Client,
    values: Arc<V>,
    cancel: CancellationToken,
    timeout: Duration,
) -> Result<Status, String> {
    struct ReadOnly<V>(Arc<V>);
    impl<V: Values> Values for ReadOnly<V> {
        fn get(&self, key: &str) -> Result<Option<String>, String> {
            self.0.get(key)
        }
        fn set(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<(), String> {
            Err("Authentication tests cannot delete values.".into())
        }
    }
    tokio::select! { biased;
        _ = cancel.cancelled() => Err("Authentication test cancelled.".into()),
        _ = tokio::time::sleep(timeout) => Err("Authentication test timed out.".into()),
        record = acquire(plan, client, Arc::new(ReadOnly(values)), None, cancel.clone()) => record?.status_now(),
    }
}

async fn acquire<V: Values + Send + Sync + 'static>(
    plan: Plan,
    client: Client,
    values: Arc<V>,
    old: Option<Record>,
    cancel: CancellationToken,
) -> Result<Record, String> {
    let snapshot = plan.clone();
    let source = values.clone();
    let previous = old.clone();
    let transport = client.clone();
    let token = cancel.clone();
    let request = worker(move || {
        build(
            &snapshot,
            &transport,
            source.as_ref(),
            previous.as_ref(),
            &token,
        )
    })
    .await?;
    let acquired_at = now()?;
    let mut response = client.execute(request).await.map_err(|_| {
        "Token endpoint could not be reached or TLS verification failed. Existing token was kept."
    })?;
    let status = response.status().as_u16();
    let definition = match &plan.config {
        Auth::CustomLogin { config } => Some(if old.is_some() {
            config.renewal.as_ref().unwrap_or(&config.login)
        } else {
            &config.login
        }),
        _ => None,
    };
    let accepted = definition
        .map(|d| d.success_statuses.contains(&status))
        .unwrap_or(status == 200);
    if !accepted {
        return Err(format!(
            "Token endpoint returned unexpected HTTP {status}. Existing token was kept."
        ));
    }
    if response.headers().len() > 1000
        || response
            .headers()
            .iter()
            .map(|(n, v)| n.as_str().len() + v.as_bytes().len())
            .sum::<usize>()
            > 65_536
    {
        return Err(
            "Token response headers exceed 64 KiB or 1,000 entries. Existing token was kept."
                .into(),
        );
    }
    let headers = response.headers().clone();
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Token response was interrupted. Existing token was kept.")?
    {
        if body.len().saturating_add(chunk.len()) > RESPONSE_LIMIT {
            return Err("Token response exceeds 1 MiB. Existing token was kept.".into());
        }
        body.extend_from_slice(&chunk);
    }
    // Parsing stays off the asynchronous executor, including bounded JSON extraction.
    let parsed_plan = plan.clone();
    let record = worker(move || {
        let (token, renewal_token, seconds) = if let Some(definition) = match &parsed_plan.config {
            Auth::CustomLogin { config } => Some(if old.is_some() {
                config.renewal.as_ref().unwrap_or(&config.login)
            } else {
                &config.login
            }),
            _ => None,
        } {
            let token = extract(&definition.token, &body, &headers, false)?;
            let renewal = match &definition.renewal_token {
                Some(extraction) => Some(extract(extraction, &body, &headers, false)?),
                None => old.and_then(|r| r.renewal_token),
            };
            let seconds = match &definition.expires_in {
                Some(extraction) => Some(lifetime(&extract(extraction, &body, &headers, true)?)?),
                None => definition.lifetime_seconds,
            };
            (token, renewal, seconds)
        } else {
            let json: serde_json::Value = serde_json::from_slice(&body)
                .map_err(|_| "OAuth2 response is not valid JSON. Existing token was kept.")?;
            if !json
                .get("token_type")
                .and_then(|v| v.as_str())
                .is_some_and(|v| v.eq_ignore_ascii_case("bearer"))
            {
                return Err(
                    "OAuth2 must return the Bearer token type. Existing token was kept.".into(),
                );
            }
            let token = json
                .get("access_token")
                .and_then(|v| v.as_str())
                .ok_or("OAuth2 access_token is missing. Existing token was kept.")?
                .to_owned();
            let seconds = json
                .get("expires_in")
                .map(|v| {
                    lifetime(
                        v.as_str()
                            .map(String::from)
                            .unwrap_or_else(|| v.to_string())
                            .as_str(),
                    )
                })
                .transpose()?;
            (token, None, seconds)
        };
        parsed_plan.placed_value(&token)?;
        let expires_at = seconds
            .map(|s| {
                acquired_at
                    .checked_add(s)
                    .ok_or("Token expiry overflows the clock.")
            })
            .transpose()?;
        if expires_at.is_some_and(|v| v <= now().unwrap_or(u64::MAX)) {
            return Err("Token expired during acquisition. Existing token was kept.".into());
        }
        Ok(Record {
            schema_version: 1,
            token,
            renewal_token,
            acquired_at,
            expires_at,
        })
    })
    .await?;
    let saved = record.clone();
    worker(move || {
        let bytes = serde_json::to_string(&saved).map_err(|_| "Could not encode local token data.")?;
        if bytes.len() > RECORD_LIMIT { return Err("Token and renewal data exceed the 2,560-byte OS credential limit. Existing token was kept.".into()); }
        if cancel.is_cancelled() { return Err("Token acquisition cancelled before storing the result.".into()); }
        values.set(&plan.key, &bytes).map_err(|_| "Could not store acquired token. No API request was sent. The prior token may remain; check the OS store and retry explicitly.".into())
    }).await?;
    Ok(record)
}

#[cfg(test)]
mod tests;
