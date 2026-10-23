use super::{
    model::{ExecutionInput, ExecutionResult, Outcome},
    prepare, tokens,
    transfer::{self, Download},
};
use crate::history::{self, Candidate, HistoryState};
use crate::projects::{
    commands::{run, ProjectState},
    environments::OsValues,
    model::validate_id,
};
use reqwest::Client;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{Manager, State};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct Slot {
    token: CancellationToken,
    active: bool,
    created: Instant,
}
#[derive(Default)]
pub struct HttpState {
    slots: Arc<Mutex<HashMap<String, Slot>>>,
    client: Arc<Mutex<Option<Client>>>,
    staged: Arc<Mutex<HashMap<String, PathBuf>>>,
    closing: AtomicBool,
    tokens: tokens::Manager,
}
impl HttpState {
    fn claim(&self, id: &str) -> Result<(CancellationToken, Guard), String> {
        validate_id(id)?;
        let mut slots = self
            .slots
            .lock()
            .map_err(|_| "HTTP request state is unavailable.")?;
        let slot = slots
            .get_mut(id)
            .ok_or("Request was cancelled or its reservation expired.")?;
        if slot.active {
            return Err("This request is already executing.".into());
        }
        slot.active = true;
        Ok((
            slot.token.clone(),
            Guard {
                id: id.into(),
                slots: self.slots.clone(),
                staged: self.staged.clone(),
            },
        ))
    }
    /// Give cancelled futures time to drop staged downloads before the process exits.
    pub fn shutdown(&self, app: &tauri::AppHandle, api: &tauri::ExitRequestApi, code: i32) {
        if self.closing.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Ok(mut slots) = self.slots.lock() {
            for slot in slots.values() {
                slot.token.cancel();
            }
            slots.retain(|_, slot| slot.active);
        }
        let slots = self.slots.clone();
        let staged = self.staged.clone();
        let empty =
            slots.lock().is_ok_and(|s| s.is_empty()) && staged.lock().is_ok_and(|s| s.is_empty());
        if empty {
            return;
        }
        api.prevent_exit();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if slots.lock().is_ok_and(|s| s.is_empty())
                    && staged.lock().is_ok_and(|s| s.is_empty())
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            // Only paths created for these explicit downloads are eligible for final cleanup.
            let _ = tauri::async_runtime::spawn_blocking(move || {
                let paths: Vec<_> = staged
                    .lock()
                    .map(|s| s.values().cloned().collect())
                    .unwrap_or_default();
                for path in paths {
                    let _ = std::fs::remove_file(path);
                }
            })
            .await;
            app.exit(code);
        });
    }
}
fn client(state: &Mutex<Option<Client>>) -> Result<Client, String> {
    let mut client = state
        .lock()
        .map_err(|_| "HTTP client state is unavailable.")?;
    if client.is_none() {
        *client = Some(
            Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .no_proxy()
                .referer(false)
                .pool_max_idle_per_host(2)
                .pool_idle_timeout(Duration::from_secs(30))
                .build()
                .map_err(|_| "Could not initialize the system TLS and HTTP client.")?,
        );
    }
    client
        .as_ref()
        .cloned()
        .ok_or("HTTP client is unavailable.".into())
}
struct Guard {
    id: String,
    slots: Arc<Mutex<HashMap<String, Slot>>>,
    staged: Arc<Mutex<HashMap<String, PathBuf>>>,
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(mut slots) = self.slots.lock() {
            slots.remove(&self.id);
        }
        if let Ok(mut staged) = self.staged.lock() {
            staged.remove(&self.id);
        }
    }
}

#[tauri::command]
pub fn begin_http_request(state: State<'_, HttpState>) -> Result<String, String> {
    if state.closing.load(Ordering::SeqCst) {
        return Err("Fluxkit is closing. No new requests can start.".into());
    }
    let mut slots = state
        .slots
        .lock()
        .map_err(|_| "HTTP request state is unavailable.")?;
    slots.retain(|_, slot| slot.active || slot.created.elapsed() < Duration::from_secs(600));
    if slots.len() >= 8 {
        return Err(
            "At most eight HTTP requests can be active at once. Cancel or finish another request."
                .into(),
        );
    }
    let id = Uuid::new_v4().to_string();
    slots.insert(
        id.clone(),
        Slot {
            token: CancellationToken::new(),
            active: false,
            created: Instant::now(),
        },
    );
    Ok(id)
}
#[tauri::command]
pub fn cancel_http_request(state: State<'_, HttpState>, id: String) -> Result<(), String> {
    validate_id(&id)?;
    let mut slots = state
        .slots
        .lock()
        .map_err(|_| "HTTP request state is unavailable.")?;
    if let Some(slot) = slots.remove(&id) {
        slot.token.cancel();
    }
    Ok(())
}
#[tauri::command]
pub async fn execute_http_request(
    app: tauri::AppHandle,
    projects: State<'_, ProjectState>,
    state: State<'_, HttpState>,
    history: State<'_, HistoryState>,
    id: String,
    input: ExecutionInput,
) -> Result<ExecutionResult, String> {
    let (cancel, guard) = state.claim(&id)?;
    let ticket = if let Some(scope) = &input.scope {
        let handle = scope.handle.clone();
        let method = if input.request.method.len() <= 32 {
            input.request.method.clone()
        } else {
            "INVALID".into()
        };
        let url = if input.request.url.len() <= 65_536 {
            input.request.url.clone()
        } else {
            "[URL redacted]".into()
        };
        let history_store = history.store();
        let registration = run(&projects, move |store| {
            store.with_environment(&handle, |_, project| {
                history_store?.begin(project, Candidate::new(&method, &url), history::now()?)
            })
        });
        tokio::select! { biased;
            _ = cancel.cancelled() => return Ok(ExecutionResult::failure(Outcome::Cancelled, "Cancelled before dispatch. No HTTP request was sent.")),
            result = registration => result,
        }
    } else {
        Ok(None)
    };
    let mut result = match execute(&app, &projects, &state, input, cancel, &guard).await {
        Ok(result) => result,
        Err(error) => ExecutionResult::failure(Outcome::InvalidRequest, &error),
    };
    match ticket {
        Ok(Some(ticket)) => {
            let store = history.store();
            let status = result.status;
            let elapsed = result.elapsed_ms;
            let saved = tauri::async_runtime::spawn_blocking(move || store?.finish(ticket, status, elapsed, history::now()?)).await;
            if !saved.is_ok_and(|value| value.is_ok()) {
                result.history_warning = Some("Request history could not be saved. Check local history storage; do not resend to repair history.".into());
            }
        }
        Err(_) => result.history_warning = Some("Request history could not be accessed. Check its storage or explicitly clear the project's history.".into()),
        Ok(None) => {}
    }
    Ok(result)
}

async fn execute(
    app: &tauri::AppHandle,
    projects: &State<'_, ProjectState>,
    state: &State<'_, HttpState>,
    input: ExecutionInput,
    cancel: CancellationToken,
    guard: &Guard,
) -> Result<ExecutionResult, String> {
    let settings = app
        .path()
        .app_data_dir()
        .map_err(|_| "Could not locate local settings.")?
        .join("environment-selection");
    let timeout = input.timeout_ms;
    let save = input.save_response;
    let handle = input.scope.as_ref().map(|scope| scope.handle.clone());
    let client_state = state.client.clone();
    let token = cancel.clone();
    let preparation = run(projects, move |store| {
        let client = client(&client_state)?;
        let prepared = if let Some(handle) = handle {
            store.with_environment(&handle, |root, project| {
                prepare::prepare(
                    &input,
                    Some(root),
                    project,
                    &settings,
                    &client,
                    &OsValues,
                    &token,
                )
            })?
        } else {
            prepare::prepare(
                &input, None, "session", &settings, &client, &OsValues, &token,
            )?
        };
        Ok((client, prepared))
    });
    let (client, mut prepared) = tokio::select! { biased;
        _ = cancel.cancelled() => return Ok(ExecutionResult::failure(Outcome::Cancelled, "Cancelled before dispatch. No HTTP request was sent.")),
        result = preparation => match result {
            Ok(prepared) => prepared,
            Err(error) => return Ok(ExecutionResult::failure(Outcome::InvalidRequest, &error)),
        }
    };
    let destination = if save {
        let selected = tokio::select! { biased;
            _ = cancel.cancelled() => return Ok(ExecutionResult::failure(Outcome::Cancelled, "Cancelled before dispatch. No HTTP request was sent.")),
            path = rfd::AsyncFileDialog::new().set_title("Send request and save its response to a file").set_file_name("response.bin").save_file() => path,
        };
        let Some(selected) = selected else {
            return Ok(ExecutionResult::failure(
                Outcome::Cancelled,
                "Save dialog cancelled. No HTTP request was sent.",
            ));
        };
        let path = selected.path().to_path_buf();
        match tauri::async_runtime::spawn_blocking(move || Download::new(path)).await {
            Ok(Ok(destination)) => Some(destination),
            _ => {
                return Ok(ExecutionResult::failure(
                    Outcome::SaveError,
                    "Could not prepare the destination file. No HTTP request was sent.",
                ))
            }
        }
    } else {
        None
    };
    if cancel.is_cancelled() {
        return Ok(ExecutionResult::failure(
            Outcome::Cancelled,
            "Cancelled before dispatch. No HTTP request was sent.",
        ));
    }
    if let Some(download) = &destination {
        state
            .staged
            .lock()
            .map_err(|_| "Download state is unavailable.")?
            .insert(guard.id.clone(), download.staged_path());
    }
    let mut token_status = None;
    if let Some(plan) = prepared.token.take() {
        let result = state
            .tokens
            .ensure(
                &plan,
                &client,
                Arc::new(OsValues),
                false,
                &cancel,
                Duration::from_millis(timeout),
            )
            .await;
        match result.and_then(|record| {
            plan.apply(&mut prepared.request, &record)?;
            token_status = Some(record.status_now()?);
            Ok(())
        }) {
            Ok(()) => {}
            Err(error) => {
                let outcome = if error == tokens::CANCELLED {
                    Outcome::Cancelled
                } else if error == tokens::TIMED_OUT {
                    Outcome::Timeout
                } else {
                    Outcome::InvalidRequest
                };
                return Ok(ExecutionResult::failure(outcome, &error));
            }
        }
    }
    let mut result = transfer::execute(
        &client,
        prepared,
        Duration::from_millis(timeout),
        &cancel,
        destination,
    )
    .await;
    result.token_status = token_status;
    Ok(result)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TokenAction {
    Status,
    Renew,
    Forget,
}

/// Return metadata only. Login responses and tokens never cross this boundary.
#[tauri::command]
pub async fn authentication_action(
    app: tauri::AppHandle,
    projects: State<'_, ProjectState>,
    state: State<'_, HttpState>,
    id: String,
    input: ExecutionInput,
    action: TokenAction,
) -> Result<tokens::Status, String> {
    let (cancel, _guard) = state.claim(&id)?;
    let timeout = Duration::from_millis(input.timeout_ms.clamp(1_000, 300_000));
    let settings = app
        .path()
        .app_data_dir()
        .map_err(|_| "Could not locate local settings.")?
        .join("environment-selection");
    let client_state = state.client.clone();
    let preparation = run(&projects, move |store| {
        let scope = input
            .scope
            .as_ref()
            .ok_or("Token acquisition requires an open project.")?;
        let plan = store.with_environment(&scope.handle, |root, project| {
            prepare::token_plan(&input, root, project, &settings)
        })?;
        Ok((client(&client_state)?, plan))
    });
    let (client, plan) = tokio::select! { biased;
        _ = cancel.cancelled() => return Err(tokens::CANCELLED.into()),
        result = preparation => result?,
    };
    let operation = async {
        match action {
            TokenAction::Status => state.tokens.status(&plan, Arc::new(OsValues)).await,
            TokenAction::Forget => state.tokens.forget(&plan, Arc::new(OsValues)).await,
            TokenAction::Renew => {
                state
                    .tokens
                    .ensure(&plan, &client, Arc::new(OsValues), true, &cancel, timeout)
                    .await?;
                state.tokens.status(&plan, Arc::new(OsValues)).await
            }
        }
    };
    tokio::select! { biased;
        _ = cancel.cancelled() => Err(tokens::CANCELLED.into()),
        _ = tokio::time::sleep(timeout) => Err(tokens::TIMED_OUT.into()),
        result = operation => result,
    }
}
