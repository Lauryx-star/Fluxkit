use super::{CommitResult, Plan};
use crate::projects::{
    environments::{self, Values},
    store::{self, ProjectStore},
};
use std::path::Path;

pub fn commit(
    store: &ProjectStore,
    plan: &mut Plan,
    store_values: bool,
) -> Result<CommitResult, String> {
    let handle = plan.handle.clone();
    store.ensure_writable(&handle)?;
    store.with_environment(&handle, |root, project| {
        let result = apply(root, project, plan, store_values, &environments::OsValues);
        Ok(CommitResult {
            project: store::load(root, &handle)?,
            error: result.err(),
        })
    })
}
/// Definitions are committed first, so every attempted OS entry remains discoverable even
/// after a crash. Only fresh import-owned IDs are written. A retry resumes this exact plan.
pub(super) fn apply(
    root: &Path,
    project: &str,
    plan: &mut Plan,
    store_values: bool,
    values: &impl Values,
) -> Result<(), String> {
    if !plan.preview.blockers.is_empty() {
        return Err(
            "Resolve the blocking compatibility issues in the source and preview it again.".into(),
        );
    }
    let current = store::load(root, &plan.handle)?;
    if current.revision != plan.project_revision
        || current
            .collections
            .iter()
            .map(|c| (c.data.id.clone(), c.revision.clone()))
            .collect::<Vec<_>>()
            != plan.collection_revisions
    {
        return Err("Project files changed after preview. Cancel, reload the project and preview again. Existing data was not overwritten.".into());
    }
    let (_, revision) = environments::definitions(root)?;
    if revision != plan.environment_revision {
        return Err("Project variables changed after preview. Cancel, reload and preview again. Existing values were not overwritten.".into());
    }
    plan.collection.validate()?;
    plan.definitions.validate()?;
    let collection = store::encode(&plan.collection)?;
    let definitions = store::encode(&plan.definitions)?;
    if !plan.environment_only
        && (current.collections.len() >= 1000
            || collection.len()
                + current
                    .collections
                    .iter()
                    .map(|c| store::encode(&c.data).map(|b| b.len()))
                    .collect::<Result<Vec<_>, _>>()?
                    .iter()
                    .sum::<usize>()
                > 32 * 1024 * 1024)
    {
        return Err(
            "The imported collection exceeds the project's collection count or storage limit."
                .into(),
        );
    }
    if !plan.definitions_saved {
        let path = root.join("environments.fluxkit.json");
        if revision == "missing" {
            store::atomic(&path, &definitions, true)?;
        } else {
            store::replace(root, &path, &revision, &definitions, "environments.json")?;
        }
        plan.environment_revision = store::revision(&definitions);
        plan.definitions_saved = true;
    }
    if store_values {
        for variable in &plan.variables {
            if let Some(value) = &variable.value {
                let key = format!(
                    "{project}/{}/{}",
                    variable.definition.id,
                    variable.environment.as_deref().unwrap_or("default")
                );
                match values.get(&key) {
                    Ok(Some(existing)) if existing==*value=>{},
                    Ok(Some(_))=>return Err("An imported variable value was edited locally. It was not overwritten. Cancel and inspect Project variables before importing again.".into()),
                    Ok(None)=>values.set(&key,value).map_err(|_|"Could not store all imported values. Imported variable definitions and any values already saved remain visible in Project variables. Unlock the OS store and retry this import, or close this preview and inspect Project variables and environments. The import is incomplete; definitions may already exist.".to_string())?,
                    Err(_)=>return Err("Could not access the OS store. Imported variable definitions and any already saved values remain visible in Project variables. Unlock it and retry, or cancel and inspect the added variables. The import is incomplete; definitions may already exist.".into()),
                }
            }
        }
    }
    let latest = store::load(root, &plan.handle)?;
    if latest.revision != plan.project_revision
        || latest
            .collections
            .iter()
            .map(|c| (c.data.id.clone(), c.revision.clone()))
            .collect::<Vec<_>>()
            != plan.collection_revisions
        || environments::definitions(root)?.1 != plan.environment_revision
    {
        return Err("Project files changed during import. Added definitions/values may already exist; inspect Project variables and preview again. No existing collection was overwritten.".into());
    }
    if plan.environment_only {
        return Ok(());
    }
    let path = root
        .join("collections")
        .join(format!("{}.json", plan.collection.id));
    store::atomic(&path,&collection,true).map_err(|_|"Could not create the imported collection. Added variable definitions and any saved OS values remain available. Fix file access and retry this preview; no existing collection was overwritten.".into())
}
