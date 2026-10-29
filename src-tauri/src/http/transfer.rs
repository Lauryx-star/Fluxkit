use super::{
    model::{ContentKind, ExecutionResult, Header, Media, Outcome},
    prepare::Prepared,
};
use reqwest::{
    header::{CONTENT_DISPOSITION, CONTENT_ENCODING, CONTENT_TYPE},
    Client,
};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

pub const PREVIEW_LIMIT: usize = 1024 * 1024;
/// Images and PDFs are kept in memory for the Preview tab up to this size.
pub const MEDIA_LIMIT: usize = 20 * 1024 * 1024;

/// Media types the UI renders without executing content: raster images and SVG only via `<img>`
/// (no scripts), PDFs only through pdf.js with eval disabled.
fn previewable(mime: &str) -> bool {
    matches!(
        mime,
        "image/png"
            | "image/jpeg"
            | "image/gif"
            | "image/webp"
            | "image/bmp"
            | "image/avif"
            | "image/x-icon"
            | "image/vnd.microsoft.icon"
            | "image/svg+xml"
            | "application/pdf"
    )
}

#[derive(PartialEq)]
struct Stamp {
    modified: SystemTime,
    len: u64,
}
fn stamp(path: &Path) -> Result<Option<Stamp>, String> {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            Ok(Some(Stamp {
                modified: metadata
                    .modified()
                    .map_err(|_| "Could not inspect the download target.")?,
                len: metadata.len(),
            }))
        }
        Ok(_) => Err("Choose a regular download target, without a symbolic link.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Could not access the download target.".into()),
    }
}
/// A staged file exists only after an explicit save action. Drop removes partial output.
pub struct Download {
    temporary: NamedTempFile,
    target: PathBuf,
    previous: Option<Stamp>,
}
impl Download {
    pub(super) fn staged_path(&self) -> PathBuf {
        self.temporary.path().to_path_buf()
    }
    pub fn new(target: PathBuf) -> Result<Self, String> {
        let parent = target
            .parent()
            .ok_or("Choose a valid destination folder.")?;
        let parent = parent
            .canonicalize()
            .map_err(|_| "Could not open the destination folder.")?;
        let target = parent.join(
            target
                .file_name()
                .ok_or("Choose a valid destination file.")?,
        );
        let previous = stamp(&target)?;
        let temporary = NamedTempFile::new_in(&parent).map_err(|_| {
            "Could not stage the download. Check folder permissions and disk space."
        })?;
        Ok(Self {
            temporary,
            target,
            previous,
        })
    }
    fn writer(&self) -> Result<tokio::fs::File, String> {
        self.temporary
            .as_file()
            .try_clone()
            .map(tokio::fs::File::from_std)
            .map_err(|_| "Could not open the staged download.".into())
    }
    pub fn commit(self) -> Result<(), String> {
        if stamp(&self.target)? != self.previous {
            return Err("The destination changed during download. Nothing was replaced. Choose a different file and explicitly send again.".into());
        }
        if self.previous.is_none() {
            self.temporary
                .persist_noclobber(&self.target)
                .map_err(|_| {
                    "The destination exists or could not be created. Nothing was overwritten."
                })?;
        } else {
            self.temporary
                .persist(&self.target)
                .map_err(|_| "Could not replace the selected destination file.")?;
        }
        #[cfg(unix)]
        if let Some(parent) = self.target.parent() {
            if let Ok(directory) = std::fs::File::open(parent) {
                let _ = directory.sync_all();
            }
        }
        Ok(())
    }
}
fn milliseconds(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u64::MAX as u128) as u64
}
fn network_error(error: reqwest::Error) -> (Outcome, &'static str) {
    if error.is_timeout() {
        (
            Outcome::Timeout,
            "The request timed out. No retry was sent.",
        )
    } else {
        (Outcome::NetworkError, "The HTTP exchange failed. Check connectivity, TLS certificates, and the target API. No retry was sent.")
    }
}
struct LimitedWriter(Vec<u8>);
impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len() + bytes.len() > PREVIEW_LIMIT {
            return Err(std::io::Error::other("Preview limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn preview(result: &mut ExecutionResult, bytes: Vec<u8>, textual: bool, json: bool) {
    if result.byte_count == 0 {
        return;
    }
    if !textual {
        result.content_kind = ContentKind::Binary;
        return;
    }
    let text = match std::str::from_utf8(&bytes) {
        Ok(value) => value,
        Err(error) if result.truncated && error.error_len().is_none() => {
            match std::str::from_utf8(&bytes[..error.valid_up_to()]) {
                Ok(value) => value,
                Err(_) => {
                    result.content_kind = ContentKind::Binary;
                    return;
                }
            }
        }
        Err(_) => {
            result.content_kind = ContentKind::Binary;
            return;
        }
    };
    if text.contains('\0') {
        result.content_kind = ContentKind::Binary;
        return;
    }
    result.content_kind = if json {
        ContentKind::Json
    } else {
        ContentKind::Text
    };
    if !result.truncated && (json || text.trim_start().starts_with(['{', '['])) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
            let mut writer = LimitedWriter(Vec::new());
            if serde_json::to_writer_pretty(&mut writer, &value).is_ok() {
                result.json_text = String::from_utf8(writer.0).ok();
                result.content_kind = ContentKind::Json;
            }
        }
    }
    result.text = Some(text.into());
}

pub async fn execute(
    client: &Client,
    prepared: Prepared,
    timeout: std::time::Duration,
    cancel: &CancellationToken,
    mut download: Option<Download>,
) -> ExecutionResult {
    if prepared.token.is_some() {
        return ExecutionResult::failure(
            Outcome::InvalidRequest,
            "Acquire the configured token before API dispatch.",
        );
    }
    let start = Instant::now();
    let mut result = ExecutionResult::failure(Outcome::Response, "");
    result.message = None;
    result.request_headers = prepared.header_summary();
    let mut bytes = Vec::new();
    let mut textual = false;
    let mut json = false;
    let mut media: Option<(String, Vec<u8>, bool)> = None;
    let exchange = async {
        let mut response = client
            .execute(prepared.request)
            .await
            .map_err(network_error)?;
        result.status = Some(response.status().as_u16());
        let mut header_bytes = 0;
        for (name, value) in response.headers() {
            header_bytes += name.as_str().len() + value.as_bytes().len();
            if header_bytes > 64 * 1024 || result.headers.len() >= 1000 {
                return Err((
                    Outcome::NetworkError,
                    "Response headers exceed the 64 KiB or 1,000-header display limit.",
                ));
            }
            result.headers.push(Header {
                name: name.to_string(),
                value: value.as_bytes().iter().map(|b| char::from(*b)).collect(),
            });
        }
        let mime = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        json = mime == "application/json" || mime.ends_with("+json");
        let encoded = response
            .headers()
            .get(CONTENT_ENCODING)
            .is_some_and(|v| v != "identity");
        let attached = response
            .headers()
            .get(CONTENT_DISPOSITION)
            .is_some_and(|v| v.as_bytes().starts_with(b"attachment"));
        textual = !encoded
            && !attached
            && (mime.is_empty()
                || mime.starts_with("text/")
                || json
                || mime.ends_with("xml")
                || mime == "application/javascript"
                || mime == "application/x-www-form-urlencoded");
        // Compressed bodies stay binary: the client has no decompressor for them.
        if !encoded && previewable(&mime) {
            media = Some((mime.clone(), Vec::new(), true));
        }
        let mut writer = download
            .as_ref()
            .map(Download::writer)
            .transpose()
            .map_err(|_| (Outcome::SaveError, "Could not open the staged download."))?;
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            result.byte_count = result.byte_count.saturating_add(chunk.len() as u64);
            if textual {
                let remaining = PREVIEW_LIMIT.saturating_sub(bytes.len());
                bytes.extend_from_slice(&chunk[..remaining.min(chunk.len())]);
                result.truncated = result.byte_count > PREVIEW_LIMIT as u64;
            }
            if let Some((_, buffer, fits)) = &mut media {
                if *fits && buffer.len() + chunk.len() <= MEDIA_LIMIT {
                    buffer.extend_from_slice(&chunk);
                } else if *fits {
                    *fits = false;
                    *buffer = Vec::new();
                }
            }
            if let Some(writer) = &mut writer {
                writer.write_all(&chunk).await.map_err(|_| {
                    (
                        Outcome::SaveError,
                        "Could not write the download. Check disk space and permissions.",
                    )
                })?;
            }
        }
        result.elapsed_ms = Some(milliseconds(start));
        if let Some(writer) = writer {
            writer.sync_all().await.map_err(|_| {
                (
                    Outcome::SaveError,
                    "Could not synchronize the downloaded file.",
                )
            })?;
        }
        Ok::<(), (Outcome, &str)>(())
    };
    let completed = tokio::select! { biased;
        _ = cancel.cancelled() => Err((Outcome::Cancelled, "Request cancelled. The server may already have processed it.")),
        result = tokio::time::timeout(timeout, exchange) => match result {
            Ok(result) => result,
            Err(_) => Err((Outcome::Timeout, "The request timed out. The server may already have processed it. No retry was sent.")),
        }
    };
    if result.elapsed_ms.is_none() {
        result.elapsed_ms = Some(milliseconds(start));
    }
    if let Err((outcome, message)) = completed {
        result.outcome = outcome;
        result.message = Some(message.into());
        result.truncated = result.byte_count > 0;
        return result;
    }
    if cancel.is_cancelled() {
        result.outcome = Outcome::Cancelled;
        result.message = Some("Request cancelled before saving the response.".into());
        return result;
    }
    if let Some(destination) = download.take() {
        match tauri::async_runtime::spawn_blocking(move || destination.commit()).await {
            Ok(Ok(())) => result.saved = true,
            _ => {
                result.outcome = Outcome::SaveError;
                result.message = Some("Could not commit the downloaded file, or the destination changed. The previous destination was preserved where possible.".into());
            }
        }
    }
    // Parsing and pretty-printing run away from the async network executor and UI thread.
    tauri::async_runtime::spawn_blocking(move || {
        preview(&mut result, bytes, textual, json);
        if let Some((mime, buffer, fits)) = media.filter(|_| result.byte_count > 0) {
            use base64::{engine::general_purpose::STANDARD, Engine};
            result.media = Some(Media {
                mime,
                data: fits.then(|| STANDARD.encode(buffer)),
            });
        }
        result
    })
    .await
    .unwrap_or_else(|_| {
        ExecutionResult::failure(
            Outcome::NetworkError,
            "Could not prepare the response preview.",
        )
    })
}
