//! Preview-only naming changes: no filesystem or credential access.
use super::{Plan, Preview};

pub fn variable(plan: &mut Plan, old_name: &str, name: &str) -> Result<Preview, String> {
    if plan.preview.source != "OpenAPI" || plan.definitions_saved || plan.environment_only {
        return Err("Variable names can only be changed before starting an OpenAPI import.".into());
    }
    if old_name.len() > 200 || name.len() > 200 {
        return Err("Variable names must not exceed 200 characters.".into());
    }
    let index = plan
        .variables
        .iter()
        .position(|v| v.definition.name == old_name)
        .ok_or("This variable is not part of the import preview.")?;
    let id = &plan.variables[index].definition.id;
    let mut definitions = plan.definitions.clone();
    let definition = definitions
        .variables
        .iter_mut()
        .find(|v| &v.id == id)
        .ok_or("The import definition is unavailable. Preview the source again.")?;
    definition.name = name.into();
    definitions.validate()?;
    let before = format!("{{{{{old_name}}}}}");
    let after = format!("{{{{{name}}}}}");
    // Touch executable template fields only. Preserved source metadata and UUID auth references
    // retain their identity; no source examples or resolved values are exposed to the frontend.
    let mut collection = plan.collection.clone();
    let mut size = crate::projects::store::encode(&collection)?.len();
    let mut rewrite = |text: &mut String| -> Result<(), String> {
        if after.len() > before.len() {
            size = size
                .checked_add(
                    text.matches(&before)
                        .count()
                        .saturating_mul(after.len() - before.len()),
                )
                .filter(|size| *size <= 8 * 1024 * 1024)
                .ok_or("The renamed collection would exceed the 8 MiB limit.")?;
        }
        *text = text.replace(&before, &after);
        Ok(())
    };
    for request in &mut collection.requests {
        rewrite(&mut request.url)?;
        for entry in request.params.iter_mut().chain(request.headers.iter_mut()) {
            rewrite(&mut entry.key)?;
            rewrite(&mut entry.value)?;
        }
        rewrite(&mut request.body.text)?;
        for field in &mut request.body.fields {
            rewrite(&mut field.key)?;
            rewrite(&mut field.value)?;
        }
    }
    collection.validate()?;
    crate::projects::store::encode(&collection)?;
    plan.collection = collection;
    plan.variables[index].definition.name = name.into();
    plan.definitions = definitions;
    plan.preview.variables[index].name = name.into();
    for request in &mut plan.preview.requests {
        request.url = request.url.replace(&before, &after);
    }
    Ok(plan.preview.clone())
}
