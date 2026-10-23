use super::*;
use crate::projects::{environments::Definitions, model::*};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use uuid::Uuid;

fn message(list: &mut Vec<String>, text: &str) {
    if list.len() < 200 && !list.iter().any(|s| s == text) {
        list.push(text.into());
    }
}
fn templated(text: &str) -> Result<(String, Vec<String>), String> {
    let mut out = String::new();
    let mut rest = text;
    let mut names = vec![];
    while let Some(start) = rest.find("{{") {
        if rest[..start].contains(['{', '}']) {
            return Err("URL contains ambiguous literal braces.".into());
        }
        out.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let end = tail
            .find("}}")
            .ok_or("URL contains an unclosed variable.")?;
        let name = &tail[..end];
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        {
            return Err("URL contains an invalid variable name.".into());
        }
        out.push('{');
        out.push_str(name);
        out.push('}');
        names.push(name.into());
        rest = &tail[end + 2..];
    }
    if rest.contains(['{', '}']) {
        return Err("URL contains ambiguous literal braces.".into());
    }
    out.push_str(rest);
    Ok((out, names))
}
fn url(
    request: &Request,
    warnings: &mut Vec<String>,
) -> Result<(String, Value, Vec<Value>), String> {
    let text = request.url.split(['?', '#']).next().unwrap_or_default();
    if text.is_empty() {
        return Err("A saved request has no URL. Configure and save it before export.".into());
    }
    let (base, path) = if let Some((scheme, tail)) = text.split_once("://") {
        if !matches!(scheme, "http" | "https") {
            return Err("Only literal HTTP(S) URL schemes can be exported.".into());
        }
        let (authority, path) = tail.split_once('/').unwrap_or((tail, ""));
        if authority.is_empty() || authority.contains('@') {
            return Err("Empty hosts or embedded URL credentials block export. Move credentials to authentication first.".into());
        }
        (format!("{scheme}://{authority}"), format!("/{path}"))
    } else if text.starts_with("{{") {
        let end = text.find("}}").ok_or("Unclosed base URL variable.")? + 2;
        if !text[end..].is_empty() && !text[end..].starts_with('/') {
            return Err("A base URL placeholder must be followed by a path or stand alone.".into());
        }
        if text[end..].is_empty() {
            return Err("An entire URL stored in one variable has an unknown path. Save an explicit path outside the base URL placeholder before exporting.".into());
        }
        (
            text[..end].into(),
            if text[end..].is_empty() {
                "/".into()
            } else {
                text[end..].into()
            },
        )
    } else if text.starts_with('/') {
        message(warnings,"Relative request URLs use a synthetic https://example.invalid server. Configure it in the exported description.");
        ("https://example.invalid".into(), text.into())
    } else {
        return Err("URL structure needs local values to determine its server/path. Save an explicit URL or a leading base-URL variable before export.".into());
    };
    let (base, server_names) = templated(&base)?;
    let (path, path_names) = templated(&path)?;
    if path.chars().any(char::is_whitespace) {
        return Err("Encode whitespace in saved URL paths before export.".into());
    }
    let mut variables = Map::new();
    let mut rendered = base.clone();
    for name in server_names {
        let token = format!("{{{name}}}");
        let default = if base == token {
            "https://example.invalid"
        } else if base.contains(&format!(":{token}")) {
            "443"
        } else {
            "example.invalid"
        };
        rendered = rendered.replace(&token, default);
        variables.insert(name,json!({"default":default,"description":"Synthetic placeholder; set a local value. Fluxkit did not export the stored value."}));
    }
    if reqwest::Url::parse(&rendered).is_err() {
        return Err(
            "The saved server URL cannot be represented as a valid HTTP(S) server template.".into(),
        );
    }
    let mut server = json!({"url":base});
    if !variables.is_empty() {
        server["variables"] = variables.into();
        message(warnings,"Server variables use synthetic defaults. No local environment or OS-store value is exported; replace the defaults before using the API description.");
    }
    let parameters = path_names
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|name| json!({"name":name,"in":"path","required":true,"schema":{"type":"string"}}))
        .collect();
    Ok((path, server, parameters))
}
fn effective<'a>(
    project: &'a ProjectView,
    collection: &'a Collection,
    request: &'a Request,
) -> Option<&'a Auth> {
    let mut auth = collection.auth.as_ref().or(project.data.auth.as_ref());
    let mut ancestors = vec![];
    let mut parent = request.folder_id.as_ref();
    while let Some(id) = parent {
        let folder = collection.folders.iter().find(|f| &f.id == id)?;
        ancestors.push(folder);
        parent = folder.parent_id.as_ref();
    }
    for folder in ancestors.into_iter().rev() {
        auth = folder.auth.as_ref().or(auth);
    }
    request.auth.as_ref().or(auth)
}
fn security(
    auth: Option<&Auth>,
    request: &Request,
    schemes: &mut Map<String, Value>,
    warnings: &mut Vec<String>,
) -> Value {
    let manual = match auth {
        Some(Auth::ApiKey {
            name, placement, ..
        }) => match placement {
            KeyPlacement::Header => request
                .headers
                .iter()
                .any(|e| e.enabled && e.key.eq_ignore_ascii_case(name)),
            KeyPlacement::Query => {
                request.params.iter().any(|e| e.enabled && e.key == *name)
                    || request.url.split_once('?').is_some_and(|(_, q)| {
                        reqwest::Url::parse(&format!("http://localhost/?{q}"))
                            .is_ok_and(|u| u.query_pairs().any(|(k, _)| k == *name))
                    })
            }
        },
        Some(
            Auth::Basic { .. }
            | Auth::Bearer { .. }
            | Auth::Oauth2 { .. }
            | Auth::CustomLogin { .. },
        ) => request
            .headers
            .iter()
            .any(|e| e.enabled && e.key.eq_ignore_ascii_case("authorization")),
        _ => false,
    };
    if manual {
        message(warnings,"Manual authentication fields override inherited configuration. Their values are omitted and no automatic security requirement is claimed for that operation.");
        return json!([]);
    }
    let definition = match auth {
        None | Some(Auth::None) => return json!([]),
        Some(Auth::Basic { .. }) => json!({"type":"http","scheme":"basic"}),
        Some(Auth::Bearer { .. }) => json!({"type":"http","scheme":"bearer"}),
        Some(Auth::ApiKey {
            name, placement, ..
        }) => {
            json!({"type":"apiKey","name":name,"in":if *placement==KeyPlacement::Header{"header"}else{"query"}})
        }
        Some(Auth::Oauth2 { .. }) => {
            message(warnings,"OAuth2 Client Credentials is described with a synthetic token endpoint and empty scopes. Local token URL/scope compositions, client credentials, acquired tokens and renewal state are not exported.");
            json!({"type":"oauth2","flows":{"clientCredentials":{"tokenUrl":"https://example.invalid/oauth/token","scopes":{}}}})
        }
        Some(Auth::CustomLogin { .. }) => {
            message(warnings,"Custom Login automation has no standard OpenAPI mapping. Configure authentication manually; no credentials or login request are exported.");
            return json!([]);
        }
    };
    let name = if let Some((name, _)) = schemes.iter().find(|(_, v)| **v == definition) {
        name.clone()
    } else {
        let name = format!("auth{}", schemes.len() + 1);
        schemes.insert(name.clone(), definition);
        name
    };
    let mut requirement = Map::new();
    requirement.insert(name, json!([]));
    json!([requirement])
}
// Only API-description metadata is carried; runnable callbacks/links and annotations holding
// example/default values are omitted. Property names and literal schema constraints survive.
fn preserve(v: &Value, old: bool, schema: bool, names: bool) -> Value {
    match v {
        Value::Object(m) => {
            let mut result = Map::new();
            for (key, value) in m {
                if !names
                    && (matches!(
                        key.as_str(),
                        "example"
                            | "examples"
                            | "default"
                            | "servers"
                            | "security"
                            | "callbacks"
                            | "links"
                            | "externalDocs"
                            | "externalValue"
                    ) || key.starts_with("x-"))
                {
                    continue;
                }
                if schema && !names && matches!(key.as_str(), "enum" | "const") {
                    result.insert(key.clone(), value.clone());
                    continue;
                }
                let child_schema = if names {
                    schema
                } else {
                    matches!(key.as_str(), "schema" | "schemas")
                        || (schema
                            && matches!(
                                key.as_str(),
                                "items"
                                    | "additionalProperties"
                                    | "not"
                                    | "allOf"
                                    | "anyOf"
                                    | "oneOf"
                                    | "contains"
                                    | "if"
                                    | "then"
                                    | "else"
                                    | "prefixItems"
                                    | "unevaluatedProperties"
                            ))
                };
                let child_names = !names
                    && matches!(
                        key.as_str(),
                        "schemas"
                            | "properties"
                            | "patternProperties"
                            | "$defs"
                            | "definitions"
                            | "responses"
                            | "headers"
                            | "requestBodies"
                            | "parameters"
                            | "content"
                    );
                result.insert(
                    key.clone(),
                    preserve(
                        value,
                        old,
                        child_schema || (schema && child_names),
                        child_names,
                    ),
                );
            }
            if schema && !names && old {
                let nullable = result.remove("nullable").and_then(|v| v.as_bool());
                if nullable == Some(true) {
                    if let Some(kind) = result.get_mut("type") {
                        if let Some(kind_name) = kind.as_str() {
                            *kind = json!([kind_name, "null"]);
                        }
                    } else {
                        let nonnull = Value::Object(result);
                        return json!({"anyOf":[nonnull,{"type":"null"}]});
                    }
                }
                for (exclusive, inclusive) in [
                    ("exclusiveMinimum", "minimum"),
                    ("exclusiveMaximum", "maximum"),
                ] {
                    if let Some(flag) = result.get(exclusive).and_then(Value::as_bool) {
                        result.remove(exclusive);
                        if flag {
                            if let Some(limit) = result.remove(inclusive) {
                                result.insert(exclusive.into(), limit);
                            }
                        }
                    }
                }
            }
            Value::Object(result)
        }
        Value::Array(a) => {
            Value::Array(a.iter().map(|v| preserve(v, old, schema, false)).collect())
        }
        _ => v.clone(),
    }
}
fn body(
    request: &Request,
    original: Option<&Value>,
    old: bool,
    warnings: &mut Vec<String>,
) -> Option<Value> {
    if matches!(request.body.kind, BodyKind::None) {
        return None;
    }
    let media = match request.body.kind {
        BodyKind::Json => "application/json",
        BodyKind::Text => "text/plain",
        BodyKind::UrlEncoded => "application/x-www-form-urlencoded",
        BodyKind::Multipart => "multipart/form-data",
        BodyKind::File => "application/octet-stream",
        BodyKind::None => return None,
    };
    let content_type = request
        .headers
        .iter()
        .find(|e| e.enabled && e.key.eq_ignore_ascii_case("content-type"));
    let media = if let Some(e) = content_type {
        if e.value.contains(['{', '}']) {
            message(
                warnings,
                "Variable Content-Type values use the body-type default media type in the export.",
            );
            media
        } else if e
            .value
            .split(';')
            .next()
            .is_some_and(|s| s.contains('/') && !s.chars().any(char::is_whitespace))
        {
            e.value.split(';').next().unwrap_or(media)
        } else {
            media
        }
    } else {
        media
    };
    let schema = match request.body.kind {
        BodyKind::UrlEncoded | BodyKind::Multipart => {
            let mut properties = Map::new();
            for field in request.body.fields.iter().filter(|f| f.enabled) {
                if properties.contains_key(&field.key) {
                    message(warnings,"Repeated form field names become a single schema property; exact multipart/form serialization cannot be reproduced.");
                }
                properties.insert(
                    field.key.clone(),
                    if field.file.is_some() {
                        json!({"type":"string","format":"binary"})
                    } else {
                        json!({"type":"string"})
                    },
                );
            }
            json!({"type":"object","properties":properties})
        }
        BodyKind::File => json!({"type":"string","format":"binary"}),
        BodyKind::Json => json!({}),
        _ => json!({"type":"string"}),
    };
    let source = original
        .and_then(|op| op.get("requestBody"))
        .and_then(|b| b.get("content"))
        .and_then(|c| c.get(media))
        .and_then(|m| m.get("schema"));
    let schema = source
        .map(|s| preserve(s, old, true, false))
        .unwrap_or(schema);
    let mut content = Map::new();
    content.insert(media.into(), json!({"schema":schema}));
    Some(json!({"content":content}))
}
fn canonical_path(path: &str) -> String {
    let mut out = String::new();
    let mut variable = false;
    for c in path.chars() {
        match c {
            '{' => {
                variable = true;
                out.push_str("{}");
            }
            '}' => variable = false,
            _ if !variable => out.push(c),
            _ => {}
        }
    }
    out
}
fn check_refs(root: &Value, value: &Value, depth: usize, names: bool) -> Result<(), String> {
    if depth > 64 {
        return Err("Export metadata exceeds the nesting limit.".into());
    }
    match value {
        Value::Object(m) => {
            if !names {
                if let Some(r) = m.get("$ref").and_then(Value::as_str) {
                    if !r.starts_with("#/")
                        || crate::imports::document::resolve(root, value).is_err()
                    {
                        return Err("Preserved metadata has an external, missing or cyclic non-schema reference. Simplify that source metadata before exporting.".into());
                    }
                }
            }
            for (key, v) in m {
                if names || !matches!(key.as_str(), "enum" | "const") {
                    let child_names = !names
                        && matches!(
                            key.as_str(),
                            "properties"
                                | "patternProperties"
                                | "$defs"
                                | "definitions"
                                | "schemas"
                                | "responses"
                                | "headers"
                                | "parameters"
                                | "requestBodies"
                                | "securitySchemes"
                                | "paths"
                                | "content"
                        );
                    check_refs(root, v, depth + 1, child_names)?;
                }
            }
        }
        Value::Array(a) => {
            for v in a {
                check_refs(root, v, depth + 1, false)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn validate_descriptions(document: &Value) -> Result<(), String> {
    fn schemas(value: &Value, schema: bool, names: bool) -> Result<(), String> {
        if schema && !names && !(value.is_object() || value.is_boolean() || value.is_array()) {
            return Err("Invalid schema shape.".into());
        }
        if let Some(m) = value.as_object() {
            for (key, v) in m {
                if schema && !names && matches!(key.as_str(), "enum" | "const") {
                    continue;
                }
                if schema && !names && key == "type" {
                    let valid = |s: &str| {
                        matches!(
                            s,
                            "string"
                                | "number"
                                | "integer"
                                | "boolean"
                                | "object"
                                | "array"
                                | "null"
                        )
                    };
                    if !(v.as_str().is_some_and(valid)
                        || v.as_array().is_some_and(|a| {
                            !a.is_empty() && a.iter().all(|v| v.as_str().is_some_and(valid))
                        }))
                    {
                        return Err("Invalid schema type.".into());
                    }
                }
                let child_names = !names
                    && matches!(
                        key.as_str(),
                        "properties"
                            | "patternProperties"
                            | "$defs"
                            | "definitions"
                            | "schemas"
                            | "responses"
                            | "headers"
                            | "parameters"
                            | "requestBodies"
                            | "securitySchemes"
                            | "paths"
                            | "content"
                    );
                let child_schema = if names {
                    schema
                } else {
                    matches!(key.as_str(), "schema" | "schemas")
                        || schema
                            && (child_names
                                || matches!(
                                    key.as_str(),
                                    "items"
                                        | "additionalProperties"
                                        | "not"
                                        | "allOf"
                                        | "anyOf"
                                        | "oneOf"
                                        | "contains"
                                        | "if"
                                        | "then"
                                        | "else"
                                        | "prefixItems"
                                        | "unevaluatedProperties"
                                ))
                };
                schemas(v, child_schema, child_names)?;
            }
        } else if let Some(a) = value.as_array() {
            for v in a {
                schemas(v, schema, false)?;
            }
        }
        Ok(())
    }
    schemas(document, false, false)?;
    for path in document["paths"]
        .as_object()
        .ok_or("Invalid paths.")?
        .values()
    {
        for operation in path.as_object().ok_or("Invalid path item.")?.values() {
            for (status, response) in operation["responses"]
                .as_object()
                .ok_or("Invalid responses.")?
            {
                if status != "default"
                    && !(status.len() == 3
                        && status
                            .bytes()
                            .next()
                            .is_some_and(|b| (b'1'..=b'5').contains(&b))
                        && (status[1..].bytes().all(|b| b.is_ascii_digit())
                            || &status[1..] == "XX"))
                {
                    return Err("Invalid response status.".into());
                }
                let response = crate::imports::document::resolve(document, response)?;
                if response
                    .get("description")
                    .and_then(Value::as_str)
                    .is_none()
                {
                    return Err("A response needs a description.".into());
                }
            }
        }
    }
    Ok(())
}
fn build(
    project: &ProjectView,
    collection: &Collection,
    selected: &[&Request],
    warnings: &mut Vec<String>,
    blockers: &mut Vec<String>,
) -> Result<Document, String> {
    let mut paths = Map::new();
    let mut schemes = Map::new();
    let mut seen = HashSet::new();
    let mut path_templates = BTreeMap::new();
    let mut components = Map::new();
    if let Some(meta) = &collection.open_api {
        if let Some(source) = meta.document.get("components").and_then(Value::as_object) {
            for name in [
                "schemas",
                "responses",
                "headers",
                "parameters",
                "requestBodies",
            ] {
                if let Some(value) = source.get(name) {
                    let mut wrapper = Map::new();
                    wrapper.insert(name.into(), value.clone());
                    let preserved = preserve(
                        &Value::Object(wrapper),
                        meta.version.starts_with("3.0."),
                        false,
                        false,
                    );
                    if let Some(value) = preserved.get(name) {
                        components.insert(name.into(), value.clone());
                    }
                }
            }
        }
        message(warnings,"Imported schemas and response descriptions are retained where available; source examples/defaults, callbacks, links and vendor annotations are omitted. Review schemas after editing requests; Fluxkit does not infer response schemas from live responses.");
    }
    for request in selected {
        if !request.import_issues.is_empty() {
            message(warnings,"Some selected requests still require import review. Their exported descriptions may be incomplete; review authentication and fields before using them.");
        }
        let method = request.method.to_ascii_lowercase();
        if !matches!(
            method.as_str(),
            "get" | "put" | "post" | "delete" | "options" | "head" | "patch" | "trace"
        ) {
            message(blockers,"An HTTP method has no OpenAPI operation mapping (for example CONNECT). Adjust the saved request before export.");
            continue;
        }
        let (path, server, mut parameters) = match url(request, warnings) {
            Ok(v) => v,
            Err(e) => {
                message(blockers, &e);
                continue;
            }
        };
        let canonical = canonical_path(&path);
        if path_templates
            .get(&canonical)
            .is_some_and(|existing| existing != &path)
        {
            message(blockers,"Equivalent path templates use different parameter names. Align their saved path names before export.");
            continue;
        }
        path_templates.insert(canonical.clone(), path.clone());
        let key = (canonical, method.clone());
        if !seen.insert(key) {
            message(blockers,"Multiple requests map to the same method and path template, even if their servers differ. Export a smaller selection or resolve the conflict; no operation is silently overwritten.");
            continue;
        }
        let original = collection.open_api.as_ref().and_then(|m| {
            m.operations.get(&request.id).and_then(|r| {
                m.document
                    .pointer(r.strip_prefix('#')?)
                    .and_then(|op| crate::imports::document::resolve(&m.document, op).ok())
            })
        });
        let old = collection
            .open_api
            .as_ref()
            .is_some_and(|m| m.version.starts_with("3.0."));
        let mut names: HashSet<(String, String)> = HashSet::new();
        let mut rows = request.params.clone();
        if let Some((_, query)) = request.url.split_once('?') {
            if let Ok(parsed) = reqwest::Url::parse(&format!(
                "http://localhost/?{}",
                query.split('#').next().unwrap_or_default()
            )) {
                for (key, _) in parsed.query_pairs() {
                    rows.push(Entry {
                        id: rows.len() as u32,
                        enabled: true,
                        key: key.into(),
                        value: String::new(),
                    });
                }
            }
        }
        for (location, rows) in [
            ("query", rows.as_slice()),
            ("header", request.headers.as_slice()),
        ] {
            for entry in rows.iter().filter(|e| e.enabled) {
                if entry.key.contains(['{', '}']) {
                    message(blockers,"Variable header/query/form field names cannot be represented as fixed OpenAPI names. Use literal names before export.");
                    continue;
                }
                if location == "header"
                    && matches!(
                        entry.key.to_ascii_lowercase().as_str(),
                        "authorization" | "cookie" | "content-type" | "accept"
                    )
                {
                    message(warnings,"Authorization/Cookie/Accept/Content-Type headers are not emitted as ordinary parameters. Auth is described separately; all manual values are omitted.");
                    continue;
                }
                let identity = (
                    location.into(),
                    if location == "header" {
                        entry.key.to_ascii_lowercase()
                    } else {
                        entry.key.clone()
                    },
                );
                if !names.insert(identity) {
                    message(blockers,"Duplicate enabled query/header names cannot be represented without choosing serialization semantics. Export a request with unique names.");
                    continue;
                }
                parameters.push(json!({"name":entry.key,"in":location,"required":false,"schema":{"type":"string"}}));
            }
        }
        let mut operation = json!({"operationId":format!("request_{}",request.id.replace('-',"_")),"summary":request.name,"servers":[server],"parameters":parameters,"security":security(effective(project,collection,request),request,&mut schemes,warnings),"responses":{"default":{"description":"Response schema is unspecified."}}});
        if let Some(original) = original {
            if let Some(description) = original.get("description").and_then(Value::as_str) {
                operation["description"] = description.into();
            }
            if let Some(responses) = original
                .get("responses")
                .filter(|r| r.as_object().is_some_and(|m| !m.is_empty()))
            {
                operation["responses"] = preserve(responses, old, false, true);
            }
        }
        if request
            .body
            .fields
            .iter()
            .any(|f| f.enabled && f.key.contains(['{', '}']))
        {
            message(
                blockers,
                "Variable form field names cannot be represented as fixed schema properties.",
            );
        }
        if let Some(body) = body(request, original, old, warnings) {
            operation["requestBody"] = body;
        }
        if let Some(folder) = request
            .folder_id
            .as_ref()
            .and_then(|id| collection.folders.iter().find(|f| &f.id == id))
        {
            operation["tags"] = json!([folder.name]);
        }
        paths.entry(path).or_insert_with(|| json!({}))[method] = operation;
    }
    if paths.is_empty() {
        message(
            blockers,
            "This selection has no exportable saved operations.",
        );
    }
    components.insert("securitySchemes".into(), schemes.into());
    let version = collection
        .open_api
        .as_ref()
        .and_then(|m| m.document.pointer("/info/version"))
        .and_then(Value::as_str)
        .unwrap_or("1.0.0");
    let document = json!({"openapi":"3.1.0","info":{"title":collection.name,"version":version},"paths":paths,"components":components});
    if validate_descriptions(&document).is_err() {
        message(blockers,"Preserved response/schema metadata has an invalid shape for OpenAPI 3.1. Repair or remove that metadata before exporting.");
    }
    if check_refs(&document, &document, 0, false).is_err() {
        message(blockers,"Preserved metadata contains unsupported or missing references after selection. Export without that source metadata or repair it before retrying.");
    }
    let yaml = serde_saphyr::to_string(&document)
        .map_err(|_| "Could not serialize the OpenAPI description.")?;
    if yaml.len() > 4 * 1024 * 1024 {
        return Err("An exported document exceeds 4 MiB. Choose a smaller selection.".into());
    }
    let roundtrip = crate::imports::document::parse_bounded(yaml.as_bytes())?;
    if roundtrip != document {
        return Err("OpenAPI YAML round-trip validation failed.".into());
    }
    Ok(Document {
        name: format!(
            "{}-{}.yml",
            collection
                .name
                .chars()
                .take(50)
                .map(|c| if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                })
                .collect::<String>()
                .trim_matches('-'),
            collection.id
        ),
        yaml,
    })
}
pub fn plan(
    project: ProjectView,
    definitions: Definitions,
    revision: String,
    target: Target,
) -> Result<Plan, String> {
    let mut warnings = vec![];
    let mut blockers = vec![];
    let mut documents = vec![];
    message(&mut warnings,"Only saved requests are exported. No OS credential-store access, network request, environment values, tokens, header/query/form values, raw body examples, attachments or history are included.");
    message(&mut warnings,"Saved literal URL hosts/paths, request names and imported schema constraints/descriptions may contain private information. Review the generated description before sharing it. This is an API description, not a workspace backup.");
    message(&mut warnings,"Folder structure becomes operation tags. Disabled rows are omitted. Parameter schemas are generic strings unless preserved body/response metadata applies.");
    let directory = matches!(target, Target::Project);
    let selected_collection = match &target {
        Target::Project => None,
        Target::Collection { collection }
        | Target::Folder { collection, .. }
        | Target::Request { collection, .. } => {
            validate_id(collection)?;
            Some(collection)
        }
    };
    if let Target::Folder { folder, .. } = &target {
        validate_id(folder)?;
    }
    if let Target::Request { request, .. } = &target {
        validate_id(request)?;
    }
    let collections: Vec<_> = project
        .collections
        .iter()
        .filter(|c| selected_collection.is_none_or(|id| c.data.id == *id))
        .collect();
    if collections.is_empty() {
        return Err("Select a saved collection in the current project.".into());
    }
    for c in collections {
        let mut folders = HashSet::new();
        if let Target::Folder { folder, .. } = &target {
            if !c.data.folders.iter().any(|f| f.id == *folder) {
                return Err("Selected folder no longer exists.".into());
            }
            folders.insert(folder.clone());
            loop {
                let before = folders.len();
                for f in &c.data.folders {
                    if f.parent_id.as_ref().is_some_and(|p| folders.contains(p)) {
                        folders.insert(f.id.clone());
                    }
                }
                if before == folders.len() {
                    break;
                }
            }
        }
        let selected: Vec<_> = c
            .data
            .requests
            .iter()
            .filter(|r| match &target {
                Target::Request { request, .. } => r.id == *request,
                Target::Folder { .. } => {
                    r.folder_id.as_ref().is_some_and(|id| folders.contains(id))
                }
                _ => true,
            })
            .collect();
        if let Target::Request { .. } = &target {
            if selected.is_empty() {
                return Err("Selected saved request no longer exists.".into());
            }
        }
        if selected.is_empty() {
            message(
                &mut warnings,
                "Empty collections/folders have no operations and are omitted from project export.",
            );
            if !directory {
                message(&mut blockers, "The selected scope has no saved operations.");
            }
            continue;
        }
        let referenced: HashSet<_> = definitions
            .variables
            .iter()
            .map(|v| v.name.as_str())
            .collect();
        for request in &selected {
            let (_, names) = templated(request.url.split(['?', '#']).next().unwrap_or_default())
                .unwrap_or_default();
            if names.iter().any(|n| !referenced.contains(n.as_str())) {
                message(&mut warnings,"Some URL placeholders have no project definition. They remain explicit OpenAPI placeholders with synthetic server defaults.");
            }
        }
        documents.push(build(
            &project,
            &c.data,
            &selected,
            &mut warnings,
            &mut blockers,
        )?);
    }
    if documents.is_empty() {
        message(
            &mut blockers,
            "No saved operations were selected for export.",
        );
    }
    if documents.iter().map(|d| d.yaml.len()).sum::<usize>() > 32 * 1024 * 1024 {
        return Err("Export exceeds the 32 MiB total limit. Select fewer collections.".into());
    }
    let id = Uuid::new_v4().to_string();
    let preview = Preview {
        id: id.clone(),
        title: project.data.name.clone(),
        directory,
        folder_name: format!("fluxkit-export-{}", &id[..8]),
        files: documents
            .iter()
            .map(|d| FilePreview {
                name: d.name.clone(),
                operations: crate::imports::document::parse(d.yaml.as_bytes())
                    .ok()
                    .and_then(|v| v.get("paths").cloned())
                    .and_then(|p| {
                        p.as_object().map(|p| {
                            p.values()
                                .map(|v| v.as_object().map_or(0, |m| m.len()))
                                .sum()
                        })
                    })
                    .unwrap_or(0),
                bytes: d.yaml.len(),
            })
            .collect(),
        warnings,
        blockers,
    };
    Ok(Plan {
        preview,
        snapshot: project,
        definitions_revision: revision,
        documents,
        created: Instant::now(),
    })
}
