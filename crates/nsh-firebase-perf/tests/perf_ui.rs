//! Headless UI tests of the monitor and the perf facade, on a manual
//! clock with the stubbed network: the instrumented transport's event
//! fields, the failure event, ring eviction, a trace's clock duration
//! and its flush to the sink transport, and the native-tier routing
//! through the fixed `firebase.perf.*` handlers.

use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use nsh_core::{JsonValue, platform::HttpRequest, prelude::*};
use nsh_engine::{Clock, EngineConfig, Network, Root};
use nsh_firebase_core::{FirebaseError, FirebaseHost, HostChannel, NativeHost, PureHost};
use nsh_firebase_perf::{
    BatchingTransport, HttpLineTransport, Monitor, NetworkEvent, Perfmon, instrumented_send,
};
use nsh_testing::{Headless, StubNetwork, StubResponse};
use serde_json::json;

const TODOS: &str = "http://127.0.0.1:19000/todos.json";
const SINK: &str = "http://127.0.0.1:18742/collect";

fn clock() -> Clock {
    Clock::manual(SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_258_000))
}

/// Starts `root` on `clock` over `network`; the network's response
/// delays pass on that same clock, so the wall's never decides one.
fn start(root: Root, clock: Clock, network: Arc<dyn Network>) -> Headless {
    let mut config = EngineConfig::new(root);
    config.clock = clock;
    config.network = Some(network);
    Headless::start(config).unwrap()
}

fn stub_network(clock: Clock) -> Arc<StubNetwork> {
    let mut response = StubResponse::default();
    response.status = 200;
    response.body = br#"[{"id":1,"title":"one"}]"#.to_vec();
    response.headers = vec![("content-length".to_owned(), "24".to_owned())];
    response.delay = Duration::from_millis(120);
    // Without `with_clock` a stub's delays run on the wall clock.
    Arc::new(StubNetwork::new([(format!("GET {TODOS}"), response)]).with_clock(clock))
}

fn sink_network() -> Arc<StubNetwork> {
    let mut response = StubResponse::default();
    response.status = 200;
    Arc::new(StubNetwork::new([(format!("POST {SINK}"), response)]))
}

fn empty_network() -> Arc<StubNetwork> {
    Arc::new(StubNetwork::new(Vec::<(String, StubResponse)>::new()))
}

fn sample_event(id: &str) -> NetworkEvent {
    NetworkEvent {
        id: id.to_owned(),
        method: "GET".to_owned(),
        url: format!("http://api.test/{id}"),
        host: "api.test".to_owned(),
        path: format!("/{id}"),
        status: Some(200),
        started_at_ms: 1,
        duration_ms: 1.0,
        ttfb_ms: None,
        request_bytes: None,
        response_bytes: None,
        error: None,
        initiator: None,
    }
}

#[component]
fn InstrumentApp() -> impl IntoElement {
    let monitor = Arc::new(Monitor::new());
    let line = signal(String::new());
    let (fetch_monitor, fetch_line) = (Arc::clone(&monitor), line);
    let (summary_monitor, summary_line) = (Arc::clone(&monitor), line);
    view()
        .flex_1()
        .gap_1()
        .child(text(move || summary_line.get()))
        .child(
            view()
                .test_id("fetch")
                .on_press(move || {
                    let (monitor, line) = (Arc::clone(&fetch_monitor), fetch_line);
                    spawn(async move {
                        match instrumented_send(&monitor, HttpRequest::get(TODOS)).await {
                            Ok(response) => {
                                line.set(format!("ok status={} body={}", response.status(), response.text()))
                            }
                            Err(error) => line.set(format!("error: {error}")),
                        }
                    });
                })
                .child(text("fetch")),
        )
        .child(
            view()
                .test_id("summary")
                .on_press(move || {
                    let monitor = Arc::clone(&summary_monitor);
                    let event = monitor.events().pop();
                    summary_line.set(match event {
                        Some(event) => format!(
                            "event: {} {} host={} path={} status={:?} dur={:.1} req={:?} res={:?} err={:?} initiator={:?}",
                            event.method,
                            event.url,
                            event.host,
                            event.path,
                            event.status,
                            event.duration_ms,
                            event.request_bytes,
                            event.response_bytes,
                            event.error,
                            event.initiator,
                        ),
                        None => "event: none".to_owned(),
                    });
                })
                .child(text("summary")),
        )
}

#[test]
fn an_instrumented_fetch_records_every_field_on_the_manual_clock() {
    let clock = clock();
    let mut app = start(Root::of::<InstrumentApp>(), clock.clone(), stub_network(clock));
    app.wait_for("text:fetch").unwrap();
    app.press("id:fetch").unwrap();
    // The response's 120 ms delay passes when the clock moves past it,
    // so the recorded duration is exactly the stub's.
    app.fast_forward(Duration::from_millis(120)).unwrap();
    app.wait_for(r#"text:ok status=200 body=[{"id":1,"title":"one"}]"#)
        .unwrap();
    app.press("id:summary").unwrap();
    app.wait_for(
        r#"text:event: GET http://127.0.0.1:19000/todos.json host=127.0.0.1:19000 path=/todos.json status=Some(200) dur=120.0 req=None res=Some(24) err=None initiator=Some("firebase-fetch")"#,
    )
    .unwrap();
}

#[test]
fn an_unrouted_request_records_an_error_event() {
    let mut app = start(Root::of::<InstrumentApp>(), clock(), empty_network());
    app.wait_for("text:fetch").unwrap();
    app.press("id:fetch").unwrap();
    app.wait_for("text:/error: core\\/network/").unwrap();
    app.press("id:summary").unwrap();
    app.wait_for("text:/status=None dur=[0-9.]+ req=None res=None err=Some/")
        .unwrap();
}

#[component]
fn RingApp() -> impl IntoElement {
    let line = signal(String::new());
    let ring_line = line;
    view()
        .flex_1()
        .gap_1()
        .child(text(move || ring_line.get()))
        .child(
            view()
                .test_id("ring")
                .on_press(move || {
                    let monitor = Monitor::with_capacity(3);
                    for n in 1..=5 {
                        monitor.record(sample_event(&format!("e{n}")));
                    }
                    let kept: Vec<String> = monitor
                        .events()
                        .iter()
                        .map(|event| event.id.clone())
                        .collect();
                    ring_line.set(format!("ring: {} kept {}", monitor.len(), kept.join(",")));
                })
                .child(text("ring")),
        )
}

#[test]
fn the_ring_keeps_the_newest_three() {
    let mut app = start(Root::of::<RingApp>(), clock(), empty_network());
    app.wait_for("text:ring").unwrap();
    app.press("id:ring").unwrap();
    app.wait_for("text:ring: 3 kept e3,e4,e5").unwrap();
}

type TraceSlot = Arc<Mutex<Option<Arc<nsh_firebase_perf::Trace>>>>;

static TRACE_STATE: Mutex<Option<(Arc<Perfmon>, TraceSlot)>> = Mutex::new(None);

#[component]
fn TraceApp() -> impl IntoElement {
    let (perfmon, running) = TRACE_STATE
        .lock()
        .expect("trace state")
        .as_ref()
        .expect("set")
        .clone();
    let line = signal(String::new());
    let (start_pm, start_line, start_running) = (Arc::clone(&perfmon), line, Arc::clone(&running));
    let (stop_pm, stop_line, stop_running) = (Arc::clone(&perfmon), line, Arc::clone(&running));
    let (flush_pm, flush_line) = (Arc::clone(&perfmon), line);
    view()
        .flex_1()
        .gap_1()
        .child(text(move || line.get()))
        .child(
            view()
                .test_id("trace-start")
                .on_press(move || {
                    let (perfmon, line, running) = (
                        Arc::clone(&start_pm),
                        start_line,
                        Arc::clone(&start_running),
                    );
                    spawn(async move {
                        match perfmon.start_trace("screen-load").await {
                            Ok(trace) => {
                                trace.put_metric("first_paint", 80.0);
                                trace.put_attribute("screen", "monitor");
                                *running.lock().expect("trace slot") = Some(trace);
                                line.set("trace: started".to_owned());
                            }
                            Err(error) => line.set(format!("trace error: {error}")),
                        }
                    });
                })
                .child(text("trace start")),
        )
        .child(
            view()
                .test_id("trace-stop")
                .on_press(move || {
                    let (perfmon, line, running) =
                        (Arc::clone(&stop_pm), stop_line, Arc::clone(&stop_running));
                    spawn(async move {
                        let Some(trace) = running.lock().expect("trace slot").take() else {
                            line.set("trace error: none".to_owned());
                            return;
                        };
                        match trace.stop().await {
                            Ok(record) => line.set(format!(
                                "trace: {} dur={:.1} fp={} screen={} records={}",
                                record.name,
                                record.duration_ms,
                                record.metrics["first_paint"],
                                record.attributes["screen"],
                                perfmon.records().len(),
                            )),
                            Err(error) => line.set(format!("trace error: {error}")),
                        }
                    });
                })
                .child(text("trace stop")),
        )
        .child(
            view()
                .test_id("flush")
                .on_press(move || {
                    let (perfmon, line) = (Arc::clone(&flush_pm), flush_line);
                    spawn(async move {
                        match perfmon.flush().await {
                            Ok(()) => line.set("flush: ok".to_owned()),
                            Err(error) => line.set(format!("flush error: {error}")),
                        }
                    });
                })
                .child(text("flush")),
        )
}

#[test]
fn a_trace_stops_on_the_clock_and_flushes_one_ndjson_line_to_the_sink() {
    let perfmon = {
        let sink = BatchingTransport::new(Arc::new(HttpLineTransport::new(SINK)), 4);
        Perfmon::new(Arc::new(PureHost), Some(Arc::new(sink)))
    };
    *TRACE_STATE.lock().expect("trace state") =
        Some((Arc::clone(&perfmon), Arc::new(Mutex::new(None))));
    let network = sink_network();
    let mut app = start(Root::of::<TraceApp>(), clock(), network);
    app.wait_for("text:trace start").unwrap();
    app.press("id:trace-start").unwrap();
    app.wait_for("text:trace: started").unwrap();
    app.fast_forward(Duration::from_millis(120)).unwrap();
    app.press("id:trace-stop").unwrap();
    app.wait_for("text:trace: screen-load dur=120.0 fp=80 screen=monitor records=1")
        .unwrap();
    // The record waits in the batching transport until the flush, which
    // posts one ndjson line to the sink.
    app.press("id:flush").unwrap();
    app.wait_for("text:flush: ok").unwrap();
    let state = TRACE_STATE.lock().expect("trace state");
    let perfmon = state.as_ref().expect("set").0.clone();
    drop(state);
    assert_eq!(perfmon.records().len(), 1);
}

/// The test's stand-in for the embedding app: it records every call and
/// post and answers the fixed perf handlers the way the native hosts do.
struct PerfChannel {
    calls: Mutex<Vec<(String, JsonValue)>>,
    posts: Mutex<Vec<(String, JsonValue)>>,
    next_id: AtomicU64,
}

impl PerfChannel {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            posts: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
        })
    }
}

impl HostChannel for PerfChannel {
    fn call(
        &self,
        name: String,
        args: JsonValue,
    ) -> Pin<Box<dyn Future<Output = Result<JsonValue, FirebaseError>> + Send>> {
        self.calls
            .lock()
            .expect("channel calls")
            .push((name.clone(), args));
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move {
            if name == "firebase.perf.startTrace" {
                Ok(json!({ "id": format!("t-{id}") }))
            } else if name == "firebase.perf.stopTrace" {
                Ok(JsonValue::Null)
            } else {
                Err(FirebaseError::from_host_code(
                    "ERR_NSH_HOST_ERROR",
                    format!("`{name}` is not handled"),
                ))
            }
        })
    }

    fn post(&self, name: String, args: JsonValue) -> Result<(), FirebaseError> {
        self.posts.lock().expect("channel posts").push((name, args));
        Ok(())
    }
}

static NATIVE_STATE: Mutex<Option<(Arc<PerfChannel>, Arc<Perfmon>)>> = Mutex::new(None);
static NATIVE_TRACE: Mutex<Option<Arc<nsh_firebase_perf::Trace>>> = Mutex::new(None);

#[component]
fn NativeApp() -> impl IntoElement {
    let (channel, perfmon) = NATIVE_STATE
        .lock()
        .expect("native state")
        .as_ref()
        .expect("set")
        .clone();
    let line = signal(String::new());
    let (metric_channel, metric_pm, metric_line) =
        (Arc::clone(&channel), Arc::clone(&perfmon), line);
    let (start_channel, start_pm, start_line) = (Arc::clone(&channel), Arc::clone(&perfmon), line);
    let (stop_channel, stop_pm, stop_line) = (Arc::clone(&channel), Arc::clone(&perfmon), line);
    view()
        .flex_1()
        .gap_1()
        .child(text(move || line.get()))
        .child(
            view()
                .test_id("metric")
                .on_press(move || {
                    let (channel, perfmon, line) = (
                        Arc::clone(&metric_channel),
                        Arc::clone(&metric_pm),
                        metric_line,
                    );
                    spawn(async move {
                        let mut event = sample_event("evt-1");
                        event.host = "127.0.0.1:19000".to_owned();
                        event.path = "/todos.json".to_owned();
                        event.url = TODOS.to_owned();
                        event.duration_ms = 120.0;
                        event.ttfb_ms = Some(40.0);
                        event.response_bytes = Some(24);
                        event.initiator = Some("firebase-fetch".to_owned());
                        match perfmon.record_http_metric(event).await {
                            Ok(()) => line.set(format!(
                                "metric: posts={} records={}",
                                channel.posts.lock().expect("posts").len(),
                                perfmon.records().len(),
                            )),
                            Err(error) => line.set(format!("metric error: {error}")),
                        }
                    });
                })
                .child(text("metric")),
        )
        .child(
            view()
                .test_id("trace-start")
                .on_press(move || {
                    let (channel, perfmon, line) = (
                        Arc::clone(&start_channel),
                        Arc::clone(&start_pm),
                        start_line,
                    );
                    spawn(async move {
                        match perfmon.start_trace("screen-load").await {
                            Ok(trace) => {
                                *NATIVE_TRACE.lock().expect("native trace") = Some(trace);
                                line.set(format!(
                                    "trace: calls={} started",
                                    channel.calls.lock().expect("calls").len(),
                                ));
                            }
                            Err(error) => line.set(format!("trace error: {error}")),
                        }
                    });
                })
                .child(text("native trace start")),
        )
        .child(
            view()
                .test_id("trace-stop")
                .on_press(move || {
                    let (channel, perfmon, line) =
                        (Arc::clone(&stop_channel), Arc::clone(&stop_pm), stop_line);
                    spawn(async move {
                        let Some(trace) = NATIVE_TRACE.lock().expect("native trace").take() else {
                            line.set("trace error: none".to_owned());
                            return;
                        };
                        match trace.stop().await {
                            Ok(record) => line.set(format!(
                                "stopped: {} dur={:.1} calls={} posts={}",
                                record.name,
                                record.duration_ms,
                                channel.calls.lock().expect("calls").len(),
                                channel.posts.lock().expect("posts").len(),
                            )),
                            Err(error) => line.set(format!("trace error: {error}")),
                        }
                    });
                })
                .child(text("native trace stop")),
        )
}

#[test]
fn the_native_tier_takes_the_trace_and_the_http_metric_through_the_fixed_handlers() {
    let channel = PerfChannel::new();
    let perfmon = Perfmon::new(
        Arc::new(NativeHost::new(channel.clone())) as Arc<dyn FirebaseHost>,
        None,
    );
    *NATIVE_STATE.lock().expect("native state") =
        Some((Arc::clone(&channel), Arc::clone(&perfmon)));
    let mut app = start(Root::of::<NativeApp>(), clock(), empty_network());
    app.wait_for("text:metric").unwrap();

    // The HTTP metric crosses as the recordHttpMetric post, its args
    // object the event itself.
    app.press("id:metric").unwrap();
    app.wait_for("text:metric: posts=1 records=1").unwrap();
    let posts = channel.posts.lock().expect("posts").clone();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].0, "firebase.perf.recordHttpMetric");
    assert_eq!(posts[0].1["id"], "evt-1");
    assert_eq!(posts[0].1["host"], "127.0.0.1:19000");
    assert_eq!(posts[0].1["durationMs"], 120.0);

    // The probe made 2 calls (a throwaway startTrace and its stop),
    // then the trace's own start is the third.
    app.press("id:trace-start").unwrap();
    app.wait_for("text:trace: calls=3 started").unwrap();
    let calls = channel.calls.lock().expect("calls").clone();
    assert_eq!(calls[0].0, "firebase.perf.startTrace");
    assert_eq!(calls[0].1["name"], nsh_firebase_perf::PROBE_TRACE_NAME);
    assert_eq!(calls[1].0, "firebase.perf.stopTrace");
    assert_eq!(calls[2].0, "firebase.perf.startTrace");
    assert_eq!(calls[2].1["name"], "screen-load");

    // The stop crosses as its own call, the trace's handler id in hand.
    app.press("id:trace-stop").unwrap();
    app.wait_for("text:stopped: screen-load dur=0.0 calls=4 posts=1")
        .unwrap();
    let calls = channel.calls.lock().expect("calls").clone();
    assert_eq!(calls[3].0, "firebase.perf.stopTrace");
    assert_eq!(calls[3].1["id"], "t-3");
}
