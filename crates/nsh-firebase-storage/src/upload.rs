//! The resumable upload task: one spawned loop that starts a session,
//! queries what the server already holds and pushes 256 KiB slices, with
//! the progress signal and the caller's pause/resume/cancel. The whole
//! state machine runs on the engine thread (the loop is a task of the
//! owner that started it, so it dies with that component).

use std::{cell::Cell, rc::Rc, time::Duration};

use nsh_core::{element::HttpResponse, prelude::*};
use nsh_firebase_core::FirebaseError;
use serde_json::Value as JsonValue;

use crate::wire;

/// The progress states, the same strings the TypeScript task uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadState {
    Paused,
    Running,
    Done,
    Error,
}

impl UploadState {
    pub fn as_str(&self) -> &'static str {
        match self {
            UploadState::Paused => "paused",
            UploadState::Running => "running",
            UploadState::Done => "done",
            UploadState::Error => "error",
        }
    }
}

/// The snapshot the progress signal holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadProgress {
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub state: UploadState,
}

/// What one loop needs between spawns (a resume spawns a fresh loop at
/// the same handle).
pub(crate) struct UploadJob {
    /// The URL the start command goes to.
    pub start_url: String,
    /// The object path, which errors name.
    pub path: String,
    /// The start body: the metadata's writable fields plus the name.
    pub start_body: Vec<u8>,
    /// `X-Goog-Upload-Header-Content-Type` (the same value the body has).
    pub start_content_type: String,
    /// The session URI; `None` until the start answers (a `resume_upload`
    /// handle provides one up front). RefCell: the loop shares the handle.
    pub session_url: std::cell::RefCell<Option<String>>,
    /// The whole payload, sliced from what the server reports it holds.
    pub bytes: Rc<Vec<u8>>,
}

pub(crate) struct UploadTaskInner {
    pub progress: Signal<UploadProgress>,
    pub result: Signal<Option<Result<wire::ObjectMetadata, FirebaseError>>>,
    pub paused: Cell<bool>,
    pub cancelled: Cell<bool>,
    pub job: UploadJob,
    /// `Authorization: Firebase <token>` comes from here when wired.
    pub auth: Option<Rc<dyn Fn() -> Option<String>>>,
}

impl UploadTaskInner {
    fn take_session(&self) -> Option<String> {
        self.job.session_url.borrow_mut().take()
    }

    fn store_session(&self, url: String) {
        *self.job.session_url.borrow_mut() = Some(url);
    }
}

/// A resumable upload in flight: its progress as a signal, and the
/// caller's [`pause`](Self::pause)/[`resume`](Self::resume)/[`cancel`](Self::cancel).
#[derive(Clone)]
pub struct UploadTask {
    pub(crate) inner: Rc<UploadTaskInner>,
}

impl PartialEq for UploadTask {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl std::fmt::Debug for UploadTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UploadTask")
    }
}

impl UploadTask {
    pub(crate) fn new(inner: Rc<UploadTaskInner>) -> Self {
        Self { inner }
    }

    /// The progress signal: `bytesTransferred`, `totalBytes`, `state`.
    pub fn progress(&self) -> ReadSignal<UploadProgress> {
        self.inner.progress.read_only()
    }

    /// The current snapshot, without subscribing.
    pub fn snapshot(&self) -> Option<UploadProgress> {
        self.inner.progress.try_get()
    }

    /// The session URI the server issued — what a resumed run persists
    /// across process death — or `None` before the start answered.
    pub fn session_url(&self) -> Option<String> {
        self.inner.job.session_url.borrow().clone()
    }

    /// The final metadata, or why there is none (cancel included);
    /// resolves once the loop reached `done` or `error`.
    pub async fn finished(&self) -> Result<wire::ObjectMetadata, FirebaseError> {
        loop {
            match self.inner.result.try_get() {
                // The signal is gone: its owner was disposed and the
                // upload died with it (spawn cancels its tasks).
                None => {
                    return Err(FirebaseError::new(
                        wire::CANCELED,
                        "the upload's owner was disposed",
                    ));
                }
                Some(Some(ended)) => return ended,
                Some(None) => timer(Duration::from_millis(16)).await,
            }
        }
    }

    /// Stops after the slice in flight; the loop ends in `paused` and the
    /// session stays on the server for [`resume`](Self::resume).
    pub fn pause(&self) {
        let snapshot = match self.inner.progress.try_get() {
            Some(snapshot) => snapshot,
            None => return,
        };
        if snapshot.state != UploadState::Running {
            return;
        }
        self.inner.paused.set(true);
    }

    /// Queries the session for the server's offset and runs the rest.
    pub fn resume(&self) {
        let Some(snapshot) = self.inner.progress.try_get() else {
            return;
        };
        if snapshot.state != UploadState::Paused {
            return;
        }
        self.inner.paused.set(false);
        self.inner.progress.try_set(UploadProgress {
            bytes_transferred: snapshot.bytes_transferred,
            total_bytes: snapshot.total_bytes,
            state: UploadState::Running,
        });
        spawn(run_loop(Rc::new(UploadTask {
            inner: Rc::clone(&self.inner),
        })));
    }

    /// Abandons the upload: the loop ends in `error` with
    /// `storage/canceled` (an in-flight slice still lands).
    pub fn cancel(&self) {
        let snapshot = match self.inner.progress.try_get() {
            Some(snapshot) => snapshot,
            None => return,
        };
        self.inner.cancelled.set(true);
        if snapshot.state == UploadState::Paused {
            // No loop is running: end it here.
            self.end(Err(FirebaseError::new(
                wire::CANCELED,
                "the upload was canceled",
            )));
        }
    }

    /// Records a final outcome (once) and moves the state to `done` or
    /// `error`. A done upload reports all bytes: the finalize answered.
    pub(crate) fn end(&self, outcome: Result<wire::ObjectMetadata, FirebaseError>) {
        if self.inner.result.try_get().flatten().is_some() {
            return;
        }
        let snapshot = self.inner.progress.try_get();
        let state = if outcome.is_ok() {
            UploadState::Done
        } else {
            UploadState::Error
        };
        self.inner.result.try_set(Some(outcome));
        if let Some(snapshot) = snapshot {
            self.inner.progress.try_set(UploadProgress {
                bytes_transferred: if state == UploadState::Done {
                    snapshot.total_bytes
                } else {
                    snapshot.bytes_transferred
                },
                total_bytes: snapshot.total_bytes,
                state,
            });
        }
    }

    pub(crate) fn set_running(&self, bytes_transferred: u64) {
        if let Some(snapshot) = self.inner.progress.try_get() {
            self.inner.progress.try_set(UploadProgress {
                bytes_transferred,
                total_bytes: snapshot.total_bytes,
                state: UploadState::Running,
            });
        }
    }

    /// Advances `bytesTransferred` while the loop is still running (a
    /// late slice response after a pause leaves the state alone).
    pub(crate) fn transferred(&self, bytes_transferred: u64) {
        if let Some(snapshot) = self.inner.progress.try_get()
            && snapshot.state == UploadState::Running
        {
            self.inner.progress.try_set(UploadProgress {
                bytes_transferred,
                total_bytes: snapshot.total_bytes,
                state: UploadState::Running,
            });
        }
    }

    /// The loop ended at a pause boundary: state `paused`, no result.
    pub(crate) fn mark_paused(&self) {
        if let Some(snapshot) = self.inner.progress.try_get() {
            self.inner.progress.try_set(UploadProgress {
                bytes_transferred: snapshot.bytes_transferred,
                total_bytes: snapshot.total_bytes,
                state: UploadState::Paused,
            });
        }
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.inner.cancelled.get()
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.inner.paused.get()
    }

    fn auth_header(&self) -> Vec<(&'static str, String)> {
        auth_header(self.inner.auth.as_deref())
    }

    /// Sends `request`, adding the auth header when a provider is wired.
    async fn send(
        &self,
        request: nsh_core::platform::HttpRequest,
    ) -> Result<HttpResponse, FirebaseError> {
        let mut request = request;
        for (name, value) in self.auth_header() {
            request = request.header(name, value);
        }
        nsh_firebase_core::send(request).await
    }
}

pub(crate) fn auth_header(
    auth: Option<&(dyn Fn() -> Option<String> + 'static)>,
) -> Vec<(&'static str, String)> {
    match auth.and_then(|auth| auth()) {
        Some(token) if !token.is_empty() => {
            vec![("Authorization", format!("Firebase {token}"))]
        }
        _ => Vec::new(),
    }
}

/// The loop's sentinel for "the caller paused": `run_loop` turns it into
/// the paused state, never into a task result.
const PAUSED: &str = "storage/paused";

/// Runs the session to its end: start (when the session URI is unknown),
/// a query for the server's offset, then 256 KiB slices to the last one.
pub(crate) async fn run_loop(task: Rc<UploadTask>) {
    task.set_running(task.snapshot().map_or(0, |s| s.bytes_transferred));
    match run_session(&task).await {
        Err(error) if error.code == PAUSED && task.is_paused() => task.mark_paused(),
        outcome => task.end(outcome),
    }
}

async fn run_session(task: &Rc<UploadTask>) -> Result<wire::ObjectMetadata, FirebaseError> {
    let total = task.snapshot().map_or(0, |s| s.total_bytes);
    let path = task.inner.job.path.clone();
    // A denial carries `x-goog-upload-status: final` even on a 403:
    // the HTTP status is read before any upload-status header.
    let ensure_ok = |response: &HttpResponse| -> Result<(), FirebaseError> {
        if response.ok() {
            Ok(())
        } else {
            Err(map_status_error(response.status(), &path, &response.text()))
        }
    };
    let session = match task.inner.take_session() {
        Some(session) => session,
        None => {
            let response = task
                .send(
                    nsh_core::platform::HttpRequest::post(
                        task.inner.job.start_url.clone(),
                        task.inner.job.start_body.clone(),
                    )
                    .header("X-Goog-Upload-Protocol", "resumable")
                    .header("X-Goog-Upload-Command", "start")
                    .header("X-Goog-Upload-Header-Content-Length", total.to_string())
                    .header(
                        "X-Goog-Upload-Header-Content-Type",
                        task.inner.job.start_content_type.clone(),
                    )
                    .header("Content-Type", "application/json; charset=utf-8"),
                )
                .await?;
            ensure_ok(&response)?;
            check_upload_status(&response, &["active"])?;
            let session = response.header("X-Goog-Upload-URL").ok_or_else(|| {
                FirebaseError::new(
                    wire::RETRY_LIMIT_EXCEEDED,
                    "the start response has no X-Goog-Upload-URL",
                )
            })?;
            let session = session.to_owned();
            task.inner.store_session(session.clone());
            session
        }
    };

    // The server's offset is the truth: it may hold a slice whose
    // response was lost, which is what makes resume-from-id safe.
    let query = task
        .send(
            nsh_core::platform::HttpRequest::post(session.clone(), Vec::new())
                .header("X-Goog-Upload-Command", "query"),
        )
        .await?;
    ensure_ok(&query)?;
    let query_status = check_upload_status(&query, &["active", "final"])?;
    let size_received: u64 = query
        .header("X-Goog-Upload-Size-Received")
        .and_then(|size| size.parse().ok())
        .ok_or_else(|| {
            FirebaseError::new(
                wire::RETRY_LIMIT_EXCEEDED,
                "the query response has no X-Goog-Upload-Size-Received",
            )
        })?;
    if size_received > total {
        return Err(FirebaseError::new(
            wire::SERVER_FILE_WRONG_SIZE,
            format!("the session holds {size_received} bytes, more than the {total} to upload"),
        ));
    }
    if query_status == "final" && size_received != total {
        return Err(FirebaseError::new(
            wire::SERVER_FILE_WRONG_SIZE,
            format!("the session finalized at {size_received} bytes, not the {total} to upload"),
        ));
    }
    task.set_running(size_received);

    let mut offset = size_received;
    while offset < total {
        if task.is_cancelled() {
            return Err(FirebaseError::new(
                wire::CANCELED,
                "the upload was canceled",
            ));
        }
        if task.is_paused() {
            // Leave the session live; resume() starts a fresh loop that
            // queries before it slices.
            return Err(FirebaseError::new(
                PAUSED,
                "the upload is paused; resume() continues it",
            ));
        }
        let end = (offset as usize + wire::CHUNK_SIZE).min(total as usize);
        let slice = &task.inner.job.bytes[offset as usize..end];
        let command = if end == total as usize {
            "upload, finalize"
        } else {
            "upload"
        };
        let response = task
            .send(
                nsh_core::platform::HttpRequest::post(session.clone(), slice.to_vec())
                    .header("X-Goog-Upload-Command", command)
                    .header("X-Goog-Upload-Offset", offset.to_string())
                    .header("Content-Type", "application/octet-stream"),
            )
            .await?;
        ensure_ok(&response)?;
        let status = check_upload_status(&response, &["active", "final"])?;
        offset = end as u64;
        if status == "final" {
            let metadata = parse_final_metadata(&response)?;
            return Ok(metadata);
        }
        task.transferred(offset);
    }

    // Every byte landed but nothing finalized (the size hit the chunk
    // boundary): an empty `finalize` asks for the object metadata.
    if task.is_cancelled() {
        return Err(FirebaseError::new(
            wire::CANCELED,
            "the upload was canceled",
        ));
    }
    let response = task
        .send(
            nsh_core::platform::HttpRequest::post(session, Vec::new())
                .header("X-Goog-Upload-Command", "finalize")
                .header("X-Goog-Upload-Offset", offset.to_string())
                .header("Content-Type", "application/octet-stream"),
        )
        .await?;
    ensure_ok(&response)?;
    check_upload_status(&response, &["final"])?;
    parse_final_metadata(&response)
}

/// The upload status header must name one of `allowed`
/// (`checkResumeHeader_` in the SDK; a miss is the retry-limit error).
fn check_upload_status(response: &HttpResponse, allowed: &[&str]) -> Result<String, FirebaseError> {
    let status = response.header("X-Goog-Upload-Status").ok_or_else(|| {
        FirebaseError::new(
            wire::RETRY_LIMIT_EXCEEDED,
            format!(
                "the response has no X-Goog-Upload-Status (HTTP {})",
                response.status()
            ),
        )
    })?;
    if !allowed.contains(&status) {
        return Err(FirebaseError::new(
            wire::RETRY_LIMIT_EXCEEDED,
            format!(
                "the upload status is {status}, not one of {}",
                allowed.join("/")
            ),
        ));
    }
    Ok(status.to_owned())
}

/// A `final` slice response carries the object metadata as its body.
fn parse_final_metadata(response: &HttpResponse) -> Result<wire::ObjectMetadata, FirebaseError> {
    let json: JsonValue = serde_json::from_str(&response.text()).map_err(|error| {
        FirebaseError::new(
            wire::UNKNOWN,
            format!("the final response body is not JSON: {error}"),
        )
    })?;
    Ok(wire::ObjectMetadata::parse(&json))
}

/// HTTP error statuses become the SDK's storage errors.
pub(crate) fn map_status_error(status: u16, path: &str, body: &str) -> FirebaseError {
    let code = match status {
        401 => wire::UNAUTHENTICATED,
        402 => wire::QUOTA_EXCEEDED,
        403 => wire::UNAUTHORIZED,
        404 => wire::OBJECT_NOT_FOUND,
        _ => wire::UNKNOWN,
    };
    let detail = body.trim();
    FirebaseError::new(
        code,
        if detail.is_empty() {
            format!("{status} on {path}")
        } else {
            format!("{status} on {path}: {detail}")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_read_as_the_type_script_strings() {
        assert_eq!(UploadState::Paused.as_str(), "paused");
        assert_eq!(UploadState::Running.as_str(), "running");
        assert_eq!(UploadState::Done.as_str(), "done");
        assert_eq!(UploadState::Error.as_str(), "error");
    }

    #[test]
    fn status_errors_map_like_the_sdk() {
        let error = map_status_error(404, "a/b.txt", "");
        assert_eq!(error.code, wire::OBJECT_NOT_FOUND);
        assert_eq!(error.message, "404 on a/b.txt");
        assert_eq!(map_status_error(403, "x", "").code, wire::UNAUTHORIZED);
        assert_eq!(map_status_error(401, "x", "").code, wire::UNAUTHENTICATED);
        assert_eq!(map_status_error(402, "x", "").code, wire::QUOTA_EXCEEDED);
        let server_said = map_status_error(500, "x", "{\"error\":\"boom\"}");
        assert_eq!(server_said.code, wire::UNKNOWN);
        assert!(server_said.message.contains("boom"));
    }

    #[test]
    fn the_auth_header_is_firebase_prefixed_and_optional() {
        let provider = || Some("id-token".to_owned());
        assert_eq!(
            auth_header(Some(&provider)),
            vec![("Authorization", "Firebase id-token".to_owned())]
        );
        let empty = || None::<String>;
        assert!(auth_header(Some(&empty)).is_empty());
        assert!(auth_header(None).is_empty());
    }
}
