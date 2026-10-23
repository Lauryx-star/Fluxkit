use super::model::ExecutionInput;
use crate::projects::{
    model::{validate_id, Auth, Collection, KeyPlacement},
    store,
};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION},
    Client, Url,
};
use std::path::Path;

/// Reload only the selected ancestry under the caller's project lock. No UI-supplied
/// inherited credentials are trusted, and stale configuration blocks dispatch.
pub(super) fn effective(
    input: &ExecutionInput,
    root: Option<&Path>,
) -> Result<Option<Auth>, String> {
    let mut inherited = None;
    if let Some(scope) = &input.scope {
        let root = root.ok_or("Open the authentication project's folder before sending.")?;
        let (project, bytes) = store::load_project(root)?;
        if store::revision(&bytes) != scope.project_revision {
            return Err(
                "Project authentication changed. Reload the project before sending.".into(),
            );
        }
        inherited = project.auth;
        if let Some(id) = &scope.collection_id {
            validate_id(id)?;
            store::safe(&root.join("collections"), true)?;
            let bytes = store::read(&root.join("collections").join(format!("{id}.json")))?;
            if scope.collection_revision.as_deref() != Some(&store::revision(&bytes)) {
                return Err(
                    "Collection authentication changed. Reload the project before sending.".into(),
                );
            }
            let collection: Collection = store::decode(&bytes)?;
            collection.validate()?;
            if collection.id != *id {
                return Err("Collection identity changed.".into());
            }
            if collection.auth.is_some() {
                inherited = collection.auth;
            }
            let mut ancestors = Vec::new();
            let mut parent = input.request.folder_id.as_deref();
            while let Some(id) = parent {
                if ancestors.len() >= 32 {
                    return Err("Invalid authentication hierarchy.".into());
                }
                let folder = collection
                    .folders
                    .iter()
                    .find(|f| f.id == id)
                    .ok_or("Request folder no longer exists. Reload the project.")?;
                ancestors.push(folder);
                parent = folder.parent_id.as_deref();
            }
            for folder in ancestors.into_iter().rev() {
                if folder.auth.is_some() {
                    inherited = folder.auth.clone();
                }
            }
        } else if input.request.folder_id.is_some() || scope.collection_revision.is_some() {
            return Err("A request folder requires its collection context.".into());
        }
    } else if input.request.folder_id.is_some() {
        return Err("Open this request's project before sending.".into());
    }
    Ok(input.request.auth.clone().or(inherited))
}

/// Manual fields win. Do not even read credentials for an overridden authentication field.
pub(super) fn apply(
    auth: Option<&Auth>,
    url: &mut Url,
    headers: &mut HeaderMap,
    client: &Client,
    value: impl Fn(&str) -> Result<String, String>,
) -> Result<(), String> {
    let header_value = |value: String| {
        HeaderValue::from_bytes(value.as_bytes())
            .map_err(|_| "Authentication contains invalid header characters.".to_string())
    };
    match auth {
        Some(Auth::Basic { username, password }) if !headers.contains_key(AUTHORIZATION) => {
            let username = value(username)?;
            if username.contains(':') {
                return Err("Basic authentication usernames cannot contain a colon.".into());
            }
            let password = value(password)?;
            let request = client
                .get("https://example.invalid")
                .basic_auth(username, Some(password))
                .build()
                .map_err(|_| "Could not encode Basic authentication.")?;
            let header = request
                .headers()
                .get(AUTHORIZATION)
                .ok_or("Could not encode Basic authentication.")?
                .clone();
            headers.insert(AUTHORIZATION, header);
        }
        Some(Auth::Bearer { token }) if !headers.contains_key(AUTHORIZATION) => {
            let token = value(token)?;
            if token.is_empty() || token.chars().any(char::is_whitespace) {
                return Err("Bearer tokens must be non-empty and contain no whitespace.".into());
            }
            headers.insert(AUTHORIZATION, header_value(format!("Bearer {token}"))?);
        }
        Some(Auth::ApiKey {
            name,
            value: id,
            placement: KeyPlacement::Header,
        }) => {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| "Invalid API key header name.")?;
            if !headers.contains_key(&name) {
                headers.insert(name, header_value(value(id)?)?);
            }
        }
        Some(Auth::ApiKey {
            name,
            value: id,
            placement: KeyPlacement::Query,
        }) => {
            if !url.query_pairs().any(|(key, _)| key == *name) {
                url.query_pairs_mut().append_pair(name, &value(id)?);
            }
        }
        _ => {}
    }
    Ok(())
}
