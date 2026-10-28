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
    /// The import binds to this already existing project variable instead of creating one.
    pub existing: bool,
    /// Secret values go to the personal vault in server workspaces, never to shared data.
    pub secret: bool,
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
    /// An existing project variable matched by source name. Its default value is never written.
    pub reused: bool,
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
    /// The collection/definitions are saved in the workspace; a retry continues after it.
    pub local_saved: bool,
    /// Server workspaces only: the saved workspace reached the server.
    pub synced: bool,
    pub created: Instant,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitResult {
    pub project: ProjectView,
    pub error: Option<String>,
}

impl Plan {
    /// Value slots this import may write: new import-owned defaults and new environment slots.
    /// Reused project variables keep their existing default.
    pub fn value_slots(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.variables
            .iter()
            .filter(|v| !v.reused || v.environment.is_some())
            .filter_map(|v| {
                Some((
                    v.definition.id.as_str(),
                    v.environment.as_deref().unwrap_or("default"),
                    v.value.as_deref()?,
                ))
            })
    }
    /// Secret classification as saved in the plan's definitions (missing means secret).
    pub fn is_secret(&self, id: &str) -> bool {
        self.definitions
            .variables
            .iter()
            .find(|v| v.id == id)
            .is_none_or(|v| v.secret.unwrap_or(true))
    }
}
