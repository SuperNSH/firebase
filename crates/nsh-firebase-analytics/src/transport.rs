//! The pluggable transports: the trait the queue sends through, the
//! in-memory recorder tests (and the demo's console) use, the GA4
//! Measurement Protocol transport, and the collector transport that
//! posts NDJSON to `tools/collector`. The HTTP ones go through the
//! engine's `fetch` (`nsh_firebase_core::send`), so they must be driven
//! from the engine thread (a component task's `spawn`).

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use nsh_core::platform::HttpRequest;
use nsh_firebase_core::{FirebaseError, error::NETWORK, join_url, send};

use crate::model::Batch;

/// One batch, at-most-once to the caller but retried by the queue on
/// failure. The future is deliberately not `Send`: the engine's `fetch`
/// is single-threaded, so `send` (like [`nsh_firebase_core::send`]) must
/// be awaited on the engine thread, never spawned across threads or
/// blocked on.
pub trait Transport: Send + Sync {
    fn send<'a>(
        &'a self,
        batch: &'a Batch,
    ) -> Pin<Box<dyn Future<Output = Result<(), FirebaseError>> + 'a>>;
}

/// Records every batch it is handed. The unit tests' transport, and a
/// convenient stand-in for a console sink.
#[derive(Default)]
pub struct InMemoryTransport {
    batches: Mutex<Vec<Batch>>,
}

impl InMemoryTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn batches(&self) -> Vec<Batch> {
        self.batches
            .lock()
            .expect("in-memory transport lock")
            .clone()
    }

    pub fn clear(&self) {
        self.batches
            .lock()
            .expect("in-memory transport lock")
            .clear();
    }
}

impl Transport for InMemoryTransport {
    fn send<'a>(
        &'a self,
        batch: &'a Batch,
    ) -> Pin<Box<dyn Future<Output = Result<(), FirebaseError>> + 'a>> {
        Box::pin(async move {
            self.batches
                .lock()
                .expect("in-memory transport lock")
                .push(batch.clone());
            Ok(())
        })
    }
}

/// The collector transport: NDJSON lines (one event per line) to
/// `tools/collector`'s `/collect`, the sandbox's assertion sink. No
/// Firebase account, no GA4: the collector asserts payloads locally.
pub struct CollectorTransport {
    endpoint: String,
}

impl CollectorTransport {
    /// `endpoint` is the collector's base, e.g.
    /// `http://127.0.0.1:18742`.
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }
}

/// The request the collector transport makes: POST `<endpoint>/collect`
/// with one JSON object per event, newline-delimited.
pub fn collector_request(endpoint: &str, batch: &Batch) -> HttpRequest {
    HttpRequest::post(
        join_url(endpoint, "collect"),
        batch.to_collector_lines().into_bytes(),
    )
    .header("Content-Type", "application/x-ndjson")
}

/// The GA4 Measurement Protocol transport. Honest limits, per
/// docs/research.md §3: it delivers *events* only — no sessions, no
/// automatic IDFA/GAID or geo/device metadata (that joins only through
/// an `app_instance_id` the native SDK generated), no rule-based event
/// creation or renaming, and no audiences that rely on automatic
/// collection. Requests are capped at 25 events and ~130 KB; this queue
/// batches 20 to stay under both. `timestamp_micros` may not be more
/// than 72 hours in the past.
pub struct MeasurementProtocolTransport {
    base_url: String,
    api_secret: String,
    measurement_id: String,
}

impl MeasurementProtocolTransport {
    /// `base_url` defaults to `https://www.google-analytics.com`; the
    /// sandbox points it at a local stand-in instead.
    pub fn new(
        base_url: impl Into<String>,
        api_secret: impl Into<String>,
        measurement_id: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_secret: api_secret.into(),
            measurement_id: measurement_id.into(),
        }
    }
}

/// The MP collect URL: the secret and the measurement id travel in the
/// query, as the protocol reference specifies, percent-encoded so a
/// credential containing `&` or `=` cannot corrupt the URL. TS's
/// `encodeURIComponent` leaves a few sub-delims raw; credentials made of
/// alphanumerics encode identically on both tiers.
pub fn mp_collect_url(base_url: &str, api_secret: &str, measurement_id: &str) -> String {
    format!(
        "{}/mp/collect?api_secret={}&measurement_id={}",
        base_url.trim_end_matches('/'),
        percent_encode_query(api_secret),
        percent_encode_query(measurement_id)
    )
}

/// Percent-encodes every byte the query cannot carry raw (everything
/// outside RFC 3986's unreserved set), without pulling a crate.
fn percent_encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The request the MP transport makes.
pub fn mp_collect_request(
    base_url: &str,
    api_secret: &str,
    measurement_id: &str,
    batch: &Batch,
) -> HttpRequest {
    HttpRequest::post(
        mp_collect_url(base_url, api_secret, measurement_id),
        batch.to_mp_body().to_string().into_bytes(),
    )
    .header("Content-Type", "application/json")
}

/// 2xx is acceptance; anything else is a network-coded failure the queue
/// retries with backoff.
fn check_status(status: u16, what: &str) -> Result<(), FirebaseError> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(FirebaseError::new(
            NETWORK,
            format!("{what} responded with HTTP {status}"),
        ))
    }
}

impl Transport for CollectorTransport {
    fn send<'a>(
        &'a self,
        batch: &'a Batch,
    ) -> Pin<Box<dyn Future<Output = Result<(), FirebaseError>> + 'a>> {
        let request = collector_request(&self.endpoint, batch);
        Box::pin(async move {
            let response = send(request).await?;
            check_status(response.status(), "the collector")
        })
    }
}

impl Transport for MeasurementProtocolTransport {
    fn send<'a>(
        &'a self,
        batch: &'a Batch,
    ) -> Pin<Box<dyn Future<Output = Result<(), FirebaseError>> + 'a>> {
        let request = mp_collect_request(
            &self.base_url,
            &self.api_secret,
            &self.measurement_id,
            batch,
        );
        Box::pin(async move {
            let response = send(request).await?;
            check_status(response.status(), "the Measurement Protocol endpoint")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AnalyticsEvent;
    use serde_json::{Value as JsonValue, json};

    fn batch() -> Batch {
        Batch {
            client_id: "cid-1".to_owned(),
            user_id: Some("u-1".to_owned()),
            user_properties: json!({"difficulty": {"value": "hard"}}),
            non_personalized_ads: true,
            events: vec![AnalyticsEvent::new("level_up", json!({"level": 3}), 5_000)],
        }
    }

    #[test]
    fn the_collector_request_is_ndjson_to_collect() {
        let request = collector_request("http://127.0.0.1:18742/", &batch());
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, "http://127.0.0.1:18742/collect");
        assert_eq!(
            request.headers,
            vec![("Content-Type".to_owned(), "application/x-ndjson".to_owned())]
        );
        let body = String::from_utf8(match request.body {
            nsh_core::platform::HttpBody::Bytes(bytes) => bytes,
            _ => panic!("the collector request carries bytes"),
        })
        .unwrap();
        assert!(body.contains("\"name\":\"level_up\""));
        assert!(body.contains("\"client_id\":\"cid-1\""));
        assert!(body.contains("\"user_id\":\"u-1\""));
    }

    #[test]
    fn the_mp_request_carries_the_secret_and_id_in_the_query() {
        let request = mp_collect_request(
            "https://www.google-analytics.com",
            "secret-1",
            "measure-1",
            &batch(),
        );
        assert_eq!(
            request.url,
            "https://www.google-analytics.com/mp/collect?api_secret=secret-1&measurement_id=measure-1"
        );
        assert_eq!(
            request.headers,
            vec![("Content-Type".to_owned(), "application/json".to_owned())]
        );
        let body = String::from_utf8(match request.body {
            nsh_core::platform::HttpBody::Bytes(bytes) => bytes,
            _ => panic!("the MP request carries bytes"),
        })
        .unwrap();
        let parsed: JsonValue = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["client_id"], "cid-1");
        assert_eq!(parsed["user_id"], "u-1");
        assert_eq!(parsed["non_personalized_ads"], true);
        assert_eq!(parsed["events"][0]["timestamp_micros"], 5_000_000);
    }

    #[test]
    fn mp_query_values_are_percent_encoded() {
        let url = mp_collect_url("https://www.google-analytics.com", "ab&c=x", "m=1");
        assert_eq!(
            url,
            "https://www.google-analytics.com/mp/collect?api_secret=ab%26c%3Dx&measurement_id=m%3D1"
        );
    }

    #[tokio::test]
    async fn the_in_memory_transport_records_batches() {
        let transport = InMemoryTransport::new();
        transport.send(&batch()).await.unwrap();
        transport.send(&batch()).await.unwrap();
        assert_eq!(transport.batches().len(), 2);
        transport.clear();
        assert!(transport.batches().is_empty());
    }
}
