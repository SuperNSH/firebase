//! Headless UI tests of the database: CRUD, push ordering, transaction
//! retry, query encoding and the live listener with its reconnect, all
//! through a test-only component on a manual clock, with a stubbed
//! network (the REST calls) and a scripted stream transport (the SSE
//! connections).

use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

use nsh_core::{JsonValue, platform::HttpRequest, prelude::*};
use nsh_engine::{Clock, EngineConfig, Network, Root};
use nsh_firebase_core::{FirebaseError, FirebaseOptions, initialize_app};
use nsh_firebase_database::{Database, StreamSink, StreamTask, StreamTransport};
use nsh_testing::{Headless, StubHandler, StubNetwork, StubReply, StubResponse, StubRoute};
use serde_json::json;

/// The manual clock's start: the same wall time every test uses, so the
/// push ids are deterministic.
fn clock_start() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_258_000)
}

fn start_with_network(root: Root, network: Arc<StubNetwork>) -> Headless {
    let mut config = EngineConfig::new(root);
    config.clock = Clock::manual(clock_start());
    config.network = Some(Arc::clone(&network) as Arc<dyn Network>);
    Headless::start(config).unwrap()
}

/// Formats a value the way the screens show it.
fn show(value: Option<&JsonValue>) -> String {
    value
        .map(JsonValue::to_string)
        .unwrap_or_else(|| "none".to_owned())
}

/// Formats a failed call the way the screens show it.
fn failed(error: FirebaseError) -> String {
    error.to_string()
}

// --- The CRUD app ------------------------------------------------------------

#[component]
fn CrudApp() -> impl IntoElement {
    let app = initialize_app(FirebaseOptions::demo(), "db-ui").unwrap();
    let db = Database::from_app(&app, never_transport()).unwrap();
    let line = signal(String::new());
    let last_push = signal(String::new());
    view()
        .flex_1()
        .gap_1()
        .child(text(move || line.get()))
        .child(
            view()
                .test_id("set")
                .on_press({
                    let r#ref = db.r#ref("ui-todos").child("one");
                    move || {
                        let r#ref = r#ref.clone();
                        spawn(async move {
                            match r#ref.set(&json!({"done": false, "title": "one"})).await {
                                Ok(()) => line.set("set: ok".to_owned()),
                                Err(error) => line.set(format!("set: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("set")),
        )
        .child(
            view()
                .test_id("get")
                .on_press({
                    let r#ref = db.r#ref("ui-todos").child("one");
                    move || {
                        let r#ref = r#ref.clone();
                        spawn(async move {
                            match r#ref.get().await {
                                Ok(value) => line.set(format!("get: {}", show(value.as_ref()))),
                                Err(error) => line.set(format!("get: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("get")),
        )
        .child(
            view()
                .test_id("update")
                .on_press({
                    let r#ref = db.r#ref("ui-multi");
                    move || {
                        let r#ref = r#ref.clone();
                        spawn(async move {
                            let done = async {
                                r#ref.set(&json!({"a": 1, "b": {"c": 1}})).await?;
                                r#ref.update(&json!({"a": 9, "b/d": 7})).await?;
                                r#ref.get().await
                            };
                            match done.await {
                                Ok(value) => line.set(format!("multi: {}", show(value.as_ref()))),
                                Err(error) => line.set(format!("multi: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("update")),
        )
        .child(
            view()
                .test_id("remove")
                .on_press({
                    let r#ref = db.r#ref("ui-todos").child("one");
                    move || {
                        let r#ref = r#ref.clone();
                        spawn(async move {
                            let removed = async {
                                r#ref.remove().await?;
                                r#ref.get().await
                            };
                            match removed.await {
                                Ok(value) => line.set(format!("removed: {}", show(value.as_ref()))),
                                Err(error) => line.set(format!("removed: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("remove")),
        )
        .child(
            view()
                .test_id("push")
                .on_press({
                    let r#ref = db.r#ref("ui-pushed");
                    move || {
                        let r#ref = r#ref.clone();
                        spawn(async move {
                            let Ok(first) = r#ref.push(&json!({"n": 1})).await else {
                                line.set("push: error".to_owned());
                                return;
                            };
                            // The same clock tick: the second id must sort
                            // right after the first.
                            match r#ref.push(&json!({"n": 2})).await {
                                Ok(second) => {
                                    last_push.set(second.clone());
                                    line.set(format!(
                                        "push: {} {} {}",
                                        first.len(),
                                        second.len(),
                                        first < second
                                    ));
                                }
                                Err(error) => line.set(format!("push: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("push")),
        )
        .child(
            view()
                .test_id("push-next")
                .on_press({
                    let r#ref = db.r#ref("ui-pushed");
                    move || {
                        let r#ref = r#ref.clone();
                        spawn(async move {
                            match r#ref.push(&json!({"n": 3})).await {
                                Ok(id) => {
                                    let previous = last_push.get_untracked();
                                    last_push.set(id.clone());
                                    line.set(format!(
                                        "push-next: {} < {} = {}",
                                        previous,
                                        id,
                                        previous < id
                                    ));
                                }
                                Err(error) => line.set(format!("push-next: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("push next")),
        )
        .child(
            view()
                .test_id("transaction")
                .on_press({
                    let r#ref = db.r#ref("ui-count");
                    move || {
                        let r#ref = r#ref.clone();
                        spawn(async move {
                            let result = r#ref
                                .transaction(|current| {
                                    let at = current.and_then(JsonValue::as_i64).unwrap_or(0);
                                    Some(json!(at + 1))
                                })
                                .await;
                            match result {
                                Ok(result) => line.set(format!(
                                    "transaction: committed={} snapshot={}",
                                    result.committed,
                                    show(result.snapshot.as_ref())
                                )),
                                Err(error) => line.set(format!("transaction: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("transaction")),
        )
        .child(
            view()
                .test_id("query")
                .on_press({
                    let r#ref = db.r#ref("ui-scores");
                    move || {
                        let r#ref = r#ref.clone();
                        spawn(async move {
                            let read = r#ref.order_by_child("score").limit_to_first(2).get().await;
                            match read {
                                Ok(Some(value)) => {
                                    let keys: Vec<String> = value
                                        .as_object()
                                        .map(|entries| entries.keys().cloned().collect())
                                        .unwrap_or_default();
                                    line.set(format!("query: {}", keys.join(" ")));
                                }
                                Ok(None) => line.set("query: none".to_owned()),
                                Err(error) => line.set(format!("query: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("query")),
        )
        .child(
            view()
                .test_id("get-missing")
                .on_press({
                    let r#ref = db.r#ref("ui-nowhere");
                    move || {
                        let r#ref = r#ref.clone();
                        spawn(async move {
                            match r#ref.get().await {
                                Ok(_) => line.set("get-missing: answered".to_owned()),
                                Err(error) => line.set(format!("get-missing: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("get missing")),
        )
        .child(
            view()
                .test_id("listen-bad")
                .on_press({
                    // A filter without orderBy: subscribe time must say
                    // so, loudly, not open a stream or panic.
                    let r#ref = db.r#ref("ui-bad").limit_to_first(2);
                    move || match r#ref.on_value() {
                        Ok(_) => line.set("listen-bad: opened".to_owned()),
                        Err(error) => line.set(format!("listen-bad: {}", failed(error))),
                    }
                })
                .child(text("listen bad")),
        )
}

/// A transport that must never open: the REST verbs need no stream, so
/// a listener secretly started would show up here.
fn never_transport() -> Arc<dyn StreamTransport> {
    struct Never;
    impl StreamTransport for Never {
        fn open(&self, request: HttpRequest, _sink: Arc<dyn StreamSink>) -> Box<dyn StreamTask> {
            panic!("a stream opened where none should: {}", request.url);
        }
    }
    Arc::new(Never)
}

fn route(method: &str, url: &str, status: u16, body: &str) -> (String, StubRoute) {
    let mut response = StubResponse::default();
    response.status = status;
    response.body = body.as_bytes().to_vec();
    (format!("{method} {url}"), response.into())
}

fn routes_of(pairs: Vec<(&str, &str, u16, &str)>) -> Arc<StubNetwork> {
    Arc::new(StubNetwork::new(pairs.into_iter().map(
        |(method, url, status, body)| route(method, url, status, body),
    )))
}

#[test]
fn set_then_get_round_trips_through_the_stubbed_rest() {
    let network = routes_of(vec![
        (
            "PUT",
            "http://127.0.0.1:19000/ui-todos/one.json",
            200,
            r#"{"done":false,"title":"one"}"#,
        ),
        (
            "GET",
            "http://127.0.0.1:19000/ui-todos/one.json",
            200,
            r#"{"done":false,"title":"one"}"#,
        ),
    ]);
    let mut app = start_with_network(Root::of::<CrudApp>(), network);
    app.wait_for("text:get missing").unwrap();
    app.press("id:set").unwrap();
    app.wait_for("text:set: ok").unwrap();
    app.press("id:get").unwrap();
    app.wait_for(r#"text:get: {"done":false,"title":"one"}"#)
        .unwrap();
}

#[test]
fn update_merges_multi_paths_and_get_shows_the_merge() {
    let network = routes_of(vec![
        ("PUT", "http://127.0.0.1:19000/ui-multi.json", 200, ""),
        ("PATCH", "http://127.0.0.1:19000/ui-multi.json", 200, ""),
        (
            "GET",
            "http://127.0.0.1:19000/ui-multi.json",
            200,
            r#"{"a":9,"b":{"c":1,"d":7}}"#,
        ),
    ]);
    let mut app = start_with_network(Root::of::<CrudApp>(), network);
    app.wait_for("text:get missing").unwrap();
    app.press("id:update").unwrap();
    app.wait_for(r#"text:multi: {"a":9,"b":{"c":1,"d":7}}"#)
        .unwrap();
}

#[test]
fn remove_deletes_and_the_read_back_is_none() {
    let network = routes_of(vec![
        (
            "DELETE",
            "http://127.0.0.1:19000/ui-todos/one.json",
            200,
            "null",
        ),
        (
            "GET",
            "http://127.0.0.1:19000/ui-todos/one.json",
            200,
            "null",
        ),
    ]);
    let mut app = start_with_network(Root::of::<CrudApp>(), network);
    app.wait_for("text:get missing").unwrap();
    app.press("id:remove").unwrap();
    app.wait_for("text:removed: none").unwrap();
}

#[test]
fn push_ids_stay_ordered_within_and_across_milliseconds() {
    let network = routes_of(vec![(
        "PUT",
        "http://127.0.0.1:19000/ui-pushed/*.json",
        200,
        "",
    )]);
    let mut app = start_with_network(Root::of::<CrudApp>(), network);
    app.wait_for("text:get missing").unwrap();
    // Two pushes on the same manual clock tick: 20 characters each, the
    // second sorting after the first.
    app.press("id:push").unwrap();
    app.wait_for("text:push: 20 20 true").unwrap();
    // One millisecond later: the timestamp prefix alone orders it after.
    app.run_for(Duration::from_millis(1)).unwrap();
    app.press("id:push-next").unwrap();
    app.wait_for("text:/^push-next: [^ ]+ < [^ ]+ = true$/")
        .unwrap();
}

/// The transaction's GET always answers `5` with a fresh ETag; the PUT
/// answers `412` with the new value `6` once, then `200`.
#[test]
fn a_transaction_rebuilds_on_412_and_commits() {
    let get_handler: StubHandler = {
        let calls = Arc::new(AtomicUsize::new(0));
        Arc::new(move |_request, reply: StubReply| {
            let which = calls.fetch_add(1, Ordering::SeqCst) + 1;
            let mut response = StubResponse::default();
            response.status = 200;
            response.headers = vec![("ETag".to_owned(), format!("etag-{which}"))];
            response.body = b"5".to_vec();
            reply.send(response);
        })
    };
    let put_handler: StubHandler = {
        let calls = Arc::new(AtomicUsize::new(0));
        Arc::new(move |_request, reply: StubReply| {
            let which = calls.fetch_add(1, Ordering::SeqCst);
            let mut response = StubResponse::default();
            if which == 0 {
                // Changed elsewhere: the current value 6 and a fresh ETag.
                response.status = 412;
                response.headers = vec![("ETag".to_owned(), "etag-2".to_owned())];
                response.body = b"6".to_vec();
            } else {
                response.status = 200;
                response.body = b"7".to_vec();
            }
            reply.send(response);
        })
    };
    let network = Arc::new(StubNetwork::new(vec![
        (
            "GET http://127.0.0.1:19000/ui-count.json".to_owned(),
            StubRoute::Handler(get_handler),
        ),
        (
            "PUT http://127.0.0.1:19000/ui-count.json".to_owned(),
            StubRoute::Handler(put_handler),
        ),
    ]));
    let mut app = start_with_network(Root::of::<CrudApp>(), Arc::clone(&network));
    app.wait_for("text:get missing").unwrap();
    app.press("id:transaction").unwrap();
    app.wait_for("text:transaction: committed=true snapshot=7")
        .unwrap();
    // Two rounds: two tagged reads, two conditional writes with the
    // ETag each read returned.
    let requests = network.requests();
    let gets: Vec<_> = requests.iter().filter(|r| r.method == "GET").collect();
    let puts: Vec<_> = requests.iter().filter(|r| r.method == "PUT").collect();
    assert_eq!(gets.len(), 2, "{:?}", network.requests());
    assert_eq!(puts.len(), 2);
    for get in &gets {
        assert!(
            has_header(&get.headers, "x-firebase-etag", "true"),
            "the transaction read carries X-Firebase-ETag: {:?}",
            get.headers
        );
    }
    assert!(has_header(&puts[0].headers, "if-match", "etag-1"));
    assert!(has_header(&puts[1].headers, "if-match", "etag-2"));
}

fn has_header(headers: &[(String, String)], name: &str, value: &str) -> bool {
    headers
        .iter()
        .any(|(header, got)| header.eq_ignore_ascii_case(name) && got == value)
}

/// The query encodes to the REST parameters (the stub route matches the
/// URL whole) and the children come back sorted client-side.
#[test]
fn a_queried_get_encodes_the_params_and_sorts_children() {
    let url = "http://127.0.0.1:19000/ui-scores.json?orderBy=%22score%22&limitToFirst=2";
    // The stub answers in a deliberately unsorted key order: the sort
    // the screen shows is the client's.
    let network = routes_of(vec![(
        "GET",
        url,
        200,
        r#"{"b":{"score":9},"d":{"score":1},"a":{"score":5}}"#,
    )]);
    let mut app = start_with_network(Root::of::<CrudApp>(), network);
    app.wait_for("text:get missing").unwrap();
    app.press("id:query").unwrap();
    app.wait_for("text:query: d a b").unwrap();
}

#[test]
fn a_request_no_route_lists_is_a_network_failure() {
    let mut app = start_with_network(Root::of::<CrudApp>(), routes_of(vec![]));
    app.wait_for("text:get missing").unwrap();
    app.press("id:get-missing").unwrap();
    // The code leads the message; a miss of the route is a network loss.
    app.wait_for("text:/^get-missing: core\\/network/").unwrap();
}

/// An invalid query fails at subscribe time with the same
/// invalid-argument error a get would carry.
#[test]
fn a_listener_on_an_invalid_query_fails_at_subscribe_time() {
    let mut app = start_with_network(Root::of::<CrudApp>(), routes_of(vec![]));
    app.wait_for("text:get missing").unwrap();
    app.press("id:listen-bad").unwrap();
    app.wait_for("text:/^listen-bad: core\\/invalid-argument/")
        .unwrap();
}

// --- The listener apps -------------------------------------------------------

/// One scripted SSE connection: its bytes go out as one chunk; `ends`
/// says the stream closes after them (a network loss), not ending keeps
/// it live.
type Script = (Vec<u8>, bool);

/// The scripted transport the listener tests inject, holding its
/// scripts and the URLs it opened in statics (one pair per component,
/// so parallel tests do not share them).
struct ScriptedTransport {
    scripts: &'static OnceLock<Vec<Script>>,
    opens: &'static Mutex<Vec<String>>,
    taken: AtomicUsize,
}

impl StreamTransport for ScriptedTransport {
    fn open(&self, request: HttpRequest, sink: Arc<dyn StreamSink>) -> Box<dyn StreamTask> {
        // The recorded line keeps the accept header with the URL: the
        // stream must ask for `text/event-stream` (the docs' first rule).
        let accept = request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("accept"))
            .map(|(_, value)| value.as_str())
            .unwrap_or("");
        self.opens
            .lock()
            .unwrap()
            .push(format!("{}\naccept:{}", request.url, accept));
        let index = self.taken.fetch_add(1, Ordering::SeqCst);
        let (chunk, ends) = self
            .scripts
            .get()
            .and_then(|scripts| scripts.get(index).cloned())
            .unwrap_or_else(|| panic!("script {index} missing"));
        sink.head(200);
        if !chunk.is_empty() {
            sink.data(chunk);
        }
        if ends {
            sink.end();
        }
        Box::new(NoTask)
    }
}

struct NoTask;

impl StreamTask for NoTask {
    fn cancel(self: Box<Self>) {}
}

static RECONNECT_SCRIPTS: OnceLock<Vec<Script>> = OnceLock::new();
static RECONNECT_OPEN_URLS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static OFF_SCRIPTS: OnceLock<Vec<Script>> = OnceLock::new();
static OFF_OPEN_URLS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The frames one script plays, as one SSE chunk.
fn frames(parts: &[&str]) -> Vec<u8> {
    parts.concat().into_bytes()
}

fn listener_db(transport: Arc<dyn StreamTransport>) -> Arc<Database> {
    let app = initialize_app(FirebaseOptions::demo(), "db-ui").unwrap();
    Database::from_app(&app, transport).unwrap()
}

#[component]
fn ListenerApp() -> impl IntoElement {
    let db = listener_db(Arc::new(ScriptedTransport {
        scripts: &RECONNECT_SCRIPTS,
        opens: &RECONNECT_OPEN_URLS,
        taken: AtomicUsize::new(0),
    }));
    let signals = db.r#ref("ui-live").on_value().unwrap();
    view()
        .flex_1()
        .gap_1()
        .child(text(move || {
            format!(
                "live: {} {}",
                signals.status.get().label(),
                show(signals.value.get().as_ref())
            )
        }))
        .child(text("listen"))
}

#[test]
fn the_listener_receives_then_reconnects_and_resyncs_from_the_put() {
    let _ = RECONNECT_SCRIPTS.set(vec![
        (
            frames(&["event: put\ndata: {\"path\":\"/\",\"data\":{\"a\":1}}\n\n"]),
            true,
        ),
        (
            frames(&["event: put\ndata: {\"path\":\"/\",\"data\":{\"a\":2}}\n\n"]),
            false,
        ),
    ]);
    let mut app = start_with_network(Root::of::<ListenerApp>(), routes_of(vec![]));
    // The first stream connects at mount, delivers the put, and ends
    // (the network loss): the value stays and the status reconnects.
    app.wait_for(r#"text:live: reconnecting {"a":1}"#).unwrap();
    assert_eq!(RECONNECT_OPEN_URLS.lock().unwrap().len(), 1);
    // After the backoff's second the new stream opens, and the server's
    // fresh put replaces the cache whole.
    app.run_for(Duration::from_millis(1_100)).unwrap();
    app.wait_for(r#"text:live: live {"a":2}"#).unwrap();
    assert_eq!(RECONNECT_OPEN_URLS.lock().unwrap().len(), 2);
    // The listen URL is the ref's, with no query (this ref has none)
    // and no auth (none set), asking for the event stream.
    assert_eq!(
        RECONNECT_OPEN_URLS.lock().unwrap()[0],
        "http://127.0.0.1:19000/ui-live.json\naccept:text/event-stream"
    );
}

#[component]
fn ListenerOffApp() -> impl IntoElement {
    let db = listener_db(Arc::new(ScriptedTransport {
        scripts: &OFF_SCRIPTS,
        opens: &OFF_OPEN_URLS,
        taken: AtomicUsize::new(0),
    }));
    let signals = db.r#ref("ui-live").on_value().unwrap();
    let handle = signals.handle.clone();
    view()
        .flex_1()
        .gap_1()
        .child(text(move || {
            format!(
                "live: {} {}",
                signals.status.get().label(),
                show(signals.value.get().as_ref())
            )
        }))
        .child(
            view()
                .test_id("off")
                .on_press(move || handle.off())
                .child(text("off")),
        )
}

#[test]
fn off_during_the_backoff_stops_the_reconnecting() {
    let _ = OFF_SCRIPTS.set(vec![(
        frames(&["event: put\ndata: {\"path\":\"/\",\"data\":{\"a\":1}}\n\n"]),
        true,
    )]);
    let mut app = start_with_network(Root::of::<ListenerOffApp>(), routes_of(vec![]));
    app.wait_for(r#"text:live: reconnecting {"a":1}"#).unwrap();
    app.press("id:off").unwrap();
    // The backoff passes; off stopped the listener, so no stream opens
    // and the screen keeps what it showed.
    app.run_for(Duration::from_millis(2_000)).unwrap();
    assert_eq!(OFF_OPEN_URLS.lock().unwrap().len(), 1);
    // The status stays where off left it: the reconnect was stopped.
    assert_eq!(
        app.screen().count(r#"text:live: reconnecting {"a":1}"#),
        Ok(1),
        "{:?}",
        app.screen().count("text:")
    );
}
