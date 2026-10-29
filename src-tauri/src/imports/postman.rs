//! Postman data is mapped without scripts, network access, source paths, or existing values.
use super::{document, ImportedVariable, Plan, Preview, RequestPreview, VariablePreview};
use crate::projects::{
    environments::{Definition, Definitions},
    model::*,
    store::ProjectView,
    token_config::{ClientAuthentication, Composition, OAuth2, Part},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::Instant,
};
use uuid::Uuid;

fn parse(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > document::INPUT_LIMIT {
        return Err("Postman files must not exceed 4 MiB.".into());
    }
    serde_json::from_slice::<serde::de::IgnoredAny>(bytes)
        .map_err(|_| "Select a valid UTF-8 Postman JSON file with bounded nesting.")?;
    document::parse_bounded(bytes)
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or("A required Postman string is missing or invalid.".into())
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a [Value], String> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(a)) => Ok(a),
        _ => Err("A Postman list has an invalid shape.".into()),
    }
}
fn scalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(_) | Value::Bool(_) => Some(v.to_string()),
        _ => None,
    }
}
fn label(s: &str) -> String {
    let mut s = s
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>();
    while s.len() > 160 {
        s.pop();
    }
    if s.is_empty() {
        "Imported Postman".into()
    } else {
        s
    }
}
fn unique_name(title: &str, names: impl Iterator<Item = String>) -> String {
    let names: HashSet<_> = names.collect();
    let mut name = label(title);
    let mut n = 2;
    while names.contains(&name) {
        name = format!("{} (import {n})", label(title));
        n += 1;
    }
    name
}
/// Turns a source label into a valid Fluxkit variable name (letters, digits, `_`, `.`, `-`).
fn variable_name(source: &str) -> String {
    let mut name: String = source
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(180)
        .collect();
    if name.is_empty() {
        name = "variable".into();
    } else if !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        name.insert(0, '_');
    }
    name
}
#[derive(Default)]
struct Mapper {
    /// Existing project variables by name; matching source references reuse them.
    existing: HashMap<String, Definition>,
    /// Every name in the project or this import, so generated names stay unique.
    taken: HashSet<String>,
    variables: Vec<ImportedVariable>,
    bindings: BTreeMap<String, String>,
    warnings: Vec<String>,
    blockers: Vec<String>,
    review: Vec<String>,
}
impl Mapper {
    fn warn(&mut self, s: &str) {
        if self.warnings.len() < 200 && !self.warnings.iter().any(|v| v == s) {
            self.warnings.push(s.into());
        }
    }
    fn block(&mut self, s: &str) {
        if self.blockers.len() < 100 && !self.blockers.iter().any(|v| v == s) {
            self.blockers.push(s.into());
        }
    }
    fn needs_review(&mut self, text: &str) {
        if self.review.len() < 100 && !self.review.iter().any(|s| s == text) {
            self.review.push(text.into());
        }
        self.warn("Some requests need manual configuration and are locked against sending and token acquisition until their import review is explicitly confirmed and saved.");
    }
    fn new(definitions: &Definitions, bindings: BTreeMap<String, String>) -> Self {
        Mapper {
            existing: definitions
                .variables
                .iter()
                .map(|v| (v.name.clone(), v.clone()))
                .collect(),
            taken: definitions
                .variables
                .iter()
                .map(|v| v.name.clone())
                .collect(),
            bindings,
            ..Default::default()
        }
    }
    /// `secret` classifies the definition: secret values are never shared as plain project data
    /// in server workspaces. Only literal credentials and Postman `type: secret` are secret.
    fn new_variable(
        &mut self,
        source: &str,
        mut value: Option<String>,
        secret: bool,
    ) -> Result<String, String> {
        if self.variables.len() >= 1000 {
            return Err("Import exceeds the 1,000-variable limit.".into());
        }
        if value.as_ref().is_some_and(|v| v.len() > 2560) {
            value = None;
            self.warn("Values larger than 2,560 bytes require manual configuration in Variables.");
        }
        if value
            .as_ref()
            .is_some_and(|v| v.contains("{{") || v.contains("}}"))
        {
            value = None;
            self.block("Nested placeholders in stored variable values cannot be reproduced by Fluxkit's single-pass resolution. Flatten those source values before import.");
        }
        let base = variable_name(source);
        let mut name = base.clone();
        let mut suffix = 2;
        while !self.taken.insert(name.clone()) {
            name = format!("{base}_{suffix}");
            suffix += 1;
        }
        let definition = Definition {
            id: Uuid::new_v4().to_string(),
            name,
            secret: Some(secret),
        };
        let id = definition.id.clone();
        self.variables.push(ImportedVariable {
            definition,
            value,
            environment: None,
            reused: false,
        });
        Ok(id)
    }
    fn binding(&mut self, name: &str) -> Result<String, String> {
        validate_name(name).map_err(|_| {
            "Postman variable names must contain 1–200 bytes without control characters."
        })?;
        if name.contains(['{', '}']) {
            return Err("Invalid braces in a Postman variable name.".into());
        }
        if name.starts_with('$') {
            self.needs_review("Postman dynamic variables were converted to unconfigured project variables. Supply a deliberate local value; Fluxkit does not generate dynamic values or execute scripts.");
        }
        if let Some(id) = self.bindings.get(name) {
            return Ok(id.clone());
        }
        let id = if let Some(definition) = self.existing.get(name).cloned() {
            self.warn("Source variables named like existing project variables reuse those variables. Their existing values stay unchanged; Postman defaults are not stored for them. Rename them in the preview to create separate variables.");
            let id = definition.id.clone();
            self.variables.push(ImportedVariable {
                definition,
                value: None,
                environment: None,
                reused: true,
            });
            id
        } else {
            self.new_variable(name, None, false)?
        };
        self.bindings.insert(name.into(), id.clone());
        Ok(id)
    }
    /// Postman `type: secret` marks a new import-owned variable secret. Existing project
    /// variables keep their classification.
    fn classify(&mut self, id: &str, source: &Value) {
        if source.get("type").and_then(Value::as_str) != Some("secret") {
            return;
        }
        if let Some(variable) = self
            .variables
            .iter_mut()
            .find(|v| v.definition.id == id && !v.reused)
        {
            variable.definition.secret = Some(true);
        }
    }
    fn placeholder(&self, id: &str) -> Result<String, String> {
        self.variables
            .iter()
            .find(|v| v.definition.id == id)
            .map(|v| format!("{{{{{}}}}}", v.definition.name))
            .ok_or("Imported variable identity is unavailable.".into())
    }
    fn rewrite(&mut self, text: &str) -> Result<String, String> {
        let mut rest = text;
        let mut out = String::new();
        while let Some(start) = rest.find("{{") {
            if rest[..start].contains("}}") {
                return Err("Unbalanced Postman variable placeholders.".into());
            }
            out.push_str(&rest[..start]);
            let tail = &rest[start + 2..];
            let end = tail
                .find("}}")
                .ok_or("Unbalanced Postman variable placeholders.")?;
            let id = self.binding(tail[..end].trim())?;
            out.push_str(&self.placeholder(&id)?);
            rest = &tail[end + 2..];
        }
        if rest.contains("}}") {
            return Err("Unbalanced Postman variable placeholders.".into());
        }
        out.push_str(rest);
        if out.len() > 4 * 1024 * 1024 {
            return Err("Expanded Postman content exceeds the import limit.".into());
        }
        Ok(out)
    }
    fn credential(&mut self, name: &str, text: &str) -> Result<String, String> {
        if let Some(source) = text.strip_prefix("{{").and_then(|s| s.strip_suffix("}}")) {
            if !source.contains(['{', '}']) {
                return self.binding(source.trim());
            }
        }
        if text.contains("{{") || text.contains("}}") {
            self.block("Composed Basic/Bearer/API Key credentials require a single variable reference or literal value. Adapt the source credential before import.");
            return self.new_variable(name, None, true);
        }
        self.new_variable(name, Some(text.into()), true)
    }
    fn auth_credential(
        &mut self,
        attributes: &HashMap<&str, String>,
        key: &str,
    ) -> Result<String, String> {
        match attributes.get(key) {
            Some(value) => self.credential(key, value),
            None => self.new_variable(key, None, true),
        }
    }
    fn composition(&mut self, name: &str, text: &str) -> Result<Composition, String> {
        // Literal segments are also OS-held, so token inputs never copy reusable values to files.
        let mut rest = text;
        let mut parts = vec![];
        while let Some(start) = rest.find("{{") {
            if start > 0 {
                parts.push(Part::Variable {
                    id: self.new_variable(name, Some(rest[..start].into()), false)?,
                });
            }
            let tail = &rest[start + 2..];
            let end = tail
                .find("}}")
                .ok_or("Unbalanced token-input placeholders.")?;
            parts.push(Part::Variable {
                id: self.binding(tail[..end].trim())?,
            });
            rest = &tail[end + 2..];
        }
        if !rest.is_empty() {
            parts.push(Part::Variable {
                id: self.new_variable(name, Some(rest.into()), false)?,
            });
        }
        Ok(Composition {
            parts,
            separator: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            base64: false,
        })
    }
    fn auth(&mut self, object: &Value, root: bool) -> Result<Option<Auth>, String> {
        let Some(auth) = object.get("auth").filter(|v| !v.is_null()) else {
            return Ok(if root { Some(Auth::None) } else { None });
        };
        let kind = string(auth, "type")?;
        if kind == "noauth" {
            return Ok(Some(Auth::None));
        }
        let mut attributes = HashMap::new();
        for a in array(auth, kind)? {
            let key = string(a, "key")?;
            if attributes
                .insert(key, a.get("value").and_then(scalar).unwrap_or_default())
                .is_some()
            {
                return Err("Duplicate Postman auth attributes.".into());
            }
        }
        let get = |key: &str| attributes.get(key).cloned().unwrap_or_default();
        let result = match kind {
            "basic" => Auth::Basic {
                username: self.auth_credential(&attributes, "username")?,
                password: self.auth_credential(&attributes, "password")?,
            },
            "bearer" => Auth::Bearer {
                token: self.auth_credential(&attributes, "token")?,
            },
            "apikey" => {
                let name = get("key");
                if name.contains("{{") {
                    self.block(
                        "Templated API Key names require manual source changes before import.",
                    );
                }
                let placement = match get("in").as_str() {
                    "header" => KeyPlacement::Header,
                    "query" => KeyPlacement::Query,
                    _ => return Err("Postman API Key placement must be header or query.".into()),
                };
                Auth::ApiKey {
                    name,
                    value: match attributes.get("value") {
                        Some(value) => self.credential("apiKey", value)?,
                        None => self.new_variable("apiKey", None, true)?,
                    },
                    placement,
                }
            }
            "oauth2" if get("grant_type") == "client_credentials" => {
                if !get("addTokenTo").is_empty() && get("addTokenTo") != "header" {
                    self.block("OAuth token placement outside the Authorization header is unsupported in this importer.");
                }
                if !get("headerPrefix").is_empty() && get("headerPrefix") != "Bearer" {
                    self.block(
                        "Custom OAuth header prefixes require manual source changes before import.",
                    );
                }
                let client_authentication = match get("client_authentication").as_str() {
                    "" | "header" => ClientAuthentication::Basic,
                    "body" => ClientAuthentication::Body,
                    _ => {
                        self.block("Unsupported OAuth client authentication setting.");
                        ClientAuthentication::Basic
                    }
                };
                let token_url = if get("accessTokenUrl").is_empty() {
                    let id = self.new_variable("tokenUrl", None, false)?;
                    Composition {
                        parts: vec![Part::Variable { id }],
                        separator: String::new(),
                        prefix: String::new(),
                        suffix: String::new(),
                        base64: false,
                    }
                } else {
                    self.composition("tokenUrl", &get("accessTokenUrl"))?
                };
                Auth::Oauth2 {
                    config: Box::new(OAuth2 {
                        token_url,
                        client_id: self.auth_credential(&attributes, "clientId")?,
                        client_secret: self.auth_credential(&attributes, "clientSecret")?,
                        scope: self.composition("scope", &get("scope"))?,
                        client_authentication,
                    }),
                }
            }
            "oauth2" if !get("accessToken").is_empty() => {
                self.warn("An OAuth configuration with a saved access token becomes manual Bearer authentication. Its original browser/renewal flow is not reproduced.");
                Auth::Bearer {
                    token: self.credential("accessToken", &get("accessToken"))?,
                }
            }
            _ => {
                let label = match kind {
                    "digest" => "Digest",
                    "ntlm" => "NTLM",
                    "awsv4" => "AWS Signature",
                    "oauth1" => "OAuth1",
                    "oauth2" => "OAuth2",
                    "hawk" => "Hawk",
                    "edgegrid" => "EdgeGrid",
                    _ => "unsupported",
                };
                self.needs_review(&format!("Postman {label} authentication could not be transferred. Select and configure authentication in Fluxkit before confirming this review; the unsupported method was not activated."));
                Auth::None
            }
        };
        Ok(Some(result))
    }
    fn report(&mut self, object: &Value) -> Result<(), String> {
        if !array(object, "event")?.is_empty() {
            self.warn("Pre-request scripts and tests are omitted and never executed. Configure required login behavior manually; scripts are not translated into Custom Login.");
        }
        if object
            .get("protocolProfileBehavior")
            .is_some_and(|v| v.as_object().is_none_or(|v| !v.is_empty()))
        {
            self.warn("Postman protocol settings (redirects, TLS, cookies, proxy and runtime overrides) are omitted. Fluxkit uses its own visible HTTP execution rules.");
        }
        if !array(object, "response")?.is_empty() {
            self.warn("Saved responses and cookies are omitted; no response payload or session is imported.");
        }
        if object.get("description").is_some() {
            self.warn("Postman descriptions and source annotations are not preserved in this request migration.");
        }
        Ok(())
    }
    fn entries(&mut self, object: &Value, key: &str) -> Result<Vec<Entry>, String> {
        let mut result = vec![];
        if let Some(Value::String(headers)) = object.get(key) {
            self.warn("Raw header blocks are parsed as one header per line. Folded or malformed lines are rejected.");
            for line in headers.lines().filter(|s| !s.is_empty()) {
                let (name, value) = line
                    .split_once(':')
                    .ok_or("Invalid raw Postman header block.")?;
                result.push(Entry {
                    id: result.len() as u32,
                    enabled: true,
                    key: self.rewrite(name.trim())?,
                    value: self.rewrite(value.trim())?,
                });
            }
            return Ok(result);
        }
        for item in array(object, key)? {
            let name = string(item, "key")?;
            result.push(Entry {
                id: result.len() as u32,
                enabled: !item
                    .get("disabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                key: self.rewrite(name)?,
                value: self.rewrite(&item.get("value").and_then(scalar).unwrap_or_default())?,
            });
        }
        Ok(result)
    }
    fn body(&mut self, request: &Value) -> Result<Body, String> {
        let mut body = Body {
            kind: BodyKind::None,
            text: String::new(),
            fields: vec![],
            file: None,
        };
        let Some(source) = request.get("body").filter(|v| !v.is_null()) else {
            return Ok(body);
        };
        if source.get("disabled").and_then(Value::as_bool) == Some(true) {
            self.warn("A disabled Postman body is omitted.");
            return Ok(body);
        }
        match string(source,"mode")? {
            "raw" => {
                body.text = self.rewrite(source.get("raw").and_then(Value::as_str).unwrap_or_default())?;
                body.kind = if source.pointer("/options/raw/language").and_then(Value::as_str) == Some("json") {BodyKind::Json} else {BodyKind::Text};
                if let Some(language) = source.pointer("/options/raw/language").and_then(Value::as_str) {
                    if !matches!(language,"json"|"text") {self.warn("Raw body language metadata is omitted. Review the Content-Type header before sending.");}
                }
            },
            mode @ ("urlencoded" | "formdata") => {
                body.kind = if mode == "urlencoded" {BodyKind::UrlEncoded} else {BodyKind::Multipart};
                for field in array(source,mode)? {
                    if field.get("type").and_then(Value::as_str) == Some("file") {
                        self.needs_review("Multipart file fields have no local attachment. Open Body and attach the intended local file, or deliberately remove/disable the field. Source file paths were never opened.");
                        let id=self.new_variable(&format!("{}_file",string(field,"key")?),None,false)?;
                        body.fields.push(FormField {id:body.fields.len() as u32,enabled:!field.get("disabled").and_then(Value::as_bool).unwrap_or(false),key:self.rewrite(string(field,"key")?)?,value:self.placeholder(&id)?,file:None});
                        continue;
                    }
                    if field.get("contentType").and_then(Value::as_str).is_some_and(|s|!s.is_empty()) {self.block("Per-field multipart Content-Type cannot be reproduced. Adapt those fields before import.");}
                    body.fields.push(FormField {id:body.fields.len() as u32,enabled:!field.get("disabled").and_then(Value::as_bool).unwrap_or(false),key:self.rewrite(string(field,"key")?)?,value:self.rewrite(&field.get("value").and_then(scalar).unwrap_or_default())?,file:None});
                }
            },
            "file" => {body.kind=BodyKind::File;self.warn("Choose a body attachment after import. No source file path is read or copied.");},
            _ => self.block("GraphQL and unsupported body modes cannot be translated to a REST request automatically. Adapt the body before importing."),
        }
        Ok(body)
    }
    fn request(&mut self, item: &Value, folder_id: Option<String>) -> Result<Request, String> {
        let source = item.get("request").ok_or("Postman item has no request.")?;
        let simple;
        let source = if let Some(url) = source.as_str() {
            simple = serde_json::json!({"url":url,"method":"GET"});
            &simple
        } else {
            source
        };
        self.report(source)?;
        let raw_url = source.get("url").ok_or("Postman request URL is missing.")?;
        let mut url = if let Some(raw) = raw_url
            .as_str()
            .or_else(|| raw_url.get("raw").and_then(Value::as_str))
        {
            raw.to_string()
        } else {
            let host = raw_url
                .get("host")
                .ok_or("A structured URL needs raw or host.")?;
            let host = if let Some(s) = host.as_str() {
                s.to_string()
            } else {
                host.as_array()
                    .ok_or("Invalid structured URL host.")?
                    .iter()
                    .map(|v| v.as_str().ok_or("Invalid URL host segment."))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(".")
            };
            let path = match raw_url.get("path") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Array(a)) => a
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .or_else(|| v.get("value").and_then(Value::as_str))
                            .ok_or("Invalid URL path segment.")
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join("/"),
                _ => String::new(),
            };
            format!(
                "{}://{}{}{}/{}",
                string(raw_url, "protocol")?,
                host,
                if raw_url.get("port").is_some() {
                    ":"
                } else {
                    ""
                },
                raw_url
                    .get("port")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                path.trim_start_matches('/')
            )
        };
        if url.split_once("://").is_some_and(|(_, tail)| {
            tail.split(['/', '?', '#'])
                .next()
                .is_some_and(|s| s.contains('@'))
        }) {
            self.block("Embedded URL credentials are not imported. Move them to explicit Basic authentication before import.");
            url.clear();
        }
        let mut params = self.entries(raw_url, "query")?;
        if raw_url.get("query").is_some_and(Value::is_array) {
            url = url
                .split_once('?')
                .map(|(s, _)| s)
                .unwrap_or(&url)
                .to_string();
        } else if let Some((base, query)) = url.clone().split_once('?') {
            url = base.to_string();
            for (name, value) in reqwest::Url::parse(&format!(
                "http://localhost/?{}",
                query.split('#').next().unwrap_or_default()
            ))
            .map_err(|_| "Invalid URL query.")?
            .query_pairs()
            {
                params.push(Entry {
                    id: params.len() as u32,
                    enabled: true,
                    key: self.rewrite(&name)?,
                    value: self.rewrite(&value)?,
                });
            }
        }
        if url.contains('#') {
            self.warn("URL fragments are omitted because HTTP does not transmit them.");
            url = url.split('#').next().unwrap_or_default().into();
        }
        // Postman path variables are separate from collection/environment variables.
        if array(raw_url, "variable")?.iter().any(|v| {
            v.get("value")
                .and_then(Value::as_str)
                .is_some_and(|s| s.contains("{{") || s.contains("}}"))
        }) {
            self.block("Nested placeholders in :path variable values require explicit encoded values before import.");
        }
        let path_values: HashMap<_, _> = array(raw_url, "variable")?
            .iter()
            .map(|v| {
                Ok((
                    string(v, "key")?.to_string(),
                    v.get("value").and_then(scalar),
                ))
            })
            .collect::<Result<_, String>>()?;
        let (head, path) = url
            .split_once("://")
            .map(|(a, b)| (format!("{a}://"), b.to_string()))
            .unwrap_or((String::new(), url.clone()));
        let mut segments = vec![];
        for (index, segment) in path.split('/').enumerate() {
            if let Some(name) = segment
                .strip_prefix(':')
                .filter(|_| index > 0 || head.is_empty())
            {
                let candidate = path_values.get(name).cloned().flatten().map(|v| {
                    v.bytes()
                        .map(|b| {
                            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                                (b as char).to_string()
                            } else {
                                format!("%{b:02X}")
                            }
                        })
                        .collect::<String>()
                });
                match candidate.filter(|v| !v.is_empty()) {
                    Some(value) => {
                        segments.push(value);
                        self.warn("Postman :path values are inserted into the URL, encoded as one segment.");
                    }
                    None => {
                        let id = self.new_variable(name, None, false)?;
                        segments.push(self.placeholder(&id)?);
                        self.warn("Postman :path parameters without a value become new variables that need a value before sending.");
                    }
                }
            } else {
                segments.push(self.rewrite(segment)?);
            }
        }
        url = format!("{}{}", self.rewrite(&head)?, segments.join("/"));
        let headers = self.entries(source, "header")?;
        let body = self.body(source)?;
        let auth = self.auth(source, false)?;
        Ok(Request {
            import_issues: self.review.clone(),
            id: Uuid::new_v4().to_string(),
            name: label(
                item.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Imported request"),
            ),
            auth,
            folder_id,
            method: source
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("GET")
                .to_uppercase(),
            url,
            params,
            headers,
            body,
        })
    }
    fn items(
        &mut self,
        source: &Value,
        collection: &mut Collection,
        parent: Option<String>,
        depth: usize,
    ) -> Result<(), String> {
        if depth > 32 {
            return Err("Postman folder nesting exceeds 32 levels.".into());
        }
        let inherited_review = self.review.clone();
        for item in array(source, "item")? {
            self.review = inherited_review.clone();
            self.report(item)?;
            if !array(item, "variable")?.is_empty() {
                self.block("Folder/request scoped Postman variables cannot be flattened without changing precedence. Move them to collection variables before import.");
            }
            if item.get("request").is_some() && item.get("item").is_some() {
                return Err("A Postman item cannot be both folder and request.".into());
            }
            if item.get("item").is_some() {
                let id = Uuid::new_v4().to_string();
                collection.folders.push(Folder {
                    id: id.clone(),
                    name: label(
                        item.get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("Imported folder"),
                    ),
                    auth: self.auth(item, false)?,
                    parent_id: parent.clone(),
                });
                self.items(item, collection, Some(id), depth + 1)?;
            } else {
                collection
                    .requests
                    .push(self.request(item, parent.clone())?);
            }
            if collection.folders.len() + collection.requests.len() > 10_000 {
                return Err("Import exceeds 10,000 collection items.".into());
            }
        }
        Ok(())
    }
}
fn finish(
    mapper: Mapper,
    project: &ProjectView,
    collection: Collection,
    mut definitions: Definitions,
    revision: String,
    environment_name: Option<String>,
) -> Result<Plan, String> {
    let existing: HashSet<_> = definitions.variables.iter().map(|v| v.id.clone()).collect();
    definitions.variables.extend(
        mapper
            .variables
            .iter()
            .filter(|v| !existing.contains(&v.definition.id))
            .map(|v| v.definition.clone()),
    );
    definitions.validate()?;
    collection.validate()?;
    let preview = Preview {
        id: Uuid::new_v4().to_string(),
        source: "Postman".into(),
        environment_name: environment_name.clone(),
        collection_name: collection.name.clone(),
        version: "2.1".into(),
        request_count: if environment_name.is_some() {
            0
        } else {
            collection.requests.len()
        },
        folder_count: if environment_name.is_some() {
            0
        } else {
            collection.folders.len()
        },
        review_count: if environment_name.is_some() {
            0
        } else {
            collection
                .requests
                .iter()
                .filter(|r| !r.import_issues.is_empty())
                .count()
        },
        variables: mapper
            .variables
            .iter()
            .map(|v| VariablePreview {
                source_name: mapper
                    .bindings
                    .iter()
                    .find(|(_, id)| **id == v.definition.id)
                    .map(|(name, _)| name.clone()),
                name: v.definition.name.clone(),
                has_default: v.value.is_some() && (!v.reused || v.environment.is_some()),
                existing: v.reused,
                secret: v.definition.secret.unwrap_or(true),
            })
            .collect(),
        requests: if environment_name.is_some() {
            vec![]
        } else {
            collection
                .requests
                .iter()
                .take(200)
                .map(|r| RequestPreview {
                    needs_review: !r.import_issues.is_empty(),
                    name: r.name.clone(),
                    method: r.method.clone(),
                    url: r.url.clone(),
                })
                .collect()
        },
        warnings: mapper.warnings,
        blockers: mapper.blockers,
    };
    Ok(Plan {
        preview,
        handle: project.handle.clone(),
        project_revision: project.revision.clone(),
        collection_revisions: project
            .collections
            .iter()
            .map(|c| (c.data.id.clone(), c.revision.clone()))
            .collect(),
        environment_revision: revision,
        collection,
        environment_only: environment_name.is_some(),
        variables: mapper.variables,
        definitions,
        definitions_saved: false,
        local_saved: false,
        synced: false,
        created: Instant::now(),
    })
}
pub fn collection(
    bytes: &[u8],
    project: &ProjectView,
    definitions: Definitions,
    revision: String,
) -> Result<Plan, String> {
    let source = parse(bytes)?;
    let schema = source
        .pointer("/info/schema")
        .and_then(Value::as_str)
        .ok_or("Postman info.schema is required.")?;
    if !matches!(
        schema,
        "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"
            | "https://schema.getpostman.com/json/draft-07/collection/v2.1.0/"
            | "https://schema.postman.com/collection/json/v2.1.0/draft-07/collection.json"
    ) {
        return Err(
            "Only Postman Collection v2.1 JSON is supported. Export that format before importing."
                .into(),
        );
    }
    if source.get("item").and_then(Value::as_array).is_none() {
        return Err("Postman collection item must be an array.".into());
    }
    let id = Uuid::new_v4().to_string();
    let mut mapper = Mapper::new(&definitions, BTreeMap::new());
    mapper.warn("Source variable names are kept. Names that already exist in the project reuse that project variable; missing/global placeholders become unconfigured variables.");
    mapper.warn("Literal URLs and header/query/form/path values stay editable request content in collection JSON. Recognized credentials use OS-held variables. Review literal values before sharing the collection.");
    mapper.report(&source)?;
    let mut seen = HashSet::new();
    for v in array(&source, "variable")? {
        let name = string(v, "key").or_else(|_| string(v, "id"))?;
        if !seen.insert(name) {
            return Err("Duplicate collection variable names are ambiguous.".into());
        }
        let id = mapper.binding(name)?;
        mapper.classify(&id, v);
        if v.get("disabled").and_then(Value::as_bool) == Some(true) {
            mapper.warn("Disabled collection variables import without a local value.");
            continue;
        }
        let value = v.get("value").and_then(scalar);
        let index = mapper
            .variables
            .iter()
            .position(|v| v.definition.id == id)
            .ok_or("Variable mapping failed.")?;
        if value
            .as_ref()
            .is_some_and(|v| v.contains("{{") || v.contains("}}"))
        {
            mapper.block(
                "Nested placeholders in stored variable values must be flattened before import.",
            );
        } else if value.as_ref().is_some_and(|v| v.len() > 2560) {
            mapper
                .warn("Values larger than 2,560 bytes require manual configuration in Variables.");
        } else {
            mapper.variables[index].value = value;
        }
    }
    let mut collection = Collection {
        schema_version: 1,
        id,
        name: unique_name(
            string(&source["info"], "name")?,
            project.collections.iter().map(|c| c.data.name.clone()),
        ),
        auth: mapper.auth(&source, true)?,
        open_api: None,
        postman: None,
        folders: vec![],
        requests: vec![],
    };
    mapper.items(&source, &mut collection, None, 0)?;
    collection.postman = Some(PostmanMetadata {
        version: "2.1".into(),
        variables: mapper.bindings.clone(),
    });
    finish(mapper, project, collection, definitions, revision, None)
}
pub fn environment(
    bytes: &[u8],
    project: &ProjectView,
    mut definitions: Definitions,
    revision: String,
    collection_id: &str,
) -> Result<Plan, String> {
    let source = parse(bytes)?;
    if source
        .get("_postman_variable_scope")
        .and_then(Value::as_str)
        != Some("environment")
        || source.get("values").and_then(Value::as_array).is_none()
    {
        return Err("Select a Postman environment JSON export with values and _postman_variable_scope: environment. Globals and bulk exports are unsupported.".into());
    }
    let collection = project
        .collections
        .iter()
        .find(|c| c.data.id == collection_id)
        .ok_or("Select an imported Postman collection in this project.")?
        .data
        .clone();
    let metadata = collection
        .postman
        .as_ref()
        .ok_or("This collection has no Postman variable mapping.")?;
    let env_id = Uuid::new_v4().to_string();
    let name = unique_name(
        string(&source, "name")?,
        definitions.environments.iter().map(|v| v.name.clone()),
    );
    definitions.environments.push(Definition {
        id: env_id.clone(),
        name: name.clone(),
        secret: None,
    });
    let mut mapper = Mapper::new(&definitions, metadata.variables.clone());
    mapper.warn("A new project-wide environment is added; selection stays unchanged. Matching source names use this collection's variable identities. Other collections only use them if they explicitly reference the same variables.");
    let mut seen = HashSet::new();
    for item in array(&source, "values")? {
        let key = string(item, "key")?;
        if !seen.insert(key) {
            return Err("Duplicate environment keys are ambiguous.".into());
        }
        if item.get("enabled").and_then(Value::as_bool) == Some(false) {
            mapper.warn(
                "Disabled environment entries are omitted; collection defaults remain effective.",
            );
            continue;
        }
        let mut value = item.get("value").and_then(scalar);
        if value
            .as_ref()
            .is_some_and(|v| v.len() > 2560 || v.contains("{{") || v.contains("}}"))
        {
            value = None;
            mapper.warn("Oversized or nested environment values require manual configuration; no value was adopted.");
        }
        let definition = if let Some(id) = metadata.variables.get(key) {
            definitions.variables.iter().find(|d|&d.id==id).cloned().ok_or("A mapped collection variable was removed. Restore it or reimport the collection before importing this environment.")?
        } else {
            mapper.warn("Environment keys not referenced by the selected collection use the existing project variable with that name or receive a new standalone variable.");
            let id = mapper.binding(key)?;
            mapper.classify(&id, item);
            mapper
                .variables
                .iter()
                .find(|v| v.definition.id == id)
                .map(|v| v.definition.clone())
                .ok_or("Variable mapping failed.")?
        };
        if item.get("type").and_then(Value::as_str) == Some("secret")
            && definition.secret == Some(false)
            && value.take().is_some()
        {
            mapper.warn("Secret environment values of ordinary (non-secret) variables were not adopted. Mark the variable secret or enter the value manually.");
        }
        if let Some(v) = mapper
            .variables
            .iter_mut()
            .find(|v| v.definition.id == definition.id)
        {
            v.value = value;
            v.environment = Some(env_id.clone());
        } else {
            mapper.variables.push(ImportedVariable {
                definition,
                value,
                environment: Some(env_id.clone()),
                reused: true,
            });
        }
    }
    finish(
        mapper,
        project,
        collection,
        definitions,
        revision,
        Some(name),
    )
}
