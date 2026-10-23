use super::{now, HistoryState, View};
use crate::projects::commands::{run, ProjectState};
use serde::Deserialize;
use tauri::State;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Action {
    Load,
    SetRecording { enabled: bool },
    Clear,
}

/// Only the bundled window can operate on a project handle issued by the native chooser.
#[tauri::command]
pub async fn history_action(
    projects: State<'_, ProjectState>,
    history: State<'_, HistoryState>,
    handle: String,
    action: Action,
) -> Result<View, String> {
    let store = history.store()?;
    let mut view = run(&projects, move |projects| {
        projects.with_environment(&handle, |_, id| {
            let time = now()?;
            match action {
                Action::Load => store.view(id, time),
                Action::SetRecording { enabled } => store.set_recording(id, enabled, time),
                Action::Clear => store.clear(id, time),
            }
        })
    })
    .await?;
    view.warning = history.warning();
    Ok(view)
}
