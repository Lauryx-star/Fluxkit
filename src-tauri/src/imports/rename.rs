//! Preview-only naming changes: no filesystem or credential access.
use super::{Plan, Preview, VariablePreview};
use crate::projects::{
    environments::Definition,
    model::{Auth, Collection},
    token_config::Part,
};
use uuid::Uuid;

pub fn variable(plan: &mut Plan, old_name: &str, name: &str) -> Result<Preview, String> {
    if plan.definitions_saved || plan.environment_only {
        return Err(
            "Variable names can only be changed before starting a collection import.".into(),
        );
    }
    if old_name.len() > 200 || name.len() > 200 {
        return Err("Variable names must not exceed 200 characters.".into());
    }
    let index = plan
        .variables
        .iter()
        .position(|v| v.definition.name == old_name)
        .ok_or("This variable is not part of the import preview.")?;
    if old_name == name {
        return Ok(plan.preview.clone());
    }
    let postman = plan.preview.source == "Postman";
    let old_id = plan.variables[index].definition.id.clone();
    let reused = plan.variables[index].reused;
    let mut definitions = plan.definitions.clone();
    let mut variables: Vec<(Definition, bool)> = plan
        .variables
        .iter()
        .map(|v| (v.definition.clone(), v.reused))
        .collect();
    let target = definitions
        .variables
        .iter()
        .find(|v| v.name == name)
        .cloned();
    let new_id = match target {
        // Postman references bind to an existing project variable by name; its values stay as
        // they are. OpenAPI keeps its strict new-identity policy.
        Some(target)
            if postman
                && !plan
                    .variables
                    .iter()
                    .any(|v| v.definition.id == target.id && !v.reused) =>
        {
            if !reused {
                definitions.variables.retain(|v| v.id != old_id);
            }
            if variables
                .iter()
                .any(|(definition, _)| definition.id == target.id)
            {
                variables.remove(index);
            } else {
                variables[index] = (target.clone(), true);
            }
            target.id
        }
        Some(_) if postman => {
            return Err("This name is already used by another variable in this import.".into())
        }
        Some(_) => return Err("Variable names must remain unique across the project.".into()),
        // Renaming a reused variable detaches it into a new import-owned variable.
        None if reused => {
            let definition = Definition {
                id: Uuid::new_v4().to_string(),
                name: name.into(),
                secret: plan.variables[index].definition.secret,
            };
            definitions.variables.push(definition.clone());
            variables[index] = (definition.clone(), false);
            definition.id
        }
        None => {
            let definition = definitions
                .variables
                .iter_mut()
                .find(|v| v.id == old_id)
                .ok_or("The import definition is unavailable. Preview the source again.")?;
            definition.name = name.into();
            variables[index].0.name = name.into();
            old_id.clone()
        }
    };
    definitions.validate()?;
    let before = format!("{{{{{old_name}}}}}");
    let after = format!("{{{{{name}}}}}");
    // Touch executable template fields only. Preserved source metadata keeps its content; UUID
    // auth references move only when the variable identity changes.
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
    if new_id != old_id {
        replace_id(&mut collection, &old_id, &new_id);
    }
    collection.validate()?;
    crate::projects::store::encode(&collection)?;
    // Everything validated: apply the new plan state.
    let source_name = plan.preview.variables[index].source_name.clone();
    let mut imported = std::mem::take(&mut plan.variables);
    let mut previews = std::mem::take(&mut plan.preview.variables);
    if variables.len() < imported.len() {
        imported.remove(index);
        previews.remove(index);
    } else {
        let (definition, reused) = variables[index].clone();
        let variable = &mut imported[index];
        variable.definition = definition;
        variable.reused = reused;
        previews[index] = VariablePreview {
            source_name,
            name: name.into(),
            has_default: variable.value.is_some() && !reused,
            existing: reused,
            secret: variable.definition.secret.unwrap_or(true),
        };
    }
    plan.variables = imported;
    plan.preview.variables = previews;
    plan.collection = collection;
    plan.definitions = definitions;
    for request in &mut plan.preview.requests {
        request.url = request.url.replace(&before, &after);
    }
    Ok(plan.preview.clone())
}

/// Moves auth and Postman source-name references to another variable identity.
fn replace_id(collection: &mut Collection, old: &str, new: &str) {
    let swap = |id: &mut String| {
        if id == old {
            *id = new.into();
        }
    };
    let auths = std::iter::once(&mut collection.auth)
        .chain(collection.folders.iter_mut().map(|f| &mut f.auth))
        .chain(collection.requests.iter_mut().map(|r| &mut r.auth));
    for auth in auths.flatten() {
        match auth {
            Auth::Basic { username, password } => {
                swap(username);
                swap(password);
            }
            Auth::Bearer { token } => swap(token),
            Auth::ApiKey { value, .. } => swap(value),
            Auth::Oauth2 { config } => {
                swap(&mut config.client_id);
                swap(&mut config.client_secret);
                for part in config
                    .token_url
                    .parts
                    .iter_mut()
                    .chain(config.scope.parts.iter_mut())
                {
                    if let Part::Variable { id } = part {
                        swap(id);
                    }
                }
            }
            // Imports never generate Custom Login or None.
            Auth::CustomLogin { .. } | Auth::None => {}
        }
    }
    if let Some(postman) = &mut collection.postman {
        postman.variables.values_mut().for_each(swap);
    }
}
