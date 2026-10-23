//! Portable token-flow definitions. Expressions hold variable identities, never stored values.
use super::model::{validate_auth, validate_id, Auth, KeyPlacement};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Composition {
    pub parts: Vec<Part>,
    pub separator: String,
    pub prefix: String,
    pub suffix: String,
    pub base64: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Part {
    Literal { text: String },
    Variable { id: String },
    AccessToken,
    RenewalToken,
}
impl Composition {
    fn validate(&self, renewal: bool) -> Result<(), String> {
        if self.parts.len() > 100
            || [&self.separator, &self.prefix, &self.suffix]
                .iter()
                .any(|s| s.len() > 4096)
        {
            return Err("Token input composition exceeds its limits.".into());
        }
        for part in &self.parts {
            match part {
                Part::Variable { id } => validate_id(id)?,
                Part::Literal { text } if text.len() > 65_536 => {
                    return Err("A token input literal exceeds 64 KiB.".into());
                }
                Part::AccessToken | Part::RenewalToken if !renewal => {
                    return Err(
                        "Stored token inputs are only available to a renewal request.".into(),
                    );
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn ids(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().filter_map(|p| match p {
            Part::Variable { id } => Some(id.as_str()),
            _ => None,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OAuth2 {
    pub token_url: Composition,
    pub client_id: String,
    pub client_secret: String,
    pub scope: Composition,
    pub client_authentication: ClientAuthentication,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ClientAuthentication {
    Basic,
    Body,
}
impl OAuth2 {
    pub fn validate(&self) -> Result<(), String> {
        self.token_url.validate(false)?;
        self.scope.validate(false)?;
        validate_id(&self.client_id)?;
        validate_id(&self.client_secret)
    }
    pub fn variable_ids(&self) -> Vec<&str> {
        self.token_url
            .ids()
            .chain(self.scope.ids())
            .chain([self.client_id.as_str(), self.client_secret.as_str()])
            .collect()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoginEntry {
    pub name: String,
    pub value: Composition,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum LoginBody {
    None,
    Text { value: Composition },
    Json { value: Composition },
    UrlEncoded { fields: Vec<LoginEntry> },
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Extraction {
    Text,
    Json { pointer: String },
    Header { name: String },
}
impl Extraction {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Json { pointer } => {
                if pointer.len() > 1024 || (!pointer.is_empty() && !pointer.starts_with('/')) {
                    return Err("Use a JSON Pointer such as /access_token for extraction.".into());
                }
                let bytes = pointer.as_bytes();
                for (i, b) in bytes.iter().enumerate() {
                    if *b == b'~' && !matches!(bytes.get(i + 1), Some(b'0' | b'1')) {
                        return Err("JSON Pointer escapes must be ~0 or ~1.".into());
                    }
                }
            }
            Self::Header { name } => {
                reqwest::header::HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| "Invalid extraction header name.")?;
            }
            Self::Text => {}
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LoginRequest {
    pub url: Composition,
    pub method: String,
    pub params: Vec<LoginEntry>,
    pub headers: Vec<LoginEntry>,
    pub body: LoginBody,
    pub success_statuses: Vec<u16>,
    pub token: Extraction,
    pub renewal_token: Option<Extraction>,
    /// Extract a positive integer lifetime in seconds, not an absolute time or JWT claim.
    pub expires_in: Option<Extraction>,
    pub lifetime_seconds: Option<u64>,
}
impl LoginRequest {
    fn expressions(&self) -> Vec<&Composition> {
        let mut values = vec![&self.url];
        values.extend(self.params.iter().chain(&self.headers).map(|v| &v.value));
        match &self.body {
            LoginBody::None => {}
            LoginBody::Text { value } | LoginBody::Json { value } => values.push(value),
            LoginBody::UrlEncoded { fields } => values.extend(fields.iter().map(|v| &v.value)),
        }
        values
    }
    fn validate(&self, renewal: bool) -> Result<(), String> {
        if self.method.is_empty()
            || self.method.len() > 32
            || !self
                .method
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b == b'-')
            || self.method == "CONNECT"
        {
            return Err("Invalid login HTTP method.".into());
        }
        if self.success_statuses.is_empty()
            || self.success_statuses.len() > 100
            || self
                .success_statuses
                .iter()
                .any(|s| !(200..=599).contains(s))
        {
            return Err(
                "Select one or more accepted HTTP status codes between 200 and 599.".into(),
            );
        }
        if self.lifetime_seconds.is_some() && self.expires_in.is_some() {
            return Err("Choose extracted expiry or a configured lifetime, not both.".into());
        }
        if self
            .lifetime_seconds
            .is_some_and(|s| s == 0 || s > 315_360_000)
        {
            return Err("Token lifetime must be between 1 second and ten years.".into());
        }
        let fields = match &self.body {
            LoginBody::UrlEncoded { fields } => fields.as_slice(),
            _ => &[],
        };
        if self.params.len() + self.headers.len() + fields.len() > 200 {
            return Err(
                "A login request supports at most 200 query, header, and form entries.".into(),
            );
        }
        for entry in self.params.iter().chain(&self.headers).chain(fields) {
            if entry.name.is_empty()
                || entry.name.len() > 200
                || entry.name.chars().any(char::is_control)
            {
                return Err(
                    "Login field names must contain 1 to 200 bytes without control characters."
                        .into(),
                );
            }
        }
        for expression in self.expressions() {
            expression.validate(renewal)?;
        }
        self.token.validate()?;
        for extraction in [&self.renewal_token, &self.expires_in]
            .into_iter()
            .flatten()
        {
            extraction.validate()?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenPlacement {
    pub name: String,
    pub placement: KeyPlacement,
    pub prefix: String,
}
impl TokenPlacement {
    pub fn validate(&self) -> Result<(), String> {
        validate_auth(Some(&Auth::ApiKey {
            name: self.name.clone(),
            value: "00000000-0000-0000-0000-000000000000".into(),
            placement: self.placement.clone(),
        }))?;
        if self.prefix.len() > 200 || self.prefix.chars().any(char::is_control) {
            return Err("Token prefix exceeds 200 bytes or contains control characters.".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomLogin {
    pub login: LoginRequest,
    pub renewal: Option<LoginRequest>,
    pub placement: TokenPlacement,
}
impl CustomLogin {
    pub fn validate(&self) -> Result<(), String> {
        self.login.validate(false)?;
        if let Some(renewal) = &self.renewal {
            renewal.validate(true)?;
        }
        self.placement.validate()
    }
    pub fn variable_ids(&self) -> Vec<&str> {
        self.login
            .expressions()
            .into_iter()
            .chain(self.renewal.iter().flat_map(LoginRequest::expressions))
            .flat_map(Composition::ids)
            .collect()
    }
}
