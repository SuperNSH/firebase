//! Performance Monitoring: traces and HTTP metrics recorded on the
//! engine's clock, kept in a local ring, and delivered through a
//! pluggable transport — or through the native tier when
//! `firebase.perf.*` handlers answer. The native shapes are fixed by
//! coordinator decision (see docs/status/perfmon.md):
//!   firebase.perf.startTrace({name}) -> {id: string}
//!   firebase.perf.stopTrace({id})
//!   firebase.perf.recordHttpMetric(<a NetworkEvent, as the args object>)
//! The TypeScript mirror is `packages/nsh-firebase/src/perf`.

use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

use nsh_core::{platform::HttpRequest, time};
use nsh_firebase_core::{FirebaseError, FirebaseHost};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};

use crate::event::NetworkEvent;

/// The local ring keeps the last 500 trace and HTTP-metric records.
const RECORDS_CAPACITY: usize = 500;

/// The name of the throwaway trace the first native call probes with: it
/// tells "handlers exist" apart from "a host exists but grants nothing",
/// which posts alone cannot (an unhandled post is dropped silently).
pub const PROBE_TRACE_NAME: &str = "__nsh_probe__";

/// One delivered record: a stopped trace, or an HTTP metric built from a
/// [`NetworkEvent`]. The JSON is one collector line; the `http` record's
/// NetworkEvent fields sit flat beside `type` so lines stay greppable by
/// host and path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PerfRecord {
    Trace(TraceRecord),
    Http {
        #[serde(flatten)]
        event: NetworkEvent,
    },
}

/// A stopped trace, as the transport sends it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceRecord {
    pub name: String,
    pub started_at_ms: i64,
    pub duration_ms: f64,
    pub metrics: BTreeMap<String, f64>,
    pub attributes: BTreeMap<String, String>,
}

/// Where records go in the pure tier. The analytics products use the same
/// shape: one `send` per batch of records, resolving when they are
/// accepted.
/// The engine's `fetch` lives on the engine thread (its future is not
/// `Send`), so a transport's futures run there too: await them in a
/// component's task or a `spawn`.
pub trait PerfTransport: Send + Sync {
    fn send<'a>(
        &'a self,
        records: Vec<PerfRecord>,
    ) -> Pin<Box<dyn Future<Output = Result<(), FirebaseError>> + 'a>>;

    /// Delivers whatever the transport queued; a non-batching transport
    /// has nothing to flush.
    fn flush<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<(), FirebaseError>> + 'a>> {
        Box::pin(async move { Ok(()) })
    }
}

/// Queues records and delivers them through `inner` once `max_batch` are
/// waiting, or when [`PerfTransport::flush`] is called. A send that
/// reaches the batch size flushes before it returns.
pub struct BatchingTransport {
    inner: Arc<dyn PerfTransport>,
    max_batch: usize,
    queue: Mutex<Vec<PerfRecord>>,
}

impl BatchingTransport {
    pub fn new(inner: Arc<dyn PerfTransport>, max_batch: usize) -> Self {
        Self {
            inner,
            max_batch: max_batch.max(1),
            queue: Mutex::new(Vec::new()),
        }
    }

    /// What is still queued.
    pub fn queued(&self) -> Vec<PerfRecord> {
        self.queue.lock().expect("transport queue lock").clone()
    }
}

impl PerfTransport for BatchingTransport {
    fn send<'a>(
        &'a self,
        mut records: Vec<PerfRecord>,
    ) -> Pin<Box<dyn Future<Output = Result<(), FirebaseError>> + 'a>> {
        Box::pin(async move {
            let full = {
                let mut queue = self.queue.lock().expect("transport queue lock");
                queue.append(&mut records);
                queue.len() >= self.max_batch
            };
            if full { self.flush().await } else { Ok(()) }
        })
    }

    fn flush<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<(), FirebaseError>> + 'a>> {
        Box::pin(async move {
            let drained: Vec<PerfRecord> =
                std::mem::take(&mut *self.queue.lock().expect("transport queue lock"));
            if drained.is_empty() {
                return Ok(());
            }
            self.inner.send(drained).await
        })
    }
}

/// Posts each batch as newline-separated JSON records (`application/x-ndjson`)
/// to `url` — the shape `tools/collector` appends to its sink. This is
/// the sandbox's stand-in for Performance's own ingestion, which has no
/// public REST to call.
pub struct HttpLineTransport {
    url: String,
}

impl HttpLineTransport {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }
}

impl PerfTransport for HttpLineTransport {
    fn send<'a>(
        &'a self,
        records: Vec<PerfRecord>,
    ) -> Pin<Box<dyn Future<Output = Result<(), FirebaseError>> + 'a>> {
        Box::pin(async move {
            if records.is_empty() {
                return Ok(());
            }
            let body = records
                .iter()
                .map(|record| serde_json::to_string(record).expect("a record serializes"))
                .collect::<Vec<_>>()
                .join("\n");
            let request = HttpRequest::post(&self.url, body.into_bytes())
                .header("content-type", "application/x-ndjson");
            let response = nsh_firebase_core::send(request).await?;
            if response.ok() {
                Ok(())
            } else {
                Err(FirebaseError::new(
                    nsh_firebase_core::error::NETWORK,
                    format!("the sink answered {}", response.status()),
                ))
            }
        })
    }
}

/// The name, the clock-time start and the metrics of a running trace.
struct TraceState {
    name: String,
    started_at_ms: i64,
    start: time::Instant,
    metrics: BTreeMap<String, f64>,
    attributes: BTreeMap<String, String>,
    native_id: Option<String>,
}

/// One trace: `put_metric` and `put_attribute` accumulate, `stop` closes
/// it on the clock and delivers it. Hold it as an `Arc<Trace>`; `stop`
/// consumes it, so a trace cannot stop twice.
pub struct Trace {
    perfmon: Arc<Perfmon>,
    state: Mutex<TraceState>,
}

impl Trace {
    /// Puts a metric (a named duration or count; the last put wins).
    pub fn put_metric(&self, name: impl Into<String>, value: f64) {
        self.state
            .lock()
            .expect("trace lock")
            .metrics
            .insert(name.into(), value);
    }

    /// Puts an attribute (a named string detail; the last put wins).
    pub fn put_attribute(&self, name: impl Into<String>, value: impl Into<String>) {
        self.state
            .lock()
            .expect("trace lock")
            .attributes
            .insert(name.into(), value.into());
    }

    /// Closes the trace: its duration is the clock's time since
    /// [`Perfmon::start_trace`]. The record joins the local ring and is
    /// delivered to the native tier (the `stopTrace` call) or the
    /// transport. On the native tier only the trace's name and duration
    /// cross: the fixed `firebase.perf.*` handlers have no put-metric
    /// call, so metrics and attributes stay local (documented in
    /// docs/status/perfmon.md).
    pub async fn stop(self: Arc<Self>) -> Result<TraceRecord, FirebaseError> {
        // The guard's scope ends here, before any await: a listener or a
        // task reading the trace must not find it held.
        let (record, native_id) = {
            let mut state = self.state.lock().expect("trace lock");
            let duration_ms = time::now()
                .saturating_duration_since(state.start)
                .as_secs_f64()
                * 1000.0;
            let record = TraceRecord {
                name: state.name.clone(),
                started_at_ms: state.started_at_ms,
                duration_ms,
                metrics: state.metrics.clone(),
                attributes: state.attributes.clone(),
            };
            state.metrics.clear();
            state.attributes.clear();
            (record, state.native_id.clone())
        };

        self.perfmon.push_record(PerfRecord::Trace(record.clone()));
        if let Some(id) = native_id {
            self.perfmon
                .host
                .call("firebase.perf.stopTrace", json!({ "id": id }))
                .await?;
            return Ok(record);
        }
        if let Some(transport) = &self.perfmon.transport {
            transport
                .send(vec![PerfRecord::Trace(record.clone())])
                .await?;
        }
        Ok(record)
    }
}

/// The Performance Monitoring facade: starts traces and records HTTP
/// metrics, keeps the last records locally, and delivers each to the
/// native tier when `firebase.perf.*` handlers exist (probed once) or to
/// the pluggable transport otherwise.
pub struct Perfmon {
    host: Arc<dyn FirebaseHost>,
    transport: Option<Arc<dyn PerfTransport>>,
    /// The probe's answer, remembered: `None` until the first record.
    native: Mutex<Option<bool>>,
    records: Mutex<VecDeque<PerfRecord>>,
}

impl Perfmon {
    /// A facade for `host` (the app's tier: `NativeHost` or `PureHost`),
    /// delivering pure-tier records through `transport` when one is
    /// attached.
    pub fn new(
        host: Arc<dyn FirebaseHost>,
        transport: Option<Arc<dyn PerfTransport>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            host,
            transport,
            native: Mutex::new(None),
            records: Mutex::new(VecDeque::with_capacity(RECORDS_CAPACITY)),
        })
    }

    /// Starts a trace named `name`. On the native tier the handler gives
    /// the trace's id, and [`Trace::stop`] stops it there; a pure-tier
    /// trace is delivered through the transport at `stop`.
    pub async fn start_trace(self: &Arc<Self>, name: &str) -> Result<Arc<Trace>, FirebaseError> {
        let native_id = if self.native_available().await {
            let answer = self
                .host
                .call("firebase.perf.startTrace", json!({ "name": name }))
                .await?;
            Some(trace_id(&answer)?)
        } else {
            None
        };
        Ok(Arc::new(Trace {
            perfmon: Arc::clone(self),
            state: Mutex::new(TraceState {
                name: name.to_owned(),
                started_at_ms: time::unix_ms() as i64,
                start: time::now(),
                metrics: BTreeMap::new(),
                attributes: BTreeMap::new(),
                native_id,
            }),
        }))
    }

    /// Builds an HTTP metric from a monitor event and records it. The
    /// event joins the local ring either way; the native tier gets it as
    /// the `recordHttpMetric` post, the pure tier through the transport.
    pub async fn record_http_metric(&self, event: NetworkEvent) -> Result<(), FirebaseError> {
        let native = self.native_available().await;
        self.push_record(PerfRecord::Http {
            event: event.clone(),
        });
        if native {
            let args = serde_json::to_value(&event).map_err(|error| {
                FirebaseError::new(
                    nsh_firebase_core::error::INVALID_ARGUMENT,
                    error.to_string(),
                )
            })?;
            self.host.post("firebase.perf.recordHttpMetric", args)?;
            return Ok(());
        }
        if let Some(transport) = &self.transport {
            transport.send(vec![PerfRecord::Http { event }]).await?;
        }
        Ok(())
    }

    /// The last records, oldest first (traces and HTTP metrics mixed).
    pub fn records(&self) -> Vec<PerfRecord> {
        self.records
            .lock()
            .expect("perfmon records lock")
            .iter()
            .cloned()
            .collect()
    }

    /// Flushes the transport's queue (a no-op without a batching one).
    pub async fn flush(&self) -> Result<(), FirebaseError> {
        match &self.transport {
            Some(transport) => transport.flush().await,
            None => Ok(()),
        }
    }

    /// Whether `firebase.perf.*` handlers answer. Probed once with a
    /// throwaway `startTrace` (stopped again when it works): a host that
    /// grants nothing must not swallow records into unhandled posts,
    /// which the runtime drops silently. A startTrace that answers Ok
    /// without a string id is a broken handler, not a working native
    /// tier — treated as absent, like the TypeScript probe treats it.
    async fn native_available(&self) -> bool {
        let cached = *self.native.lock().expect("perfmon native lock");
        if let Some(known) = cached {
            return known;
        }
        let answer = self
            .host
            .call(
                "firebase.perf.startTrace",
                json!({ "name": PROBE_TRACE_NAME }),
            )
            .await;
        let probe_id = answer
            .as_ref()
            .ok()
            .and_then(|answer| trace_id(answer).ok());
        if let Some(id) = &probe_id {
            let _ = self
                .host
                .call("firebase.perf.stopTrace", json!({ "id": id }))
                .await;
        }
        let available = probe_id.is_some();
        *self.native.lock().expect("perfmon native lock") = Some(available);
        available
    }

    fn push_record(&self, record: PerfRecord) {
        let mut records = self.records.lock().expect("perfmon records lock");
        while records.len() >= RECORDS_CAPACITY {
            records.pop_front();
        }
        records.push_back(record);
    }
}

/// The `{id: string}` a `startTrace` handler answers with.
fn trace_id(answer: &JsonValue) -> Result<String, FirebaseError> {
    answer
        .get("id")
        .and_then(|id| id.as_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            FirebaseError::new(
                nsh_firebase_core::error::HOST_ERROR,
                "firebase.perf.startTrace answered without a string id",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, host: &str, duration_ms: f64) -> NetworkEvent {
        NetworkEvent {
            id: id.to_owned(),
            method: "GET".to_owned(),
            url: format!("http://{host}/a"),
            host: host.to_owned(),
            path: "/a".to_owned(),
            status: Some(200),
            started_at_ms: 1,
            duration_ms,
            ttfb_ms: None,
            request_bytes: None,
            response_bytes: None,
            error: None,
            initiator: Some("firebase-fetch".to_owned()),
        }
    }

    #[test]
    fn an_http_record_flattens_the_event_into_one_collector_line() {
        let record = PerfRecord::Http {
            event: event("e1", "api.test", 12.5),
        };
        let value = serde_json::to_value(&record).unwrap();
        assert_eq!(value["type"], "http");
        assert_eq!(value["id"], "e1");
        assert_eq!(value["host"], "api.test");
        assert_eq!(value["durationMs"], 12.5);
        let back: PerfRecord = serde_json::from_value(value).unwrap();
        assert_eq!(back, record);
    }

    #[test]
    fn a_trace_record_names_its_metrics_and_attributes() {
        let record = PerfRecord::Trace(TraceRecord {
            name: "screen-load".to_owned(),
            started_at_ms: 1000,
            duration_ms: 250.0,
            metrics: BTreeMap::from([("first_paint".to_owned(), 80.0)]),
            attributes: BTreeMap::from([("screen".to_owned(), "monitor".to_owned())]),
        });
        let value = serde_json::to_value(&record).unwrap();
        assert_eq!(
            value,
            json!({
                "type": "trace",
                "name": "screen-load",
                "startedAtMs": 1000,
                "durationMs": 250.0,
                "metrics": { "first_paint": 80.0 },
                "attributes": { "screen": "monitor" },
            })
        );
    }

    /// A transport that keeps what it is sent, for assertions.
    #[derive(Default)]
    struct RecordingTransport {
        batches: Mutex<Vec<Vec<PerfRecord>>>,
        flushed: Mutex<Vec<usize>>,
    }

    impl RecordingTransport {
        fn sent(&self) -> Vec<PerfRecord> {
            self.batches
                .lock()
                .unwrap()
                .iter()
                .flatten()
                .cloned()
                .collect()
        }
    }

    impl PerfTransport for RecordingTransport {
        fn send<'a>(
            &'a self,
            records: Vec<PerfRecord>,
        ) -> Pin<Box<dyn Future<Output = Result<(), FirebaseError>> + 'a>> {
            Box::pin(async move {
                self.batches.lock().unwrap().push(records);
                Ok(())
            })
        }

        fn flush<'a>(&'a self) -> Pin<Box<dyn Future<Output = Result<(), FirebaseError>> + 'a>> {
            let count = self.batches.lock().unwrap().len();
            Box::pin(async move {
                self.flushed.lock().unwrap().push(count);
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn the_batching_transport_flushes_at_the_batch_size_and_on_flush() {
        let inner = Arc::new(RecordingTransport::default());
        let batching = BatchingTransport::new(inner.clone(), 2);

        batching
            .send(vec![PerfRecord::Http {
                event: event("e1", "api.test", 1.0),
            }])
            .await
            .unwrap();
        assert!(
            inner.sent().is_empty(),
            "below the batch size nothing is sent"
        );
        assert_eq!(batching.queued().len(), 1);

        batching
            .send(vec![PerfRecord::Http {
                event: event("e2", "api.test", 2.0),
            }])
            .await
            .unwrap();
        // The batch size sends both queued records in one batch.
        assert_eq!(inner.sent().len(), 2);
        assert!(batching.queued().is_empty());

        batching
            .send(vec![PerfRecord::Http {
                event: event("e3", "api.test", 3.0),
            }])
            .await
            .unwrap();
        batching.flush().await.unwrap();
        assert_eq!(inner.sent().len(), 3);
        // A batching transport delivers by sending; the inner transport's
        // own flush stays unused.
        assert!(inner.flushed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_empty_flush_sends_nothing() {
        let inner = Arc::new(RecordingTransport::default());
        let batching = BatchingTransport::new(inner.clone(), 4);
        batching.flush().await.unwrap();
        assert!(inner.sent().is_empty());
        // Nothing to deliver: the inner transport is never reached.
        assert!(inner.flushed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_refused_probe_sends_records_to_the_transport() {
        let transport = Arc::new(RecordingTransport::default());
        let perfmon = Perfmon::new(
            Arc::new(nsh_firebase_core::PureHost),
            Some(transport.clone()),
        );
        perfmon
            .record_http_metric(event("e1", "api.test", 5.0))
            .await
            .unwrap();
        assert_eq!(transport.sent().len(), 1);
        assert_eq!(perfmon.records().len(), 1);
    }

    /// A handler whose startTrace answers Ok without a string id: broken,
    /// not native. The probe must treat it as absent — like the
    /// TypeScript probe does — so records fall back to the transport
    /// instead of failing with HOST_ERROR forever.
    struct IdlessProbeChannel;

    impl nsh_firebase_core::HostChannel for IdlessProbeChannel {
        fn call(
            &self,
            _name: String,
            _args: JsonValue,
        ) -> Pin<Box<dyn Future<Output = Result<JsonValue, FirebaseError>> + Send>> {
            Box::pin(async move { Ok(json!({ "id": 42 })) })
        }

        fn post(&self, name: String, _args: JsonValue) -> Result<(), FirebaseError> {
            // A post here means the facade mistook this host for native.
            panic!("recordHttpMetric must not post over a broken handler: {name}");
        }
    }

    #[tokio::test]
    async fn a_probe_without_a_string_id_falls_back_to_the_transport() {
        let transport = Arc::new(RecordingTransport::default());
        let perfmon = Perfmon::new(
            Arc::new(nsh_firebase_core::NativeHost::new(Arc::new(
                IdlessProbeChannel,
            ))),
            Some(transport.clone()),
        );
        perfmon
            .record_http_metric(event("e1", "api.test", 5.0))
            .await
            .unwrap();
        assert_eq!(transport.sent().len(), 1);
        assert_eq!(perfmon.records().len(), 1);
    }
}
