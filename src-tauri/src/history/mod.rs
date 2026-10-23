//! Local, bounded history metadata. This module never accepts resolved requests or payloads.
pub mod commands;
mod redact;
use crate::projects::model::validate_id;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tempfile::Builder;
use uuid::Uuid;

pub const MAX_ENTRIES: usize = 100;
pub const MAX_AGE_SECONDS: u64 = 7 * 24 * 60 * 60;
const MAX_FILE_BYTES: u64 = 256 * 1024;
const IO_ERROR: &str = "Could not access local request history. Check application-data permissions and available disk space.";
const DATA_ERROR: &str = "Local request history is invalid or from an unsupported version. Clear history explicitly to reset it.";
const CLEANUP_ERROR: &str = "Some local history could not be expired. Check application-data permissions or clear affected project history.";

/// Exactly the four permitted visible fields. No timestamp, identity, outcome, or payload crosses IPC.
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    pub url: String,
    pub method: String,
    pub status: Option<u16>,
    pub elapsed_ms: Option<u64>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredEntry {
    recorded_at: u64,
    summary: Entry,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    schema_version: u32,
    project_id: String,
    generation: String,
    recording: bool,
    entries: Vec<StoredEntry>,
}
impl Document {
    fn empty(project: &str, recording: bool) -> Self {
        Self {
            schema_version: 1,
            project_id: project.into(),
            generation: Uuid::new_v4().to_string(),
            recording,
            entries: vec![],
        }
    }
    fn validate(&self, project: &str) -> Result<(), String> {
        if self.schema_version != 1
            || self.project_id != project
            || validate_id(&self.generation).is_err()
        {
            return Err(DATA_ERROR.into());
        }
        for entry in &self.entries {
            let value = &entry.summary;
            if value.url.len() > 2048
                || redact::url(&value.url) != value.url
                || !valid_method(&value.method)
                || value.status.is_some_and(|s| !(100..=999).contains(&s))
            {
                return Err(DATA_ERROR.into());
            }
        }
        Ok(())
    }
    fn prune(&mut self, time: u64) -> bool {
        let count = self.entries.len();
        self.entries
            .retain(|e| e.recorded_at <= time && time - e.recorded_at < MAX_AGE_SECONDS);
        // Stable sorting preserves completion order for equal second-resolution timestamps.
        self.entries.sort_by_key(|e| e.recorded_at);
        if self.entries.len() > MAX_ENTRIES {
            self.entries.drain(..self.entries.len() - MAX_ENTRIES);
        }
        count != self.entries.len()
    }
    fn view(self) -> View {
        View {
            recording: self.recording,
            entries: self.entries.into_iter().rev().map(|e| e.summary).collect(),
            warning: None,
        }
    }
}
fn valid_method(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.bytes().all(|b| b.is_ascii_uppercase() || b == b'-')
}

/// Built from the unresolved draft before dispatch; cannot contain resolved URL values.
pub struct Candidate(Entry);
impl Candidate {
    pub fn new(method: &str, unresolved_url: &str) -> Self {
        Self(Entry {
            url: redact::url(unresolved_url),
            method: if valid_method(method) {
                method.into()
            } else {
                "INVALID".into()
            },
            status: None,
            elapsed_ms: None,
        })
    }
}
pub struct Ticket {
    project: String,
    generation: String,
    candidate: Candidate,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub recording: bool,
    pub entries: Vec<Entry>,
    pub warning: Option<&'static str>,
}
#[derive(Clone)]
pub struct Store {
    root: PathBuf,
}
impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    fn path(&self, project: &str) -> Result<PathBuf, String> {
        validate_id(project).map_err(|_| "Invalid history project identifier.".to_string())?;
        Ok(self.root.join(format!("{project}.json")))
    }
    fn lock(&self, create: bool) -> Result<Option<File>, String> {
        if !self.root.exists() && !create {
            return Ok(None);
        }
        fs::create_dir_all(&self.root).map_err(|_| IO_ERROR)?;
        safe(&self.root, true)?;
        let path = self.root.join("write.lock");
        if path.symlink_metadata().is_ok() {
            safe(&path, false)?;
        }
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path).map_err(|_| IO_ERROR)?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(
                        "Local history is busy in another Fluxkit operation. Retry explicitly."
                            .into(),
                    )
                }
                Err(_) => return Err(IO_ERROR.into()),
            }
        }
        Ok(Some(file))
    }
    fn cleanup_staging(&self) -> Result<(), String> {
        // Only this dedicated directory's private staging prefix is eligible; the lock
        // prevents deleting another cooperating writer's live staging file.
        for item in fs::read_dir(&self.root).map_err(|_| IO_ERROR)? {
            let item = item.map_err(|_| IO_ERROR)?;
            let name = item.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(".history-stage-") && name.ends_with(".json") {
                safe(&item.path(), false)?;
                fs::remove_file(item.path()).map_err(|_| IO_ERROR)?;
            }
        }
        Ok(())
    }
    fn read(&self, project: &str) -> Result<Option<Document>, String> {
        let path = self.path(project)?;
        match path.symlink_metadata() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(IO_ERROR.into()),
            Ok(_) => safe(&path, false)?,
        }
        let file = File::open(&path).map_err(|_| IO_ERROR)?;
        if file.metadata().map_err(|_| IO_ERROR)?.len() > MAX_FILE_BYTES {
            return Err(DATA_ERROR.into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| IO_ERROR)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(DATA_ERROR.into());
        }
        let document: Document = serde_json::from_slice(&bytes).map_err(|_| DATA_ERROR)?;
        document.validate(project)?;
        Ok(Some(document))
    }
    fn write(&self, document: &Document) -> Result<(), String> {
        document.validate(&document.project_id)?;
        let path = self.path(&document.project_id)?;
        if path.symlink_metadata().is_ok() {
            safe(&path, false)?;
        }
        let mut bytes = serde_json::to_vec_pretty(document).map_err(|_| DATA_ERROR)?;
        bytes.push(b'\n');
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(DATA_ERROR.into());
        }
        let mut staged = Builder::new()
            .prefix(".history-stage-")
            .suffix(".json")
            .tempfile_in(&self.root)
            .map_err(|_| IO_ERROR)?;
        staged.write_all(&bytes).map_err(|_| IO_ERROR)?;
        staged.as_file().sync_all().map_err(|_| IO_ERROR)?;
        staged.persist(&path).map_err(|_| IO_ERROR)?;
        #[cfg(unix)]
        if let Ok(dir) = File::open(&self.root) {
            let _ = dir.sync_all();
        }
        Ok(())
    }
    pub fn view(&self, project: &str, time: u64) -> Result<View, String> {
        self.path(project)?;
        let Some(_guard) = self.lock(false)? else {
            return Ok(Document::empty(project, false).view());
        };
        let mut document = self
            .read(project)?
            .unwrap_or_else(|| Document::empty(project, false));
        if document.prune(time) {
            self.write(&document)?;
        }
        Ok(document.view())
    }
    pub fn set_recording(&self, project: &str, enabled: bool, time: u64) -> Result<View, String> {
        self.path(project)?;
        let _guard = self.lock(true)?;
        let mut document = self
            .read(project)?
            .unwrap_or_else(|| Document::empty(project, false));
        document.prune(time);
        if document.recording != enabled {
            document.generation = Uuid::new_v4().to_string();
        }
        document.recording = enabled;
        self.write(&document)?;
        Ok(document.view())
    }
    /// Explicitly clearing unreadable data is authorized by the UI confirmation.
    /// Recording resets to off if its previous setting cannot be read safely.
    pub fn clear(&self, project: &str, time: u64) -> Result<View, String> {
        self.path(project)?;
        let _guard = self.lock(true)?;
        let recording = self
            .read(project)
            .ok()
            .flatten()
            .is_some_and(|d| d.recording);
        let mut document = Document::empty(project, recording);
        document.prune(time);
        self.write(&document)?;
        Ok(document.view())
    }
    pub fn begin(
        &self,
        project: &str,
        candidate: Candidate,
        time: u64,
    ) -> Result<Option<Ticket>, String> {
        self.path(project)?;
        let Some(_guard) = self.lock(false)? else {
            return Ok(None);
        };
        let Some(mut document) = self.read(project)? else {
            return Ok(None);
        };
        if document.prune(time) {
            self.write(&document)?;
        }
        Ok(document.recording.then(|| Ticket {
            project: project.into(),
            generation: document.generation,
            candidate,
        }))
    }
    pub fn finish(
        &self,
        ticket: Ticket,
        status: Option<u16>,
        elapsed_ms: Option<u64>,
        time: u64,
    ) -> Result<(), String> {
        let Some(_guard) = self.lock(false)? else {
            return Ok(());
        };
        let Some(mut document) = self.read(&ticket.project)? else {
            return Ok(());
        };
        let pruned = document.prune(time);
        if !document.recording || document.generation != ticket.generation {
            if pruned {
                self.write(&document)?;
            }
            return Ok(());
        }
        let mut summary = ticket.candidate.0;
        summary.status = status;
        summary.elapsed_ms = elapsed_ms;
        document.entries.push(StoredEntry {
            recorded_at: time,
            summary,
        });
        document.prune(time);
        self.write(&document)
    }
    /// Scan one private directory, never project folders; keep processing independent files
    /// when one entry is malformed/unavailable. Call on a worker at startup and every minute.
    pub fn cleanup(&self, time: u64) -> Result<(), String> {
        let Some(_guard) = self.lock(false)? else {
            return Ok(());
        };
        self.cleanup_staging()?;
        let mut failed = false;
        for item in fs::read_dir(&self.root).map_err(|_| IO_ERROR)? {
            let item = match item {
                Ok(item) => item,
                Err(_) => {
                    failed = true;
                    continue;
                }
            };
            let name = item.file_name();
            let Some(project) = name
                .to_str()
                .and_then(|name| name.strip_suffix(".json"))
                .filter(|id| validate_id(id).is_ok())
            else {
                continue;
            };
            let result = (|| {
                if let Some(mut document) = self.read(project)? {
                    if document.prune(time) {
                        self.write(&document)?;
                    }
                }
                Ok::<_, String>(())
            })();
            failed |= result.is_err();
        }
        if failed {
            Err(CLEANUP_ERROR.into())
        } else {
            Ok(())
        }
    }
}
fn safe(path: &Path, directory: bool) -> Result<(), String> {
    let metadata = path.symlink_metadata().map_err(|_| IO_ERROR)?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        Err(IO_ERROR.into())
    } else {
        Ok(())
    }
}
pub fn now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|t| t.as_secs())
        .map_err(|_| "Check the system clock before accessing request history.".into())
}
#[derive(Clone, Default)]
pub struct HistoryState {
    store: Option<Store>,
    cleanup_failed: Arc<AtomicBool>,
}
impl HistoryState {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            store: root.map(Store::new),
            ..Self::default()
        }
    }
    pub fn store(&self) -> Result<Store, String> {
        self.store.clone().ok_or(
            "Local history storage is unavailable. Check the application-data directory.".into(),
        )
    }
    pub fn warning(&self) -> Option<&'static str> {
        self.cleanup_failed
            .load(Ordering::Relaxed)
            .then_some(CLEANUP_ERROR)
    }
    pub fn start_cleanup(&self) {
        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut timer = tokio::time::interval(std::time::Duration::from_secs(60));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                timer.tick().await;
                let worker_state = state.clone();
                let result = tauri::async_runtime::spawn_blocking(move || {
                    worker_state.store()?.cleanup(now()?)
                })
                .await;
                state
                    .cleanup_failed
                    .store(!result.is_ok_and(|r| r.is_ok()), Ordering::Relaxed);
            }
        });
    }
}

#[cfg(test)]
mod tests;
