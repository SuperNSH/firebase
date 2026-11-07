//! The instrumented fetch wrapper over the core transport: one
//! [`NetworkEvent`] per exchange, recorded even when the exchange fails.
//! A query fetcher can call it where it would call
//! [`nsh_firebase_core::send`], so `nsh-query` results come with the
//! monitor filled in. The TypeScript mirror is
//! `packages/nsh-firebase/src/monitor`'s `instrumentFetch`.

use nsh_core::{
    platform::{HttpBody, HttpRequest},
    time,
};
use nsh_firebase_core::{FirebaseError, send};

use crate::{
    event::{NetworkEvent, next_event_id, split_host_path},
    monitor::Monitor,
};

/// The `initiator` the wrapper puts on its events.
pub const INITIATOR: &str = "firebase-fetch";

/// Sends `request` through the core transport and records the exchange
/// on `monitor`. The engine's `fetch` resolves with the whole response
/// (head and body together, `nsh-core`'s `Core::fetch_event` delivers at
/// `End`), so `duration_ms` covers the whole exchange and `ttfb_ms` stays
/// null; `response_bytes` comes from `content-length`, else the body's
/// length, which this transport already holds.
pub async fn instrumented_send(
    monitor: &Monitor,
    request: HttpRequest,
) -> Result<nsh_core::element::HttpResponse, FirebaseError> {
    let started_at_ms = time::unix_ms() as i64;
    let start = time::now();
    let method = request.method.clone();
    let url = request.url.clone();
    let request_bytes = match &request.body {
        HttpBody::Bytes(bytes) => Some(bytes.len() as u64),
        HttpBody::Empty | HttpBody::Stream => None,
    };

    let outcome = send(request).await;
    let duration_ms = time::now().saturating_duration_since(start).as_secs_f64() * 1000.0;
    let (host, path) = split_host_path(&url);
    let event = match &outcome {
        Ok(response) => NetworkEvent {
            id: next_event_id(),
            method,
            url,
            host,
            path,
            status: Some(i64::from(response.status())),
            started_at_ms,
            duration_ms,
            ttfb_ms: None,
            request_bytes,
            response_bytes: response
                .header("content-length")
                .and_then(|value| value.trim().parse::<u64>().ok())
                .or(Some(response.body.len() as u64)),
            error: None,
            initiator: Some(INITIATOR.to_owned()),
        },
        Err(error) => NetworkEvent {
            id: next_event_id(),
            method,
            url,
            host,
            path,
            status: None,
            started_at_ms,
            duration_ms,
            ttfb_ms: None,
            request_bytes,
            response_bytes: None,
            error: Some(error.message.clone()),
            initiator: Some(INITIATOR.to_owned()),
        },
    };
    monitor.record(event);
    outcome
}
