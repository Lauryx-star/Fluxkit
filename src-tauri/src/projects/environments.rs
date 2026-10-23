//! Project definitions are portable; values and the environment selection are local.
use super::{
    model::{validate_id, validate_name},
    store,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    path::Path,
};

const FILE: &str = "environments.fluxkit.json";
const STORE_ERROR: &str = "The OS credential store could not be accessed. Unlock it, check access permissions, and retry. No plaintext fallback is used.";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Definition {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Definitions {
    pub schema_version: u32,
    pub environments: Vec<Definition>,
    pub variables: Vec<Definition>,
}
impl Default for Definitions {
    fn default() -> Self {
        Self {
            schema_version: 1,
            environments: vec![],
            variables: vec![],
        }
    }
}
impl Definitions {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("Unsupported environment file version.".into());
        }
        if self.environments.len() > 100 || self.variables.len() > 1000 {
            return Err("A project supports up to 100 environments and 1,000 variables.".into());
        }
        let mut ids = HashSet::new();
        for entries in [&self.environments, &self.variables] {
            let mut names = HashSet::new();
            for entry in entries {
                validate_id(&entry.id)?;
                validate_name(&entry.name)?;
                if !ids.insert(&entry.id) || !names.insert(&entry.name) {
                    return Err(
                        "Environment and variable identifiers and names must be unique.".into(),
                    );
                }
            }
        }
        for variable in &self.variables {
            if !valid_variable_name(&variable.name) {
                return Err("Variable names use letters, digits, underscores, dots, and hyphens; start with a letter or underscore.".into());
            }
        }
        Ok(())
    }
    fn slot(&self, variable: &str, environment: Option<&str>) -> Result<(), String> {
        if !self.variables.iter().any(|v| v.id == variable) {
            return Err("Variable no longer exists. Reload environments.".into());
        }
        self.environment(environment)
    }
    fn environment(&self, environment: Option<&str>) -> Result<(), String> {
        if environment.is_some_and(|id| !self.environments.iter().any(|e| e.id == id)) {
            return Err("Environment no longer exists. Reload environments.".into());
        }
        Ok(())
    }
}
fn valid_variable_name(name: &str) -> bool {
    name.bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub revision: String,
    pub data: Definitions,
    pub selected_environment: Option<String>,
    pub selection_missing: bool,
}

/// Only this adapter accesses platform credentials. Errors never include provider details or values.
pub trait Values {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}
pub struct OsValues;
impl OsValues {
    fn entry(key: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new("app.fluxkit.variables.v1", key).map_err(|_| STORE_ERROR.into())
    }
}
impl Values for OsValues {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        match Self::entry(key)?.get_secret() {
            Ok(value) => String::from_utf8(value)
                .map(Some)
                .map_err(|_| STORE_ERROR.into()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(STORE_ERROR.into()),
        }
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        Self::entry(key)?
            .set_secret(value.as_bytes())
            .map_err(|_| STORE_ERROR.into())
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(STORE_ERROR.into()),
        }
    }
}
/// The tuple is an implicit value reference, independent of display names and file paths.
fn key(project: &str, variable: &str, environment: Option<&str>) -> String {
    format!("{project}/{variable}/{}", environment.unwrap_or("default"))
}
fn definitions(root: &Path) -> Result<(Definitions, String), String> {
    let path = root.join(FILE);
    if path
        .symlink_metadata()
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok((Definitions::default(), "missing".into()));
    }
    let bytes = store::read(&path)?;
    let data: Definitions = store::decode(&bytes)?;
    data.validate()?;
    Ok((data, store::revision(&bytes)))
}
fn check_revision(actual: &str, expected: &str) -> Result<(), String> {
    if actual != expected {
        return Err(
            "Environments changed outside Fluxkit. Reload environments before saving.".into(),
        );
    }
    Ok(())
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    schema_version: u32,
    environment_id: Option<String>,
}
fn selected(settings: &Path, project: &str) -> Result<Option<String>, String> {
    let path = settings.join(format!("{project}.json"));
    if path
        .symlink_metadata()
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(None);
    }
    let selection: Selection = store::decode(&store::read(&path)?)?;
    if selection.schema_version != 1 {
        return Err("Unsupported environment settings version.".into());
    }
    if let Some(id) = &selection.environment_id {
        validate_id(id)?;
    }
    Ok(selection.environment_id)
}
fn write_selection(
    settings: &Path,
    project: &str,
    environment: Option<&str>,
) -> Result<(), String> {
    std::fs::create_dir_all(settings)
        .map_err(|_| "Could not create local environment settings.")?;
    store::safe(settings, true)?;
    let value = Selection {
        schema_version: 1,
        environment_id: environment.map(String::from),
    };
    store::atomic(
        &settings.join(format!("{project}.json")),
        &store::encode(&value)?,
        false,
    )
}
pub fn load(root: &Path, settings: &Path, project: &str) -> Result<View, String> {
    let (data, revision) = definitions(root)?;
    let saved = selected(settings, project)?;
    let selection_missing = data.environment(saved.as_deref()).is_err();
    Ok(View {
        revision,
        data,
        selected_environment: if selection_missing { None } else { saved },
        selection_missing,
    })
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Action {
    Load,
    Save {
        data: Definitions,
        expected: String,
    },
    Select {
        environment: Option<String>,
    },
    SetValue {
        variable: String,
        environment: Option<String>,
        value: Option<String>,
        expected: String,
    },
    Reveal {
        variable: String,
        environment: Option<String>,
    },
    Check {
        text: String,
    },
    Preview {
        variable: String,
        environment: Option<String>,
        expected: String,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub view: View,
    pub value: Option<String>,
}

/// Caller holds the project lock across file and credential operations.
/// Deletion is idempotent but not transactional: definitions stay if any deletion fails.
pub fn apply(
    root: &Path,
    settings: &Path,
    project: &str,
    action: Action,
    values: &impl Values,
) -> Result<Reply, String> {
    let (current, revision) = definitions(root)?;
    let mut revealed = None;
    match action {
        Action::Load => {}
        Action::Select { environment } => {
            current.environment(environment.as_deref())?;
            write_selection(settings, project, environment.as_deref())?;
        }
        Action::Save { data, expected } => {
            data.validate()?;
            check_revision(&revision, &expected)?;
            let removing = current
                .variables
                .iter()
                .any(|v| !data.variables.iter().any(|n| n.id == v.id))
                || current
                    .environments
                    .iter()
                    .any(|e| !data.environments.iter().any(|n| n.id == e.id));
            let removed_variables: Vec<_> = current
                .variables
                .iter()
                .filter(|v| !data.variables.iter().any(|n| n.id == v.id))
                .map(|v| v.id.as_str())
                .collect();
            store::check_auth_removal(root, &removed_variables)?;
            // Validate serialization before any irreversible credential deletion.
            let bytes = store::encode(&data)?;
            // Delete only tuples owned by removed definitions. Retained variables/defaults survive.
            for variable in &current.variables {
                let removed_variable = !data.variables.iter().any(|v| v.id == variable.id);
                for environment in std::iter::once(None)
                    .chain(current.environments.iter().map(|e| Some(e.id.as_str())))
                {
                    let removed_environment =
                        environment.is_some_and(|id| !data.environments.iter().any(|e| e.id == id));
                    if removed_variable || removed_environment {
                        values.delete(&key(project, &variable.id, environment)).map_err(|_| "Could not finish deleting OS values. Definitions were kept; some values may already be removed. Unlock the credential store and retry the removal.".to_string())?;
                    }
                }
            }
            let write_result = (|| {
                if selected(settings, project)?
                    .as_deref()
                    .is_some_and(|id| !data.environments.iter().any(|e| e.id == id))
                {
                    write_selection(settings, project, None)?;
                }
                let path = root.join(FILE);
                if revision == "missing" {
                    store::atomic(&path, &bytes, true)
                } else {
                    store::replace(root, &path, &expected, &bytes, "environments.json")
                }
            })();
            if removing {
                write_result.map_err(|_| "Definitions could not be saved after OS value deletion. Associated values may already be removed. Reload environments and retry the removal; deleted values cannot be restored from backups.".to_string())?;
            } else {
                write_result?;
            }
        }
        Action::SetValue {
            variable,
            environment,
            value,
            expected,
        } => {
            check_revision(&revision, &expected)?;
            current.slot(&variable, environment.as_deref())?;
            let reference = key(project, &variable, environment.as_deref());
            if let Some(value) = value {
                if value.len() > 2560 {
                    return Err(
                        "Variable values must not exceed 2,560 UTF-8 bytes (Windows credential limit)."
                            .into(),
                    );
                }
                values.set(&reference, &value)?;
            } else {
                values.delete(&reference)?;
            }
        }
        Action::Reveal {
            variable,
            environment,
        } => {
            current.slot(&variable, environment.as_deref())?;
            revealed = values.get(&key(project, &variable, environment.as_deref()))?;
        }
        Action::Check { text } => {
            if text.len() > 1024 * 1024 {
                return Err("Variable check input exceeds 1 MiB.".into());
            }
            let selection = load(root, settings, project)?;
            if selection.selection_missing {
                return Err("The selected environment was removed. Select an environment explicitly before resolving variables.".into());
            }
            resolve(
                &text,
                project,
                &current,
                selection.selected_environment.as_deref(),
                values,
            )?;
        }
        Action::Preview {
            variable,
            environment,
            expected,
        } => {
            check_revision(&revision, &expected)?;
            current.slot(&variable, environment.as_deref())?;
            let selection = load(root, settings, project)?;
            if selection.selection_missing || selection.selected_environment != environment {
                return Err(
                    "The project environment changed. Reload variables before previewing values."
                        .into(),
                );
            }
            let definition = current
                .variables
                .iter()
                .find(|v| v.id == variable)
                .ok_or("Variable no longer exists. Reload variables.")?;
            revealed = Some(resolve(
                &format!("{{{{{}}}}}", definition.name),
                project,
                &current,
                environment.as_deref(),
                values,
            )?);
        }
    }
    Ok(Reply {
        view: load(root, settings, project)?,
        value: revealed,
    })
}

/// Single-pass substitution: values are literal, never recursively expanded or returned to the UI.
/// An inaccessible environment value is an error, not permission to use the default.
pub fn resolve(
    text: &str,
    project: &str,
    definitions: &Definitions,
    environment: Option<&str>,
    values: &impl Values,
) -> Result<String, String> {
    definitions.environment(environment)?;
    let mut remaining = text;
    let mut output = String::new();
    let mut cache: HashMap<&str, String> = HashMap::new();
    while let Some(start) = remaining.find("{{") {
        output.push_str(&remaining[..start]);
        let rest = &remaining[start + 2..];
        let end = rest.find("}}").ok_or("Unclosed variable placeholder.")?;
        let name = &rest[..end];
        if !valid_variable_name(name) {
            return Err("Invalid variable placeholder. Use {{variableName}}.".into());
        }
        let value = match cache.entry(name) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let variable = definitions
                    .variables
                    .iter()
                    .find(|v| v.name == name)
                    .ok_or("A referenced variable is not defined in this project.")?;
                let override_value = if let Some(environment) = environment {
                    values.get(&key(project, &variable.id, Some(environment)))?
                } else {
                    None
                };
                let value = match override_value {
                    Some(value) => value,
                    None => values.get(&key(project, &variable.id, None))?.ok_or("A referenced variable has no value in this environment or in defaults. Open project variables to set it.")?,
                };
                entry.insert(value)
            }
        };
        output.push_str(value);
        if output.len() > 8 * 1024 * 1024 {
            return Err("Resolved input exceeds 8 MiB.".into());
        }
        remaining = &rest[end + 2..];
    }
    output.push_str(remaining);
    if output.len() > 8 * 1024 * 1024 {
        return Err("Resolved input exceeds 8 MiB.".into());
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
