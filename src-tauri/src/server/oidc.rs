//! External-browser OIDC handoff. Provider credentials and tokens stay on the server.
use super::*;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub struct OidcState(Mutex<Option<CancellationToken>>);
impl OidcState {
    pub fn cancel(&self) {
        if let Ok(mut pending) = self.0.lock() {
            if let Some(token) = pending.take() {
                token.cancel();
            }
        }
    }
    pub(super) fn commit<T>(
        &self,
        token: &CancellationToken,
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _pending = self.0.lock().map_err(|_| "Sign-in state is unavailable.")?;
        if token.is_cancelled() {
            return Err("Browser sign-in cancelled.".into());
        }
        action()
    }
    fn begin(&self) -> Result<CancellationToken, String> {
        let mut pending = self.0.lock().map_err(|_| "Sign-in state is unavailable.")?;
        if pending.as_ref().is_some_and(|token| !token.is_cancelled()) {
            return Err("Another browser sign-in is running. Cancel it first.".into());
        }
        let token = CancellationToken::new();
        *pending = Some(token.clone());
        Ok(token)
    }
}
struct Pending(CancellationToken);
impl Drop for Pending {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| "Could not create a secure server connection.".into())
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthMode {
    mode: String,
}
#[tauri::command]
pub async fn server_auth_mode(app: tauri::AppHandle) -> Result<AuthMode, String> {
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    let response = client()?
        .get(api_url(&current.server_url, "/api/auth/mode")?)
        .send()
        .await
        .map_err(|_| "Could not read the server authentication mode.".to_string())?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    let mode: AuthMode = values::read_json(response).await?;
    if !matches!(mode.mode.as_str(), "local" | "oidc") {
        return Err("The server uses an unsupported authentication mode.".into());
    }
    Ok(mode)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Start {
    id: uuid::Uuid,
    secret: String,
    expires_in: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Poll {
    status: String,
    session: Option<LoginReply>,
}
fn browser_url(base: &str, id: uuid::Uuid) -> Result<reqwest::Url, String> {
    api_url(base, &format!("/api/auth/oidc/authorize/{id}"))
}
fn open_browser(url: &reqwest::Url) -> Result<(), String> {
    // Fixed executable and a single validated HTTP(S) argument; never invoke a shell.
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("/usr/bin/open")
        .arg(url.as_str())
        .spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url.as_str())
        .spawn();
    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open")
        .arg(url.as_str())
        .spawn();
    let mut child = result.map_err(|_| {
        "Could not open the system browser. Check your default browser settings.".to_string()
    })?;
    // Some Linux launchers wait for the browser to close; reap without blocking sign-in polling.
    tauri::async_runtime::spawn_blocking(move || {
        let _ = child.wait();
    });
    Ok(())
}
#[tauri::command]
pub fn server_oidc_cancel(app: tauri::AppHandle) {
    app.state::<OidcState>().cancel();
}
#[tauri::command]
pub async fn server_oidc_login(
    app: tauri::AppHandle,
    projects: tauri::State<'_, super::super::projects::commands::ProjectState>,
) -> Result<ConnectionView, String> {
    let cancel = app.state::<OidcState>().begin()?;
    let _pending = Pending(cancel.clone());
    let root = app.path().app_data_dir().map_err(|_| SETTINGS_ERROR)?;
    let current = settings(&root)?;
    let client = client()?;
    let response = tokio::select! {
        _ = cancel.cancelled() => return Err("Browser sign-in cancelled.".into()),
        response = client.post(api_url(&current.server_url, "/api/auth/oidc/start")?)
            .header(reqwest::header::CONTENT_LENGTH, 0).send() =>
            response.map_err(|_| "Could not start browser sign-in.".to_string())?,
    };
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    let start: Start = values::read_json(response).await?;
    if start.secret.len() != 44 || start.expires_in == 0 || start.expires_in > 300 {
        return Err("The server returned an invalid browser sign-in handoff.".into());
    }
    let secret = zeroize::Zeroizing::new(start.secret);
    let url = browser_url(&current.server_url, start.id)?;
    if cancel.is_cancelled() {
        return Err("Browser sign-in cancelled.".into());
    }
    tauri::async_runtime::spawn_blocking(move || open_browser(&url))
        .await
        .map_err(|_| "Could not open browser sign-in.".to_string())??;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(start.expires_in);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Err("Browser sign-in cancelled.".into()),
            _ = tokio::time::sleep_until(deadline) => return Err("Browser sign-in expired. Try again.".into()),
            _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {},
        }
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err("Browser sign-in cancelled.".into()),
            response = client.post(api_url(&current.server_url, "/api/auth/oidc/poll")?)
                .json(&serde_json::json!({"id": start.id, "secret": secret.as_str()})).send() =>
                response.map_err(|_| "Browser sign-in was interrupted. Start a new sign-in.".to_string())?,
        };
        if !response.status().is_success() {
            return Err(response_error(response).await);
        }
        let poll: Poll = values::read_json(response).await?;
        match poll.status.as_str() {
            "waiting" => {},
            "complete" => {
                let reply = poll.session.ok_or("The server returned no verified session.")?;
                if cancel.is_cancelled() { return Err("Browser sign-in cancelled.".into()); }
                return finish_login(app, projects, current, reply, client, Some(&cancel)).await;
            },
            "pendingApproval" => return Err("Your OIDC account is awaiting administrator approval. Sign in again after approval.".into()),
            "denied" => return Err("Your server account is not active. Contact the server administrator.".into()),
            _ => return Err("Browser sign-in failed or expired. Start a new sign-in.".into()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_and_handoffs_never_allow_an_arbitrary_browser_destination() {
        let state = OidcState::default();
        let token = state.begin().unwrap();
        assert!(state.begin().is_err());
        state.cancel();
        assert!(token.is_cancelled());
        assert!(state.commit(&token, || Ok(())).is_err());
        assert!(state.begin().is_ok());
        let id = uuid::Uuid::new_v4();
        let url = browser_url("https://synthetic.example.test", id).unwrap();
        assert_eq!(url.host_str(), Some("synthetic.example.test"));
        assert_eq!(url.path(), format!("/api/auth/oidc/authorize/{id}"));
        assert!(!url.as_str().contains("secret"));
    }
}
