use super::Plan;
use crate::projects::{
    environments,
    store::{self, ProjectStore},
};
use std::path::{Path, PathBuf};

pub fn current(store: &ProjectStore, plan: &Plan) -> Result<(), String> {
    store.with_environment(&plan.snapshot.handle,|root,_| {
        let current=store::load(root,&plan.snapshot.handle)?;
        if current.revision!=plan.snapshot.revision || current.collections.iter().map(|c|(&c.data.id,&c.revision)).collect::<Vec<_>>() != plan.snapshot.collections.iter().map(|c|(&c.data.id,&c.revision)).collect::<Vec<_>>() || environments::definitions(root)?.1!=plan.definitions_revision {
            return Err("Project files changed after export preview. Close it, reload and preview again; no export was written.".into());
        }Ok(())
    })
}
/// Destinations come only from the native chooser. New files/directories never replace data.
pub fn write(plan: &Plan, destination: &Path) -> Result<(), String> {
    if !plan.preview.blockers.is_empty() {
        return Err("Resolve export blockers before saving.".into());
    }
    if plan.documents.is_empty() {
        return Err("No export documents are available.".into());
    }
    if plan.preview.directory {
        store::safe(destination, true)?;
        let directory = destination.join(&plan.preview.folder_name);
        std::fs::create_dir(&directory).map_err(|_| {
            "Could not create a new export folder. Nothing existing was overwritten."
        })?;
        for document in &plan.documents {
            if store::atomic(
                &directory.join(&document.name),
                document.yaml.as_bytes(),
                true,
            )
            .is_err()
            {
                return Err("Project export could not finish. The new fluxkit-export folder may contain completed files; inspect it and choose a new preview to retry. Existing files were not overwritten.".into());
            }
        }
        Ok(())
    } else {
        let parent = destination
            .parent()
            .ok_or("Choose a filename in an existing folder.")?;
        let parent = parent
            .canonicalize()
            .map_err(|_| "Could not open the selected export folder.")?;
        store::safe(&parent, true)?;
        let name = destination
            .file_name()
            .ok_or("Choose an export filename.")?;
        let path: PathBuf = parent.join(name);
        store::atomic(&path, plan.documents[0].yaml.as_bytes(), true)
    }
}
