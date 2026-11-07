//! The labelled sandbox test against the Realtime Database emulator on
//! 127.0.0.1:19000 (project `demo-nsh`, started by
//! `bash scripts/start-emulator.sh`, run by `bash scripts/test-emulator.sh`):
//! a write, a read back, a subscribed listener, and a live update written
//! by a second client through the emulator's real SSE stream. This file
//! talks to the network for real; nothing in it runs under the headless
//! unit gates.

use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use nsh_core::{JsonValue, platform::HttpRequest, prelude::*};
use nsh_engine::{EngineConfig, FetchEvent, FetchEvents, FetchTask, Network, Root, net::network};
use nsh_firebase_core::{FirebaseOptions, initialize_app};
use nsh_firebase_database::{
    Database, ListenStatus, StreamSink, StreamTask, StreamTransport, listener::ThreadReconnect,
};
use nsh_testing::Headless;
use serde_json::json;

const BASE: &str = "http://127.0.0.1:19000";

/// The engine's network stack as an SSE transport: the fetch thread
/// feeds the sink, which is Sync, on whatever thread the engine's
/// network reads from.
struct EngineSse {
    network: Arc<dyn Network>,
}

impl StreamTransport for EngineSse {
    fn open(&self, request: HttpRequest, sink: Arc<dyn StreamSink>) -> Box<dyn StreamTask> {
        let events: FetchEvents = Box::new(move |event| match event {
            FetchEvent::Head(head) => sink.head(head.status),
            FetchEvent::Data(chunk) => sink.data(chunk),
            FetchEvent::End => sink.end(),
            FetchEvent::Failed(message) => sink.failed(message),
            FetchEvent::BodyWanted => {}
        });
        let task = self.network.fetch(request, events);
        Box::new(EngineTask(task))
    }
}

struct EngineTask(Box<dyn FetchTask>);

impl StreamTask for EngineTask {
    fn cancel(self: Box<Self>) {
        self.0.cancel();
    }
}

/// One blocking request straight over the network stack: the second
/// client, which never touches the app.
type SecondClientReply = (u16, String);

fn blocking_request(network: &Arc<dyn Network>, request: HttpRequest) -> SecondClientReply {
    struct Collect {
        status: u16,
        body: Vec<u8>,
        done: bool,
    }
    let state = Arc::new(Mutex::new(Collect {
        status: 0,
        body: Vec::new(),
        done: false,
    }));
    let (tx, rx) = mpsc::channel();
    let sink = Arc::clone(&state);
    let events: FetchEvents = Box::new(move |event| {
        let mut collect = sink.lock().unwrap();
        match event {
            FetchEvent::Head(head) => collect.status = head.status,
            FetchEvent::Data(chunk) => collect.body.extend_from_slice(&chunk),
            FetchEvent::End | FetchEvent::Failed(_) => {
                collect.done = true;
                drop(collect);
                let _ = tx.send(());
            }
            FetchEvent::BodyWanted => {}
        }
    });
    let mut task = network.fetch(request, events);
    task.finish();
    rx.recv_timeout(Duration::from_secs(30))
        .expect("the emulator answered in time");
    let collect = state.lock().unwrap();
    (
        collect.status,
        String::from_utf8_lossy(&collect.body).into_owned(),
    )
}

/// One entry of what the listener saw.
type WatchedEntry = (Option<JsonValue>, ListenStatus);

/// What the listener saw, filled from the transport's threads.
#[derive(Clone, Default)]
struct Watched {
    entries: Arc<Mutex<Vec<WatchedEntry>>>,
}

impl Watched {
    fn listener(self) -> nsh_firebase_database::Listener {
        Arc::new(move |value, status| {
            self.entries.lock().unwrap().push((value.cloned(), status));
        })
    }

    fn latest(&self) -> (Option<JsonValue>, ListenStatus) {
        self.entries
            .lock()
            .unwrap()
            .last()
            .cloned()
            .unwrap_or((None, ListenStatus::Connecting))
    }
}

fn show(value: Option<&JsonValue>) -> String {
    value
        .map(JsonValue::to_string)
        .unwrap_or_else(|| "none".to_owned())
}

#[component]
fn SandboxApp() -> impl IntoElement {
    let app = initialize_app(FirebaseOptions::demo(), "sandbox-db").unwrap();
    let db = Database::from_app(&app, Arc::new(EngineSse { network: network() })).unwrap();
    let write_line = signal("write: idle".to_owned());
    let read_line = signal("read: idle".to_owned());
    // The listener reports off the transport's threads; the display
    // polls it on the engine, which owns the signals.
    let watched = Watched::default();
    let poll = watched.clone();
    let watch_line = signal("watch: idle".to_owned());
    let watch_line_state = watch_line;
    nsh_core::prelude::set_interval(Duration::from_millis(100), move || {
        let (value, status) = poll.latest();
        watch_line_state.set(format!(
            "watch: {} {}",
            status.label(),
            show(value.as_ref())
        ));
    });
    db.r#ref("sandbox-db/watch")
        .listen(Arc::new(ThreadReconnect), watched.listener())
        .expect("the sandbox listener subscribes");
    let (write_db, write_line_state) = (db.r#ref("sandbox-db/rust"), write_line);
    let (read_db, read_line_state) = (db.r#ref("sandbox-db/rust"), read_line);
    view()
        .flex_1()
        .gap_1()
        .child(text(move || write_line.get()))
        .child(text(move || read_line.get()))
        .child(text(move || watch_line_state.get()))
        .child(
            view()
                .test_id("write")
                .on_press(move || {
                    let r#ref = write_db.clone();
                    let line = write_line_state;
                    spawn(async move {
                        match r#ref.set(&json!("from-rust")).await {
                            Ok(()) => line.set("write: ok".to_owned()),
                            Err(error) => line.set(format!("write: {error}")),
                        }
                    });
                })
                .child(text("write")),
        )
        .child(
            view()
                .test_id("read")
                .on_press(move || {
                    let r#ref = read_db.clone();
                    let line = read_line_state;
                    spawn(async move {
                        match r#ref.get().await {
                            Ok(value) => line.set(format!("read: {}", show(value.as_ref()))),
                            Err(error) => line.set(format!("read: {error}")),
                        }
                    });
                })
                .child(text("read")),
        )
}

/// The app's engine: the real network stack behind `fetch` (the REST
/// calls) and the system's clock (a sandbox run keeps real time).
fn config_of(root: Root) -> EngineConfig {
    let mut config = EngineConfig::new(root);
    config.network = Some(network());
    config
}

#[ignore = "sandbox: needs emulator suite"]
#[test]
fn writes_reads_and_receives_a_live_update_against_the_emulator() {
    let net = network();
    // The second client clears both paths first, so the listener's
    // initial put is deterministic.
    let (status, _) = blocking_request(
        &net,
        HttpRequest::new("DELETE", format!("{BASE}/sandbox-db.json")),
    );
    assert!(
        (200..300).contains(&status),
        "the emulator accepted the cleanup DELETE: {status}"
    );

    let mut app = Headless::start(config_of(Root::of::<SandboxApp>())).unwrap();
    app.wait_for("text:write: idle").unwrap();

    // Write and read back through the REST API.
    app.press("id:write").unwrap();
    app.wait_for("text:write: ok").unwrap();
    app.press("id:read").unwrap();
    app.wait_for("text:read: \"from-rust\"").unwrap();

    // The listener connected and got the initial put (an absent path:
    // null), live on the emulator's event stream.
    app.wait_for("text:/^watch: live none/").unwrap();

    // The second client writes the watched path; the live update lands
    // in the app without any poll of the REST API.
    let (status, _) = blocking_request(
        &net,
        HttpRequest::new("PUT", format!("{BASE}/sandbox-db/watch.json")).body("\"second-client\""),
    );
    assert!(
        (200..300).contains(&status),
        "the emulator accepted the second client's PUT: {status}"
    );
    app.wait_for("text:watch: live \"second-client\"").unwrap();
}
