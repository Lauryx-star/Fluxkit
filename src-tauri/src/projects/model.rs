use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<Auth>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Collection {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<Auth>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_api: Option<OpenApiMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postman: Option<PostmanMetadata>,
    pub folders: Vec<Folder>,
    pub requests: Vec<Request>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Folder {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<Auth>,
    pub parent_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub import_issues: Vec<String>,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<Auth>,
    pub folder_id: Option<String>,
    pub method: String,
    pub url: String,
    pub params: Vec<Entry>,
    pub headers: Vec<Entry>,
    pub body: Body,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    pub id: u32,
    pub enabled: bool,
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Body {
    pub kind: BodyKind,
    pub text: String,
    pub fields: Vec<FormField>,
    pub file: Option<Attachment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BodyKind {
    None,
    Json,
    Text,
    UrlEncoded,
    Multipart,
    File,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormField {
    pub id: u32,
    pub enabled: bool,
    pub key: String,
    pub value: String,
    pub file: Option<Attachment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attachment {
    pub path: String,
    pub name: String,
}

pub fn validate_id(value: &str) -> Result<(), String> {
    if Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value) {
        Ok(())
    } else {
        Err("Invalid entity identifier.".into())
    }
}

pub fn validate_name(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 200 || value.chars().any(char::is_control) {
        return Err("Names must contain 1 to 200 characters without control characters.".into());
    }
    Ok(())
}

fn version(value: u32) -> Result<(), String> {
    if value != SCHEMA_VERSION {
        return Err("Unsupported file version. Open it with a compatible Fluxkit version.".into());
    }
    Ok(())
}

impl Project {
    pub fn validate(&self) -> Result<(), String> {
        version(self.schema_version)?;
        validate_id(&self.id)?;
        validate_name(&self.name)?;
        validate_auth(self.auth.as_ref())
    }
}

impl Attachment {
    pub fn validate(&self) -> Result<(), String> {
        let id = self
            .path
            .strip_prefix("attachments/")
            .and_then(|s| s.strip_suffix(".bin"));
        validate_id(id.ok_or("Invalid attachment reference.")?)?;
        validate_name(&self.name)
    }
}

impl Collection {
    pub fn validate(&self) -> Result<(), String> {
        version(self.schema_version)?;
        validate_id(&self.id)?;
        validate_name(&self.name)?;
        validate_auth(self.auth.as_ref())?;
        if let Some(metadata) = &self.open_api {
            if metadata.version.len() > 16
                || !(metadata.version.starts_with("3.0.") || metadata.version.starts_with("3.1."))
                || metadata.version[4..].parse::<u32>().is_err()
                || metadata
                    .document
                    .get("openapi")
                    .and_then(serde_json::Value::as_str)
                    != Some(metadata.version.as_str())
                || !metadata.document.is_object()
                || serde_json::to_vec(&metadata.document)
                    .map_err(|_| "Invalid OpenAPI metadata.")?
                    .len()
                    > 4 * 1024 * 1024
            {
                return Err("Invalid or oversized OpenAPI metadata.".into());
            }
            for (id, pointer) in &metadata.operations {
                validate_id(id)?;
                let pointer = pointer
                    .strip_prefix('#')
                    .ok_or("Invalid imported operation reference.")?;
                if pointer.len() > 65_536
                    || !pointer.starts_with("/paths/")
                    || metadata.document.pointer(pointer).is_none()
                {
                    return Err("Invalid imported operation reference.".into());
                }
            }
        }
        if let Some(metadata) = &self.postman {
            if metadata.version != "2.1" || metadata.variables.len() > 1000 {
                return Err("Invalid Postman mapping metadata.".into());
            }
            for (name, id) in &metadata.variables {
                validate_name(name)?;
                validate_id(id)?;
            }
        }
        if self.folders.len() + self.requests.len() > 10_000 {
            return Err("A collection can contain at most 10,000 folders and requests.".into());
        }
        let mut ids = HashSet::new();
        for (id, name) in self
            .folders
            .iter()
            .map(|f| (&f.id, &f.name))
            .chain(self.requests.iter().map(|r| (&r.id, &r.name)))
        {
            validate_id(id)?;
            validate_name(name)?;
            if !ids.insert(id) {
                return Err("Duplicate entity identifiers.".into());
            }
        }
        for folder in &self.folders {
            validate_auth(folder.auth.as_ref())?;
            let mut parent = folder.parent_id.as_ref();
            let mut visited = HashSet::from([&folder.id]);
            while let Some(id) = parent {
                if !visited.insert(id) || visited.len() > 33 {
                    return Err("Folder hierarchy contains a cycle or exceeds 32 levels.".into());
                }
                parent = self
                    .folders
                    .iter()
                    .find(|f| &f.id == id)
                    .ok_or("Folder parent does not exist.")?
                    .parent_id
                    .as_ref();
            }
        }
        for request in &self.requests {
            if request
                .folder_id
                .as_ref()
                .is_some_and(|id| !self.folders.iter().any(|f| &f.id == id))
            {
                return Err("Request folder does not exist.".into());
            }
            request.validate()?;
        }
        Ok(())
    }
}

fn validate_entries(entries: &[Entry]) -> Result<(), String> {
    let mut ids = HashSet::new();
    for entry in entries {
        if !ids.insert(entry.id) {
            return Err("Duplicate parameter/header identifiers.".into());
        }
    }
    Ok(())
}

impl Request {
    /// Validate a draft independently of whether it has been saved to a collection.
    pub fn validate(&self) -> Result<(), String> {
        validate_id(&self.id)?;
        validate_name(&self.name)?;
        validate_auth(self.auth.as_ref())?;
        if self.import_issues.len() > 100
            || self
                .import_issues
                .iter()
                .any(|s| s.len() > 1000 || s.chars().any(char::is_control))
        {
            return Err("Invalid import review notes.".into());
        }
        if let Some(id) = &self.folder_id {
            validate_id(id)?;
        }
        if self.method.is_empty()
            || self.method.len() > 32
            || !self
                .method
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b == b'-')
        {
            return Err("Invalid HTTP method.".into());
        }
        if self.url.len() > 65_536 || self.url.chars().any(char::is_control) {
            return Err("URL is too long or contains control characters.".into());
        }
        validate_entries(&self.params)?;
        validate_entries(&self.headers)?;
        let mut field_ids = HashSet::new();
        for field in &self.body.fields {
            if !field_ids.insert(field.id) {
                return Err("Duplicate form field identifiers.".into());
            }
            if let Some(file) = &field.file {
                file.validate()?;
            }
        }
        if let Some(file) = &self.body.file {
            file.validate()?;
        }
        Ok(())
    }
}

/// Authentication definitions contain stable project-variable IDs, never credential values.
/// Missing configuration inherits; at project root it means no authentication.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Auth {
    None,
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    ApiKey {
        name: String,
        value: String,
        placement: KeyPlacement,
    },
    Oauth2 {
        config: Box<super::token_config::OAuth2>,
    },
    CustomLogin {
        config: Box<super::token_config::CustomLogin>,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum KeyPlacement {
    Header,
    Query,
}

pub fn validate_auth(auth: Option<&Auth>) -> Result<(), String> {
    match auth {
        Some(Auth::Basic { username, password }) => {
            validate_id(username)?;
            validate_id(password)
        }
        Some(Auth::Bearer { token }) => validate_id(token),
        Some(Auth::Oauth2 { config }) => config.validate(),
        Some(Auth::CustomLogin { config }) => config.validate(),
        Some(Auth::ApiKey {
            name,
            value,
            placement,
        }) => {
            validate_id(value)?;
            if name.is_empty() || name.len() > 200 || name.chars().any(char::is_control) {
                return Err(
                    "API key names must contain 1 to 200 bytes without control characters.".into(),
                );
            }
            if *placement == KeyPlacement::Header {
                let header = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| "Invalid API key header name.")?;
                if matches!(
                    header.as_str(),
                    "content-length"
                        | "transfer-encoding"
                        | "connection"
                        | "upgrade"
                        | "proxy-authorization"
                        | "proxy-connection"
                        | "te"
                        | "trailer"
                        | "host"
                        | "content-type"
                ) {
                    return Err(
                        "Choose an API key header that is not managed by HTTP transport.".into(),
                    );
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

impl Auth {
    pub fn variable_ids(&self) -> Vec<&str> {
        match self {
            Self::None => vec![],
            Self::Basic { username, password } => vec![username, password],
            Self::Bearer { token } => vec![token],
            Self::ApiKey { value, .. } => vec![value],
            Self::Oauth2 { config } => config.variable_ids(),
            Self::CustomLogin { config } => config.variable_ids(),
        }
    }
}

/// Sanitized imported schemas/descriptions; examples/default values and server values are excluded.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenApiMetadata {
    pub version: String,
    pub document: serde_json::Value,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub operations: std::collections::BTreeMap<String, String>,
}

/// Source variable names map to local identities for explicitly associated environment imports.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PostmanMetadata {
    pub version: String,
    pub variables: std::collections::BTreeMap<String, String>,
}
