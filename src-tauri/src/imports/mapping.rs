use super::{
    document::{self, resolve},
    ImportedVariable, Plan, Preview, RequestPreview, VariablePreview,
};
use crate::projects::{
    environments::{Definition, Definitions},
    model::*,
    store::ProjectView,
    token_config::{ClientAuthentication, Composition, OAuth2, Part},
};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    time::Instant,
};
use uuid::Uuid;

fn object(value: &Value) -> Result<&Map<String, Value>, String> {
    value
        .as_object()
        .ok_or("An OpenAPI object has an invalid shape.".into())
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or("A required OpenAPI string is missing or invalid.".into())
}
fn label(text: &str) -> String {
    let mut text = text
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>();
    while text.len() > 180 {
        text.pop();
    }
    if text.is_empty() {
        "Imported API".into()
    } else {
        text
    }
}
fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(_) | Value::Bool(_) => Some(value.to_string()),
        _ => None,
    }
}
fn example(document: &Value, value: &Value) -> Result<Option<Value>, String> {
    if let Some(v) = value.get("example") {
        return Ok(Some(v.clone()));
    }
    if let Some(examples) = value.get("examples") {
        for item in object(examples)?.values() {
            let item = resolve(document, item)?;
            if let Some(v) = item.get("value") {
                return Ok(Some(v.clone()));
            }
        }
    }
    if let Some(schema) = value.get("schema") {
        let schema = resolve(document, schema)?;
        return Ok(schema
            .get("example")
            .or_else(|| {
                schema
                    .get("examples")
                    .and_then(Value::as_array)
                    .and_then(|v| v.first())
            })
            .or_else(|| schema.get("default"))
            .cloned());
    }
    Ok(None)
}
struct Mapper<'a> {
    document: &'a Value,
    variables: Vec<ImportedVariable>,
    warnings: Vec<String>,
    blockers: Vec<String>,
    used_names: HashSet<String>,
    sources: Vec<String>,
    review: Vec<String>,
    servers: HashMap<String, String>,
    security: HashMap<String, Auth>,
}
impl Mapper<'_> {
    fn warn(&mut self, text: &str) {
        if self.warnings.len() < 200 && !self.warnings.iter().any(|s| s == text) {
            self.warnings.push(text.into());
        }
    }
    fn block(&mut self, text: &str) {
        if self.blockers.len() < 100 && !self.blockers.iter().any(|s| s == text) {
            self.blockers.push(text.into());
        }
    }
    fn variable(
        &mut self,
        name: &str,
        mut value: Option<String>,
        source: &str,
    ) -> Result<(String, String), String> {
        if self.variables.len() >= 1000 {
            return Err("This import exceeds the 1,000 project-variable limit. Split the specification into smaller collections.".into());
        }
        if value.as_ref().is_some_and(|v| v.len() > 2560) {
            value = None;
            self.warn("Some defaults/examples exceed the OS credential limit (2,560 bytes) and require manual values.");
        }
        let slug: String = name
            .chars()
            .take(70)
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let base = if slug.is_empty() {
            "variable".into()
        } else if slug.as_bytes()[0].is_ascii_digit() {
            format!("var_{slug}")
        } else {
            slug
        };
        let mut name = base.clone();
        let mut suffix = 2;
        while !self.used_names.insert(name.clone()) {
            name = format!("{base}_{suffix}");
            suffix += 1;
        }
        self.sources.push(label(source));
        let id = Uuid::new_v4().to_string();
        let placeholder = format!("{{{{{name}}}}}");
        self.variables.push(ImportedVariable {
            definition: Definition {
                id: id.clone(),
                name,
                secret: None,
            },
            value,
            environment: None,
        });
        Ok((id, placeholder))
    }
    fn literal_parameter(&mut self, value: Option<String>, required: bool, origin: &str) -> String {
        if value
            .as_ref()
            .is_some_and(|text| text.contains("{{") || text.contains("}}"))
        {
            self.note(&format!("Replace the source placeholder in {} with an explicit value or a deliberately configured project variable.", label(origin)));
            return String::new();
        }
        if value.is_none() && required {
            self.note(&format!(
                "Configure the required {} before sending.",
                label(origin)
            ));
        }
        value.unwrap_or_default()
    }
    fn note(&mut self, text: &str) {
        if self.review.len() < 99 && !self.review.iter().any(|note| note == text) {
            self.review.push(text.into());
        } else if self.review.len() == 99 {
            self.review
                .push("Review all remaining required fields before sending.".into());
        }
        self.warn("Requests with missing required values or source placeholders are locked until their import review is confirmed and saved.");
    }
    fn server(&mut self, operation: &Value, path: &Value) -> Result<String, String> {
        let servers = operation
            .get("servers")
            .or_else(|| path.get("servers"))
            .or_else(|| self.document.get("servers"));
        let mut url = if let Some(servers) = servers {
            let servers = servers.as_array().ok_or("servers must be an array.")?;
            if servers.len() > 1 {
                self.warn("Only the first server at each operation/path/root scope is selected. Other servers are not converted to environments.");
            }
            if let Some(server) = servers.first() {
                let mut url = string(server, "url")?.to_string();
                if let Some(variables) = server.get("variables") {
                    for (name, definition) in object(variables)? {
                        let default = string(definition, "default")?;
                        url = url.replace(&format!("{{{name}}}"), default);
                    }
                    self.warn("Server variable defaults are flattened into the imported base URL. Edit that project variable to change the server.");
                }
                url
            } else {
                "/".into()
            }
        } else {
            "/".into()
        };
        while url.ends_with('/') {
            url.pop();
        }
        if let Some(placeholder) = self.servers.get(&url) {
            return Ok(placeholder.clone());
        }
        let default = match reqwest::Url::parse(&url) {
            Ok(parsed)
                if matches!(parsed.scheme(), "http" | "https")
                    && parsed.host_str().is_some()
                    && parsed.username().is_empty()
                    && parsed.password().is_none()
                    && parsed.query().is_none()
                    && parsed.fragment().is_none()
                    && !url.contains(['{', '}']) =>
            {
                Some(url.clone())
            }
            _ => {
                self.warn("Missing, relative, templated, or unsafe server URLs require a complete HTTP(S) base URL in Project variables before sending.");
                None
            }
        };
        let (_, placeholder) = self.variable("baseUrl", default, "Server URL")?;
        self.servers.insert(url, placeholder.clone());
        Ok(placeholder)
    }
    fn security(&mut self, operation: &Value) -> Result<Auth, String> {
        let security = operation
            .get("security")
            .or_else(|| self.document.get("security"));
        let Some(security) = security else {
            return Ok(Auth::None);
        };
        let requirements = security.as_array().ok_or("security must be an array.")?;
        let Some(requirement) = requirements.first() else {
            return Ok(Auth::None);
        };
        if requirements.len() > 1 {
            self.warn("Only the first security alternative is selected; alternatives remain in sanitized OpenAPI metadata.");
        }
        let requirement = object(requirement)?;
        if requirement.is_empty() {
            return Ok(Auth::None);
        }
        if requirement.len() != 1 {
            self.block("Combined (AND) security requirements cannot be represented by one Fluxkit auth method. Adapt the source or configure a compatible security requirement before import.");
            return Ok(Auth::None);
        }
        let (name, scopes) = requirement
            .iter()
            .next()
            .ok_or("Invalid security requirement.")?;
        let scopes = scopes
            .as_array()
            .ok_or("Security scopes must be an array.")?
            .iter()
            .map(|v| v.as_str().ok_or("Security scopes must be strings."))
            .collect::<Result<Vec<_>, _>>()?
            .join(" ");
        let cache_key = format!("{name}\n{scopes}");
        if let Some(auth) = self.security.get(&cache_key) {
            return Ok(auth.clone());
        }
        let schemes = self
            .document
            .pointer("/components/securitySchemes")
            .and_then(Value::as_object)
            .ok_or("A required security scheme is missing.")?;
        let scheme = resolve(
            self.document,
            schemes
                .get(name)
                .ok_or("A required security scheme is missing.")?,
        )?;
        let auth = match string(scheme, "type")? {
            "http"
                if scheme
                    .get("scheme")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.eq_ignore_ascii_case("basic")) =>
            {
                let (username, _) = self.variable(
                    &format!("{name}_username"),
                    None,
                    &format!("Authentication: {name} (username)"),
                )?;
                let (password, _) = self.variable(
                    &format!("{name}_password"),
                    None,
                    &format!("Authentication: {name} (password)"),
                )?;
                Auth::Basic { username, password }
            }
            "http"
                if scheme
                    .get("scheme")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.eq_ignore_ascii_case("bearer")) =>
            {
                let (token, _) = self.variable(
                    &format!("{name}_token"),
                    None,
                    &format!("Authentication: {name} (token)"),
                )?;
                Auth::Bearer { token }
            }
            "apiKey"
                if matches!(
                    scheme.get("in").and_then(Value::as_str),
                    Some("header" | "query")
                ) =>
            {
                let (value, _) =
                    self.variable(name, None, &format!("Authentication: {name} (API key)"))?;
                Auth::ApiKey {
                    name: string(scheme, "name")?.into(),
                    value,
                    placement: if scheme["in"] == "header" {
                        KeyPlacement::Header
                    } else {
                        KeyPlacement::Query
                    },
                }
            }
            "oauth2" if scheme.pointer("/flows/clientCredentials").is_some() => {
                let flow = &scheme["flows"]["clientCredentials"];
                let token_url = string(flow, "tokenUrl")?;
                let (url, _) = self.variable(
                    &format!("{name}_token_url"),
                    Some(token_url.into()),
                    &format!("Authentication: {name} (token endpoint)"),
                )?;
                let (client_id, _) = self.variable(
                    &format!("{name}_client_id"),
                    None,
                    &format!("Authentication: {name} (client ID)"),
                )?;
                let (client_secret, _) = self.variable(
                    &format!("{name}_client_secret"),
                    None,
                    &format!("Authentication: {name} (client secret)"),
                )?;
                self.warn("OAuth2 Client Credentials imports use Basic client authentication. Review the token endpoint/client authentication and supply credentials before sending.");
                Auth::Oauth2 {
                    config: Box::new(OAuth2 {
                        token_url: Composition {
                            parts: vec![Part::Variable { id: url }],
                            separator: String::new(),
                            prefix: String::new(),
                            suffix: String::new(),
                            base64: false,
                        },
                        client_id,
                        client_secret,
                        scope: Composition {
                            parts: vec![Part::Literal { text: scopes }],
                            separator: String::new(),
                            prefix: String::new(),
                            suffix: String::new(),
                            base64: false,
                        },
                        client_authentication: ClientAuthentication::Basic,
                    }),
                }
            }
            _ => {
                self.block("A required security scheme is unsupported (for example cookie keys, browser OAuth, OpenID Connect, or mutual TLS). No collection will be imported until the source is adapted.");
                Auth::None
            }
        };
        validate_auth(Some(&auth))?;
        self.security.insert(cache_key, auth.clone());
        Ok(auth)
    }
    fn parameters(
        &mut self,
        path: &Value,
        operation: &Value,
        url: &mut String,
    ) -> Result<(Vec<Entry>, Vec<Entry>), String> {
        let mut merged = BTreeMap::new();
        for owner in [path, operation] {
            let mut local = std::collections::HashSet::new();
            if let Some(parameters) = owner.get("parameters") {
                for parameter in parameters
                    .as_array()
                    .ok_or("parameters must be an array.")?
                {
                    let parameter = resolve(self.document, parameter)?;
                    let key = (
                        string(parameter, "in")?.to_string(),
                        string(parameter, "name")?.to_string(),
                    );
                    if !local.insert(key.clone()) {
                        return Err(
                            "Duplicate OpenAPI parameter names within one location/scope.".into(),
                        );
                    }
                    merged.insert(key, parameter);
                }
            }
        }
        let mut params = Vec::new();
        let mut headers = Vec::new();
        let mut missing_paths = Vec::new();
        for ((location, name), parameter) in merged {
            if name.contains("{{") || name.contains("}}") {
                self.block("Parameter names containing Fluxkit placeholders cannot be imported; they must not bind existing project variables.");
            }
            let required = parameter
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if location == "cookie" {
                if required {
                    self.block("A required cookie parameter is unsupported. Adapt it to a manually configured Cookie header before import.");
                } else {
                    self.warn(
                        "Optional cookie parameters are omitted; Fluxkit has no cookie session.",
                    );
                }
                continue;
            }
            if !matches!(location.as_str(), "query" | "header" | "path") {
                return Err("Unknown OpenAPI parameter location.".into());
            }
            if location == "header" {
                reqwest::header::HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| "Invalid imported header name.")?;
            }
            if parameter.get("content").is_some()
                || parameter
                    .get("style")
                    .and_then(Value::as_str)
                    .is_some_and(|s| {
                        s != if location == "query" {
                            "form"
                        } else {
                            "simple"
                        }
                    })
                || parameter.get("allowReserved").and_then(Value::as_bool) == Some(true)
            {
                self.block("A parameter uses unsupported content/style/allowReserved serialization. Adapt it to a primitive form/simple parameter before import.");
            }
            let schema = parameter
                .get("schema")
                .map(|v| resolve(self.document, v))
                .transpose()?;
            if schema.is_some_and(|s| {
                matches!(
                    s.get("type").and_then(Value::as_str),
                    Some("array" | "object")
                )
            }) {
                self.block("Array/object parameter serialization is not supported by this first importer. Adapt the source to explicit primitive parameters.");
            }
            let origin = format!("{location} parameter: {name}");
            let raw_value = example(self.document, parameter)?.as_ref().and_then(scalar);
            let unsafe_value = raw_value
                .as_ref()
                .is_some_and(|v| v.contains("{{") || v.contains("}}"));
            let missing = raw_value.is_none() || unsafe_value;
            let mut value = self.literal_parameter(raw_value, required, &origin);
            if location == "path" {
                if !required {
                    return Err("OpenAPI path parameters must be required.".into());
                }
                if !url.contains(&format!("{{{name}}}")) {
                    return Err("A path parameter has no matching URL placeholder.".into());
                }
                if !missing {
                    let text = &value;
                    let mut encoded = reqwest::Url::parse("https://example.invalid")
                        .map_err(|_| "Could not encode path example.")?;
                    encoded
                        .path_segments_mut()
                        .map_err(|_| "Could not encode path example.")?
                        .push(text);
                    value = encoded.path().trim_start_matches('/').into();
                }
            }
            if location == "path" {
                if missing {
                    missing_paths.push(name.clone());
                    self.note(&format!(
                        "Replace the path placeholder {{{}}} in the request URL before sending.",
                        label(&name)
                    ));
                } else {
                    *url = url.replace(&format!("{{{name}}}"), &value);
                }
            } else {
                let entries = if location == "header" {
                    &mut headers
                } else {
                    &mut params
                };
                entries.push(Entry {
                    id: entries.len() as u32 + 1,
                    enabled: required,
                    key: name,
                    value,
                });
            }
        }
        // Every remaining single-brace path template needs a value, even if its declaration is missing.
        let mut path_check = url.clone();
        for name in missing_paths {
            path_check = path_check.replace(&format!("{{{name}}}"), "");
        }
        let chars = path_check.chars().collect::<Vec<_>>();
        let mut index = 0;
        while index < chars.len() {
            if chars[index] == '{' {
                if chars.get(index + 1) == Some(&'{') {
                    index += 2;
                    while index + 1 < chars.len()
                        && !(chars[index] == '}' && chars[index + 1] == '}')
                    {
                        index += 1;
                    }
                    index += 2;
                    continue;
                }
                self.block(
                    "A URL path template has no matching required path parameter declaration.",
                );
                break;
            }
            index += 1;
        }
        Ok((params, headers))
    }
    fn body(&mut self, operation: &Value, headers: &mut Vec<Entry>) -> Result<Body, String> {
        let mut body = Body {
            kind: BodyKind::None,
            text: String::new(),
            fields: vec![],
            file: None,
        };
        let Some(input) = operation.get("requestBody") else {
            return Ok(body);
        };
        let input = resolve(self.document, input)?;
        let content = object(
            input
                .get("content")
                .ok_or("requestBody.content is required.")?,
        )?;
        let media = content
            .iter()
            .find(|(mime, _)| mime.as_str() == "application/json")
            .or_else(|| content.iter().find(|(mime, _)| mime.ends_with("+json")))
            .or_else(|| {
                content.iter().find(|(mime, _)| {
                    matches!(
                        mime.as_str(),
                        "text/plain" | "application/x-www-form-urlencoded" | "multipart/form-data"
                    )
                })
            })
            .or_else(|| content.iter().next())
            .ok_or("requestBody.content must not be empty.")?;
        if content.len() > 1 {
            self.warn("Only one request-body media type is selected (JSON, then text/forms, then the first available). Other definitions remain in sanitized metadata.");
        }
        let (mime, media) = media;
        if mime.contains("{{") || mime.contains("}}") {
            self.block("Body media types containing Fluxkit placeholders are unsupported.");
        }
        let sample = example(self.document, media)?;
        if mime == "application/json" || mime.ends_with("+json") {
            body.kind = BodyKind::Json;
            body.text = sample
                .as_ref()
                .map(serde_json::to_string_pretty)
                .transpose()
                .map_err(|_| "Could not read a JSON body example.")?
                .unwrap_or_default();
            if sample.is_none() {
                self.warn("Some JSON bodies have no example/default. Fill the body before sending; schemas are preserved but no example is invented.");
            }
        } else if mime == "application/x-www-form-urlencoded" || mime == "multipart/form-data" {
            body.kind = if mime == "multipart/form-data" {
                BodyKind::Multipart
            } else {
                BodyKind::UrlEncoded
            };
            let schema = media
                .get("schema")
                .map(|v| resolve(self.document, v))
                .transpose()?;
            if media.get("encoding").is_some() {
                self.block(
                    "Per-field request-body encoding is unsupported. Simplify it before importing.",
                );
            }
            if let Some(properties) = schema.and_then(|s| s.get("properties")) {
                for (name, property) in object(properties)? {
                    if name.contains("{{") || name.contains("}}") {
                        self.block(
                            "Form names containing Fluxkit placeholders cannot be imported.",
                        );
                    }
                    let property = resolve(self.document, property)?;
                    let required = schema
                        .and_then(|s| s.get("required"))
                        .and_then(Value::as_array)
                        .is_some_and(|items| items.iter().any(|v| v.as_str() == Some(name)));
                    if property
                        .get("format")
                        .and_then(Value::as_str)
                        .is_some_and(|f| matches!(f, "binary" | "byte"))
                        || matches!(
                            property.get("type").and_then(Value::as_str),
                            Some("object" | "array")
                        )
                    {
                        self.block("Binary/array/object form fields cannot be imported automatically. Select files or explicit primitive form fields in an adapted specification.");
                        continue;
                    }
                    let value = sample
                        .as_ref()
                        .and_then(|s| s.get(name))
                        .and_then(scalar)
                        .or_else(|| {
                            property
                                .get("example")
                                .or_else(|| property.get("default"))
                                .and_then(scalar)
                        });
                    let value =
                        self.literal_parameter(value, required, &format!("form field: {name}"));
                    body.fields.push(FormField {
                        id: body.fields.len() as u32 + 1,
                        enabled: required,
                        key: name.clone(),
                        value,
                        file: None,
                    });
                }
            } else {
                self.warn("A form body has no properties. Add form fields after importing.");
            }
        } else if mime.starts_with("text/") {
            body.kind = BodyKind::Text;
            body.text = sample.as_ref().and_then(scalar).unwrap_or_default();
        } else {
            body.kind = BodyKind::File;
            self.warn("Binary or unsupported body media types require choosing a body file after import. No attachment paths are taken from the source.");
        }
        if !matches!(body.kind, BodyKind::Multipart) {
            headers.push(Entry {
                id: headers.len() as u32 + 1,
                enabled: true,
                key: "Content-Type".into(),
                value: mime.clone(),
            });
        }
        if body.text.contains("{{") || body.text.contains("}}") {
            body.text.clear();
            self.warn("A body example containing Fluxkit placeholder syntax was omitted to prevent binding existing project variables. Enter its body after importing.");
        }
        Ok(body)
    }
}

pub fn plan(
    bytes: &[u8],
    project: &ProjectView,
    definitions: Definitions,
    environment_revision: String,
) -> Result<Plan, String> {
    let document = document::parse(bytes)?;
    let version = string(&document, "openapi")?.to_string();
    let title = label(string(&document["info"], "title")?);
    let mut name = title.clone();
    let mut suffix = 2;
    while project.collections.iter().any(|c| c.data.name == name) {
        name = format!("{title} (import {suffix})");
        suffix += 1;
    }
    let id = Uuid::new_v4().to_string();
    let mut mapper = Mapper {
        document: &document,
        variables: vec![],
        warnings: vec![],
        blockers: vec![],
        used_names: definitions
            .variables
            .iter()
            .map(|v| v.name.clone())
            .collect(),
        sources: vec![],
        review: vec![],
        servers: HashMap::new(),
        security: HashMap::new(),
    };
    if name != title {
        mapper.warn("A collection with this title exists. The import uses a new, numbered collection name and never overwrites it.");
    }
    mapper.warn("Only server URLs and authentication create project variables. Parameter/form defaults and examples become editable request content in collection files; optional rows start disabled. Review literal header/query values before sharing the collection.");
    mapper.warn("Source examples/default annotations, server declarations, token endpoints and vendor extensions are excluded from preserved metadata; schemas, descriptions and response definitions remain for later export.");
    if document.get("webhooks").is_some() {
        mapper.warn("Webhooks are preserved as metadata but do not create executable requests.");
    }
    let paths = object(
        document
            .get("paths")
            .ok_or("OpenAPI paths is required for this importer.")?,
    )?;
    let mut collection = Collection {
        schema_version: 1,
        id: id.clone(),
        name: name.clone(),
        auth: Some(Auth::None),
        folders: vec![],
        requests: vec![],
        postman: None,
        open_api: Some(OpenApiMetadata {
            version: version.clone(),
            document: document::metadata(&document),
            operations: BTreeMap::new(),
        }),
    };
    let mut folders = HashMap::new();
    for (path, item) in paths {
        if path.starts_with("x-") {
            continue;
        }
        if path.contains("{{") || path.contains("}}") {
            mapper.block("Source paths containing Fluxkit placeholders cannot bind existing project variables. Use OpenAPI single-brace path parameters instead.");
        }
        if !path.starts_with('/') || path.contains(['?', '#']) {
            return Err("OpenAPI paths must start with / and contain no query or fragment.".into());
        }
        let item = resolve(&document, item)?;
        object(item)?;
        for (method, operation) in object(item)? {
            if !matches!(
                method.as_str(),
                "get" | "put" | "post" | "delete" | "options" | "head" | "patch" | "trace"
            ) {
                continue;
            }
            object(operation)?;
            match operation.get("responses") {
                None | Some(Value::Null) => mapper.warn(
                    "Some operations have no response definitions. Their requests are imported normally; no response schema is invented.",
                ),
                Some(Value::Object(responses)) if responses.is_empty() => mapper.warn(
                    "Some operations have no response definitions. Their requests are imported normally; no response schema is invented.",
                ),
                Some(Value::Object(_)) => {},
                Some(_) => return Err("Operation responses must be an object when provided. Check the source response definitions.".into()),
            }
            if operation.get("callbacks").is_some() {
                mapper.warn(
                    "Callbacks are preserved as metadata but do not create executable requests.",
                );
            }
            let name = label(
                operation
                    .get("summary")
                    .or_else(|| operation.get("operationId"))
                    .and_then(Value::as_str)
                    .unwrap_or(&format!("{} {path}", method.to_uppercase())),
            );
            let folder_id = if let Some(tags) = operation.get("tags") {
                let tags = tags.as_array().ok_or("Operation tags must be an array.")?;
                if tags.len() > 1 {
                    mapper.warn("Operations with multiple tags are placed in their first tag's folder. All tags remain in metadata.");
                }
                if let Some(tag) = tags.first() {
                    let tag = label(tag.as_str().ok_or("Tags must be strings.")?);
                    Some(
                        folders
                            .entry(tag.clone())
                            .or_insert_with(|| {
                                let id = Uuid::new_v4().to_string();
                                collection.folders.push(Folder {
                                    id: id.clone(),
                                    name: tag,
                                    auth: None,
                                    parent_id: None,
                                });
                                id
                            })
                            .clone(),
                    )
                } else {
                    None
                }
            } else {
                None
            };
            mapper.review.clear();
            let base = mapper.server(operation, item)?;
            let mut url = format!("{base}{path}");
            let (params, mut headers) = mapper.parameters(item, operation, &mut url)?;
            let body = mapper.body(operation, &mut headers)?;
            let auth = mapper.security(operation)?;
            let request_id = Uuid::new_v4().to_string();
            if let Some(metadata) = &mut collection.open_api {
                metadata.operations.insert(
                    request_id.clone(),
                    format!(
                        "#/paths/{}/{}",
                        path.replace('~', "~0").replace('/', "~1"),
                        method
                    ),
                );
            }
            collection.requests.push(Request {
                import_issues: mapper.review.clone(),
                id: request_id,
                name,
                auth: Some(auth),
                folder_id,
                method: method.to_uppercase(),
                url,
                params,
                headers,
                body,
            });
            if collection.requests.len() + collection.folders.len() > 10_000 {
                return Err("Import exceeds the 10,000 collection-item limit.".into());
            }
        }
    }
    if collection.requests.is_empty() {
        return Err("No supported HTTP operations were found in this document.".into());
    }
    collection.validate()?;
    let mut updated = definitions;
    updated
        .variables
        .extend(mapper.variables.iter().map(|v| v.definition.clone()));
    updated.validate()?;
    let preview = Preview {
        id: Uuid::new_v4().to_string(),
        source: "OpenAPI".into(),
        environment_name: None,
        collection_name: name,
        version,
        request_count: collection.requests.len(),
        folder_count: collection.folders.len(),
        review_count: collection
            .requests
            .iter()
            .filter(|r| !r.import_issues.is_empty())
            .count(),
        variables: mapper
            .variables
            .iter()
            .enumerate()
            .map(|(i, v)| VariablePreview {
                source_name: Some(mapper.sources[i].clone()),
                name: v.definition.name.clone(),
                has_default: v.value.is_some(),
            })
            .collect(),
        requests: collection
            .requests
            .iter()
            .take(200)
            .map(|r| RequestPreview {
                needs_review: !r.import_issues.is_empty(),
                name: r.name.clone(),
                method: r.method.clone(),
                url: r.url.clone(),
            })
            .collect(),
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
        environment_revision,
        collection,
        environment_only: false,
        variables: mapper.variables,
        definitions: updated,
        definitions_saved: false,
        created: Instant::now(),
    })
}
