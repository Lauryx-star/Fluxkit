use serde_json::Value;
use std::collections::HashSet;

pub const INPUT_LIMIT: usize = 4 * 1024 * 1024;
pub(crate) fn parse_bounded(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > INPUT_LIMIT {
        return Err("OpenAPI files must not exceed 4 MiB.".into());
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| "OpenAPI files must be UTF-8 YAML or JSON.")?;
    let options = serde_saphyr::options! {
        strict_booleans: true,
        duplicate_keys: serde_saphyr::DuplicateKeyPolicy::Error,
        reject_unsupported_tags: true,
        alias_limits: serde_saphyr::alias_limits! { max_total_replayed_events:100_000, max_replay_stack_depth:16, max_alias_expansions_per_anchor:1000 },
        budget: serde_saphyr::budget! {
            max_depth: 64, max_nodes: 100_000, max_events: 300_000,
            max_total_scalar_bytes: INPUT_LIMIT, max_documents: 1,
            max_aliases: 1000, max_anchors: 1000,
            max_recorded_anchor_bytes: INPUT_LIMIT, max_recorded_anchor_events: 100_000,
        },
    };
    let document: Value = serde_saphyr::from_str_with_options(text, options)
        .map_err(|_| "Invalid, duplicate-key, tagged, multi-document, or overly complex YAML/JSON. Check the source file; no data was imported.")?;
    Ok(document)
}
pub fn parse(bytes: &[u8]) -> Result<Value, String> {
    let document = parse_bounded(bytes)?;
    let version = document.get("openapi").and_then(Value::as_str).ok_or(
        "Select an OpenAPI 3.0.x or 3.1.x document. Swagger 2.0 is not supported in this importer.",
    )?;
    if !supported(version) {
        return Err(
            "Unsupported OpenAPI version. This importer accepts 3.0.x and 3.1.x only.".into(),
        );
    }
    if document
        .pointer("/info/title")
        .and_then(Value::as_str)
        .is_none_or(|v| v.trim().is_empty())
        || document
            .pointer("/info/version")
            .and_then(Value::as_str)
            .is_none()
    {
        return Err("OpenAPI info.title and info.version are required strings.".into());
    }
    let mut nodes = 0;
    check(&document, &document, 0, &mut nodes)?;
    Ok(document)
}
pub fn supported(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    parts.len() == 3
        && parts[0] == "3"
        && matches!(parts[1], "0" | "1")
        && !parts[2].is_empty()
        && (parts[2].len() == 1 || !parts[2].starts_with('0'))
        && parts[2].bytes().all(|b| b.is_ascii_digit())
}
fn target<'a>(document: &'a Value, reference: &str) -> Result<&'a Value, String> {
    let fragment=reference.strip_prefix('#').ok_or("Only in-document JSON Pointer $ref references are supported. Bundle external files/URLs into one document first; nothing was fetched.")?;
    let mut decoded = Vec::new();
    let raw = fragment.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%' {
            let digits = raw
                .get(index + 1..index + 3)
                .ok_or("Invalid percent encoding in an internal reference.")?;
            let high = (digits[0] as char)
                .to_digit(16)
                .ok_or("Invalid reference escape.")?;
            let low = (digits[1] as char)
                .to_digit(16)
                .ok_or("Invalid reference escape.")?;
            decoded.push((high * 16 + low) as u8);
            index += 3;
        } else {
            decoded.push(raw[index]);
            index += 1;
        }
    }
    let pointer =
        std::str::from_utf8(&decoded).map_err(|_| "Internal references must use UTF-8.")?;
    if !pointer.starts_with('/') {
        return Err("Internal references must use a JSON Pointer beginning with #/.".into());
    }
    document.pointer(pointer).ok_or(
        "An internal $ref target does not exist. Repair the source document before importing."
            .into(),
    )
}
fn check(value: &Value, document: &Value, depth: usize, nodes: &mut usize) -> Result<(), String> {
    *nodes += 1;
    if depth > 64 || *nodes > 100_000 {
        return Err("OpenAPI document exceeds the structural limits.".into());
    }
    match value {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref") {
                target(
                    document,
                    reference.as_str().ok_or("$ref must be a string.")?,
                )?;
            }
            for (key, child) in map {
                if !matches!(key.as_str(), "example" | "examples" | "default")
                    && !key.starts_with("x-")
                {
                    check(child, document, depth + 1, nodes)?;
                }
            }
        }
        Value::Array(values) => {
            for child in values {
                check(child, document, depth + 1, nodes)?;
            }
        }
        _ => {}
    }
    Ok(())
}
/// Resolve semantic objects without recursively expanding schema graphs.
pub fn resolve<'a>(document: &'a Value, mut value: &'a Value) -> Result<&'a Value, String> {
    let mut visited = HashSet::new();
    while let Some(reference) = value.get("$ref") {
        let reference = reference.as_str().ok_or("$ref must be a string.")?;
        if !visited.insert(reference) || visited.len() > 32 {
            return Err("A non-schema $ref is cyclic or exceeds 32 indirections.".into());
        }
        value = target(document, reference)?;
    }
    Ok(value)
}
/// Preserve API schemas/descriptions for later export, without duplicating imported values.
pub fn metadata(value: &Value) -> Value {
    metadata_at(value, false)
}
fn metadata_at(value: &Value, names: bool) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| {
                    names
                        || (!matches!(
                            key.as_str(),
                            "example" | "examples" | "default" | "servers" | "tokenUrl"
                        ) && !key.starts_with("x-"))
                })
                .map(|(key, value)| {
                    let child = if !names && matches!(key.as_str(), "enum" | "const") {
                        value.clone()
                    } else {
                        metadata_at(
                            value,
                            !names
                                && matches!(
                                    key.as_str(),
                                    "properties"
                                        | "patternProperties"
                                        | "$defs"
                                        | "definitions"
                                        | "schemas"
                                        | "parameters"
                                        | "headers"
                                        | "responses"
                                        | "requestBodies"
                                        | "securitySchemes"
                                        | "callbacks"
                                        | "links"
                                        | "paths"
                                        | "webhooks"
                                ),
                        )
                    };
                    (key.clone(), child)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| metadata_at(value, false))
                .collect(),
        ),
        _ => value.clone(),
    }
}
