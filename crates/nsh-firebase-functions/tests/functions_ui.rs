//! Headless tests of the callable wire protocol: request URL and body
//! shape, result mapping (both `data` and legacy `result` answers), the
//! backend error mapping to `functions/*` codes, an unknown canonical
//! status, and the emulator's missing-function shape — all through a
//! test-only component on a manual clock with the network stubbed, no
//! real time and no real server. Native routing is exercised without an
//! engine in `src/functions.rs`'s inline tests.

use std::{sync::Arc, time::Duration, time::SystemTime};

use nsh_core::{
    platform::{HttpBody, HttpRequest},
    prelude::*,
};
use nsh_engine::{Clock, EngineConfig, Root};
use nsh_firebase_core::{FirebaseOptions, initialize_app};
use nsh_firebase_functions::{Functions, FunctionsError};
use nsh_testing::{Headless, StubNetwork, StubResponse, StubRoute, WAIT};
use serde_json::Value as JsonValue;

/// The emulator URL the sandbox default config builds.
fn url(name: &str) -> String {
    format!("http://127.0.0.1:15001/demo-nsh/us-central1/{name}")
}

/// `StubResponse` is `#[non_exhaustive]`: fields are set on the default.
fn stub(status: u16, body: &str) -> StubResponse {
    let mut response = StubResponse::default();
    response.status = status;
    response.body = body.as_bytes().to_vec();
    response
}

fn body_text(request: &HttpRequest) -> String {
    match &request.body {
        HttpBody::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        _ => String::new(),
    }
}

fn header_of(request: &HttpRequest, name: &str) -> Option<String> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn start_app() -> (Headless, Arc<StubNetwork>) {
    // The add route answers by body: the good call, the bad call whose
    // backend throws HttpsError("invalid-argument"), each the shape the
    // real emulator's firebase-functions v1 runtime sends.
    let add = StubRoute::Handler(Arc::new(
        |request: HttpRequest, reply: nsh_testing::StubReply| {
            let body = body_text(&request);
            if body.contains(r#""a":1"#) {
                reply.send(stub(200, r#"{"result":{"sum":3}}"#));
            } else {
                reply.send(stub(
                400,
                r#"{"error":{"status":"INVALID_ARGUMENT","message":"a and b must be numbers"}}"#,
            ));
            }
        },
    ));
    let routes = vec![
        (
            format!("POST {}", url("ping")),
            StubRoute::from(stub(200, r#"{"result":{"pong":true,"sent":null}}"#)),
        ),
        (format!("POST {}", url("add")), add),
        // A 200 answer may still carry an error body: the canonical name
        // decides, and an unknown one is internal.
        (
            format!("POST {}", url("weird")),
            StubRoute::from(stub(200, r#"{"error":{"status":"WEIRD","message":"m"}}"#)),
        ),
        // The emulator's missing-function answer: 404 with a plain-text
        // body (functionsEmulator.js), which is not JSON.
        (
            format!("POST {}", url("missing")),
            StubRoute::from(stub(
                404,
                "Function us-central1-missing does not exist, valid functions are: ping, add",
            )),
        ),
        (
            format!("POST {}", url("details")),
            StubRoute::from(stub(
                400,
                r#"{"error":{"status":"FAILED_PRECONDITION","message":"no","details":{"quota":9}}}"#,
            )),
        ),
        // The legacy `data` key: the SDK reads data before result.
        (
            format!("POST {}", url("legacy")),
            StubRoute::from(stub(200, r#"{"data":{"old":1}}"#)),
        ),
    ];
    let clock = Clock::manual(SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_258_000));
    let network = Arc::new(StubNetwork::new(routes).with_clock(clock.clone()));
    let mut config = EngineConfig::new(Root::of::<FunctionsApp>());
    config.clock = clock;
    config.network = Some(network.clone());
    let mut app = Headless::start(config).unwrap();
    app.wait_for("text:functions ready").unwrap();
    (app, network)
}

/// Waits until a text on screen ends with `suffix` (the line grows).
fn wait_text(app: &mut Headless, suffix: &str) {
    app.wait_until(&format!("text ending `{suffix}`"), WAIT, |screen| {
        screen.texts().iter().any(|text| text.ends_with(suffix))
    })
    .unwrap();
}

#[component]
fn FunctionsApp() -> impl IntoElement {
    let line = signal(String::from("functions ready"));
    let app = initialize_app(FirebaseOptions::demo(), "functions-ui").unwrap();
    let functions = Functions::new(&app);

    let run = |line_for_run: Signal<String>,
               functions_for_run: Functions,
               name: &'static str,
               data: JsonValue| {
        spawn(async move {
            match functions_for_run.call(name, data).await {
                Ok(result) => line_for_run.set(format!("fn: {name} ok {}", result)),
                Err(error) => line_for_run.set(format!("fn: {name} err {}", render(&error))),
            }
        });
    };

    let (line_ping, functions_ping) = (line, functions.clone());
    let (line_add, functions_add) = (line, functions.clone());
    let (line_bad, functions_bad) = (line, functions.clone());
    let (line_weird, functions_weird) = (line, functions.clone());
    let (line_missing, functions_missing) = (line, functions.clone());
    let (line_details, functions_details) = (line, functions.clone());
    let (line_legacy, functions_legacy) = (line, functions);

    view()
        .flex_1()
        .gap_1()
        .child(text(move || line.get()))
        .child(
            view()
                .test_id("ping")
                .on_press(move || run(line_ping, functions_ping, "ping", JsonValue::Null))
                .child(text("ping")),
        )
        .child(
            view()
                .test_id("add")
                .on_press(move || {
                    run(
                        line_add,
                        functions_add,
                        "add",
                        serde_json::json!({ "a": 1, "b": 2 }),
                    )
                })
                .child(text("add")),
        )
        .child(
            view()
                .test_id("bad-add")
                .on_press(move || {
                    run(
                        line_bad,
                        functions_bad,
                        "add",
                        serde_json::json!({ "a": "x", "b": 2 }),
                    )
                })
                .child(text("bad add")),
        )
        .child(
            view()
                .test_id("weird")
                .on_press(move || run(line_weird, functions_weird, "weird", JsonValue::Null))
                .child(text("weird")),
        )
        .child(
            view()
                .test_id("missing")
                .on_press(move || run(line_missing, functions_missing, "missing", JsonValue::Null))
                .child(text("missing")),
        )
        .child(
            view()
                .test_id("details")
                .on_press(move || run(line_details, functions_details, "details", JsonValue::Null))
                .child(text("details")),
        )
        .child(
            view()
                .test_id("legacy")
                .on_press(move || run(line_legacy, functions_legacy, "legacy", JsonValue::Null))
                .child(text("legacy")),
        )
}

/// The line a failed call shows: the message plus what details reached
/// the client (the wire's optional `error.details`).
fn render(error: &FunctionsError) -> String {
    match &error.details {
        Some(details) => format!("{} details={}", error, details),
        None => error.to_string(),
    }
}

/// The recorded request must speak the exact protocol.
#[test]
fn the_request_speaks_the_callable_wire_protocol() {
    let (mut app, network) = start_app();
    app.press("id:add").unwrap();
    wait_text(&mut app, "fn: add ok {\"sum\":3}");

    let requests = network.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.url, url("add"));
    assert_eq!(
        header_of(request, "Content-Type").as_deref(),
        Some("application/json")
    );
    assert_eq!(body_text(request), r#"{"data":{"a":1,"b":2}}"#);
}

/// Absent data still sends the `data` key, as null.
#[test]
fn absent_data_is_null_in_the_body() {
    let (mut app, network) = start_app();
    app.press("id:ping").unwrap();
    wait_text(&mut app, "fn: ping ok {\"pong\":true,\"sent\":null}");

    let requests = network.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url, url("ping"));
    assert_eq!(body_text(&requests[0]), r#"{"data":null}"#);
}

/// The backend's HttpsError body maps to the SDK's functions/* code.
#[test]
fn a_backend_error_maps_to_the_functions_code() {
    let (mut app, _network) = start_app();
    app.press("id:bad-add").unwrap();
    wait_text(
        &mut app,
        "fn: add err functions/invalid-argument: a and b must be numbers [400]",
    );
}

/// An unknown canonical status in the body is internal, not the
/// HTTP-status default — and it is read even on a 200.
#[test]
fn an_unknown_backend_status_is_internal() {
    let (mut app, _network) = start_app();
    app.press("id:weird").unwrap();
    wait_text(
        &mut app,
        "fn: weird err functions/internal: Unknown backend error status: WEIRD [200]",
    );
}

/// The emulator's 404 with a plain-text body: functions/not-found.
#[test]
fn a_missing_function_is_not_found() {
    let (mut app, _network) = start_app();
    app.press("id:missing").unwrap();
    wait_text(
        &mut app,
        "fn: missing err functions/not-found: not-found [404]",
    );
}

/// The wire's optional `error.details` reaches the error object.
#[test]
fn details_reach_the_error() {
    let (mut app, _network) = start_app();
    app.press("id:details").unwrap();
    wait_text(
        &mut app,
        "fn: details err functions/failed-precondition: no [400] details={\"quota\":9}",
    );
}

/// A `{data}`-shaped answer maps too: the SDK reads data before result.
#[test]
fn a_data_shaped_answer_maps() {
    let (mut app, _network) = start_app();
    app.press("id:legacy").unwrap();
    wait_text(&mut app, "fn: legacy ok {\"old\":1}");
}
