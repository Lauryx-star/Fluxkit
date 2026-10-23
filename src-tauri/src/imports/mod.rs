pub mod commands;
pub(crate) mod document;
mod mapping;
mod postman;
#[cfg(test)]
mod postman_tests;
mod rename;
mod storage;
#[cfg(test)]
mod tests;
use crate::projects::{
    environments::{Definition, Definitions},
    model::Collection,
    store::ProjectView,
};
use serde::Serialize;
use std::time::Instant;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub id: String,
    pub source: String,
    pub environment_name: Option<String>,
    pub collection_name: String,
    pub version: String,
    pub request_count: usize,
    pub folder_count: usize,
    pub review_count: usize,
    pub variables: Vec<VariablePreview>,
    pub requests: Vec<RequestPreview>,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariablePreview {
    pub source_name: Option<String>,
    pub name: String,
    pub has_default: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPreview {
    pub needs_review: bool,
    pub name: String,
    pub method: String,
    pub url: String,
}
/// Values are intentionally neither serializable nor Debug-printable.
pub struct ImportedVariable {
    pub definition: Definition,
    pub value: Option<String>,
    pub environment: Option<String>,
}
pub struct Plan {
    pub preview: Preview,
    pub handle: String,
    pub project_revision: String,
    pub collection_revisions: Vec<(String, String)>,
    pub environment_revision: String,
    pub collection: Collection,
    pub environment_only: bool,
    pub variables: Vec<ImportedVariable>,
    pub definitions: Definitions,
    pub definitions_saved: bool,
    pub created: Instant,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitResult {
    pub project: ProjectView,
    pub error: Option<String>,
}
