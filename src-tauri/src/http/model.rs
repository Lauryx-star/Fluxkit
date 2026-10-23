use crate::projects::model::Request;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scope {
    pub handle: String,
    pub environment: Option<String>,
    pub revision: String,
    pub project_revision: String,
    pub collection_id: Option<String>,
    pub collection_revision: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionInput {
    pub request: Request,
    pub scope: Option<Scope>,
    pub timeout_ms: u64,
    pub save_response: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Response,
    Cancelled,
    Timeout,
    NetworkError,
    InvalidRequest,
    SaveError,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ContentKind {
    Empty,
    Text,
    Json,
    Binary,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// Redacted dispatch metadata; never contains resolved user header values.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestHeader {
    pub name: String,
    pub value: String,
    pub source: &'static str,
}

/// Execution results are ephemeral UI data. Never log or persist this object.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionResult {
    pub outcome: Outcome,
    pub status: Option<u16>,
    pub elapsed_ms: Option<u64>,
    pub headers: Vec<Header>,
    pub byte_count: u64,
    pub text: Option<String>,
    pub json_text: Option<String>,
    pub content_kind: ContentKind,
    pub truncated: bool,
    pub saved: bool,
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_status: Option<super::tokens::Status>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_warning: Option<String>,
    pub request_headers: Vec<RequestHeader>,
}
impl ExecutionResult {
    pub fn failure(outcome: Outcome, message: &str) -> Self {
        Self {
            outcome,
            status: None,
            elapsed_ms: None,
            headers: vec![],
            byte_count: 0,
            text: None,
            json_text: None,
            content_kind: ContentKind::Empty,
            truncated: false,
            saved: false,
            message: Some(message.into()),
            token_status: None,
            history_warning: None,
            request_headers: vec![],
        }
    }
}
