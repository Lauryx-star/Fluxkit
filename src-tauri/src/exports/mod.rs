//! OpenAPI descriptions are built from saved files, with no credential or network adapter.
pub mod commands;
mod mapping;
mod storage;

use crate::projects::store::ProjectView;
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Target {
    Project,
    Collection { collection: String },
    Folder { collection: String, folder: String },
    Request { collection: String, request: String },
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub id: String,
    pub title: String,
    pub directory: bool,
    pub folder_name: String,
    pub files: Vec<FilePreview>,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePreview {
    pub name: String,
    pub operations: usize,
    pub bytes: usize,
}
pub struct Document {
    pub name: String,
    pub yaml: String,
}
pub struct Plan {
    pub preview: Preview,
    pub snapshot: ProjectView,
    pub definitions_revision: String,
    pub documents: Vec<Document>,
    pub created: Instant,
}

#[cfg(test)]
mod tests;
