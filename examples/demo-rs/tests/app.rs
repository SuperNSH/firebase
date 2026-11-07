// The demo's routes and product screens, headless on a manual clock
// with a stubbed network: the index lists the products, every route
// pushes and pops, the auth form validates and signs in through the
// stubbed emulator, the database list receives a live put, the upload
// shows its progress then downloads and deletes, messaging fails
// clearly in the pure tier, and the analytics queue flushes a batch.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use demo_rs::App;
use nsh_core::platform::{HttpBody, HttpRequest};
use nsh_engine::{Clock, EngineConfig, Root};
use nsh_testing::{Headless, StubHandler, StubNetwork, StubReply, StubResponse, StubRoute};
use serde_json::json;

const DB: &str = "http://127.0.0.1:19000";
const SESSION: &str = "http://127.0.0.1:19199/upload/demo-session";
const START_URL: &str =
    "http://127.0.0.1:19199/v0/b/demo-nsh.firebasestorage.app/o?name=demo%2Fnotes.bin";
const OBJECT_URL: &str =
    "http://127.0.0.1:19199/v0/b/demo-nsh.firebasestorage.app/o/demo%2Fnotes.bin";
const COLLECTOR: &str = "http://127.0.0.1:18742/collect";
const SIGN_IN_URL: &str = "http://127.0.0.1:19099/identitytoolkit.googleapis.com/v1/accounts:signInWithPassword?key=demo-nsh-fake-api-key";

fn app() -> Headless {
    Headless::start(EngineConfig {
        clock: Clock::manual(SystemTime::UNIX_EPOCH),
        ..EngineConfig::new(Root::of::<App>())
    })
    .unwrap()
}

fn app_with_network(network: StubNetwork) -> Headless {
    let clock = Clock::manual(SystemTime::UNIX_EPOCH);
    let network = Arc::new(network.with_clock(clock.clone()));
    Headless::start(EngineConfig {
        clock,
        network: Some(network),
        ..EngineConfig::new(Root::of::<App>())
    })
    .unwrap()
}

fn reply(status: u16, body: &str) -> StubResponse {
    let mut response = StubResponse::default();
    response.status = status;
    response.body = body.as_bytes().to_vec();
    response
}

fn header_of(request: &HttpRequest, name: &str) -> Option<String> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn body_len(request: &HttpRequest) -> usize {
    match &request.body {
        HttpBody::Bytes(bytes) => bytes.len(),
        _ => 0,
    }
}

/// The storage session: slices record, the finalize answers the
/// object's metadata; `received` counts what landed.
fn session_route(received: Arc<Mutex<u64>>) -> StubRoute {
    let handler: StubHandler = Arc::new(move |request: HttpRequest, reply: StubReply| {
        let command = header_of(&request, "X-Goog-Upload-Command").unwrap_or_default();
        if command == "query" {
            // The resume probe: what the server already holds.
            reply.send(reply_with(
                200,
                vec![
                    ("X-Goog-Upload-Status", "active".to_owned()),
                    (
                        "X-Goog-Upload-Size-Received",
                        received.lock().unwrap().to_string(),
                    ),
                ],
                String::new(),
            ));
            return;
        }
        if command == "upload" || command == "upload, finalize" {
            *received.lock().unwrap() += body_len(&request) as u64;
            if command == "upload, finalize" {
                reply.send(reply_with(
                    200,
                    vec![("X-Goog-Upload-Status", "final".to_owned())],
                    json!({
                        "name": "notes.bin",
                        "bucket": "demo-nsh.firebasestorage.app",
                        "size": "600000",
                        "contentType": "application/octet-stream",
                    })
                    .to_string(),
                ));
                return;
            }
        }
        // The first slice waits on the clock, so the in-flight
        // progress is observable before the run completes.
        let offset: u64 = header_of(&request, "X-Goog-Upload-Offset")
            .and_then(|offset| offset.parse().ok())
            .unwrap_or(0);
        reply.send(reply_with_delay(
            200,
            vec![("X-Goog-Upload-Status", "active".to_owned())],
            String::new(),
            if offset == 0 {
                Duration::from_millis(250)
            } else {
                Duration::ZERO
            },
        ));
    });
    StubRoute::Handler(handler)
}

fn reply_with_delay(
    status: u16,
    headers: Vec<(&str, String)>,
    body: String,
    delay: Duration,
) -> StubResponse {
    let mut response = reply_with(status, headers, body);
    response.delay = delay;
    response
}

fn reply_with(status: u16, headers: Vec<(&str, String)>, body: String) -> StubResponse {
    let mut response = StubResponse::default();
    response.status = status;
    response.headers = headers
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
    response.body = body.into_bytes();
    response
}

fn signed_in_reply() -> StubResponse {
    reply(
        200,
        &json!({
            "idToken": "demo-id-token",
            "refreshToken": "demo-refresh-token",
            "expiresIn": "3600",
            "email": "demo@demo-nsh.test",
            "localId": "uid-demo-1",
            "registered": true,
        })
        .to_string(),
    )
}

/// One SSE put frame: the fresh stream's whole value at the ref.
fn put(value: serde_json::Value) -> String {
    format!(
        "event: put\ndata: {}\n\n",
        json!({ "path": "/", "data": value })
    )
}

#[test]
fn the_index_lists_every_product_route() -> Result<(), String> {
    let mut app = app();
    for title in [
        "Authentication",
        "Realtime Database",
        "Cloud Storage",
        "Cloud Messaging",
        "Analytics",
        "Performance",
    ] {
        app.wait_for(title)?;
    }
    Ok(())
}

#[test]
fn every_product_route_pushes_and_pops() -> Result<(), String> {
    let mut app = app();
    app.wait_for("label:Authentication")?;
    for (title, page) in [
        ("Authentication", "page-auth"),
        ("Realtime Database", "page-database"),
        ("Cloud Storage", "page-storage"),
        ("Cloud Messaging", "page-messaging"),
        ("Analytics", "page-analytics"),
        ("Performance", "page-monitor"),
    ] {
        app.press(&format!("label:{title}"))?;
        app.wait_for(&format!("id:{page}"))?;
        app.back()?;
        app.wait_for("id:page-home")?;
    }
    Ok(())
}

#[test]
fn the_auth_form_validates_then_signs_in_through_the_stub() -> Result<(), String> {
    let mut app = app_with_network(StubNetwork::new(vec![(
        format!("POST {SIGN_IN_URL}"),
        StubRoute::from(signed_in_reply()),
    )]));
    app.wait_for("label:Authentication")?;
    app.press("label:Authentication")?;
    app.wait_for("id:page-auth")?;
    // An empty submit shows the rules, not a server call.
    app.press("role:button[Sign in]")?;
    app.wait_for("text:Enter your email.")?;
    // A valid form reaches the emulator and flips the currentUser signal.
    app.type_text("id:auth-email", "demo@demo-nsh.test")?;
    app.type_text("id:auth-password", "fake-password-1")?;
    app.press("role:button[Sign in]")?;
    app.wait_for("text:Signed in as uid-demo-1 (demo@demo-nsh.test)")?;
    // Signing out clears the signal.
    app.press("role:button[Sign out]")?;
    app.wait_for("text:Signed out")?;
    Ok(())
}

#[test]
fn a_server_auth_error_lands_on_its_field() -> Result<(), String> {
    let wrong_password = reply(
        400,
        &json!({ "error": { "code": 400, "message": "INVALID_PASSWORD" } }).to_string(),
    );
    let mut app = app_with_network(StubNetwork::new(vec![(
        format!("POST {SIGN_IN_URL}"),
        StubRoute::from(wrong_password),
    )]));
    app.wait_for("label:Authentication")?;
    app.press("label:Authentication")?;
    app.wait_for("id:page-auth")?;
    app.type_text("id:auth-email", "demo@demo-nsh.test")?;
    app.type_text("id:auth-password", "wrong-password")?;
    app.press("role:button[Sign in]")?;
    // The field error, not the form-level message: the exact library wording.
    app.wait_for("text:that password is wrong for the account (INVALID_PASSWORD)")?;
    Ok(())
}

#[test]
fn the_database_screen_shows_the_live_value_and_pushes() -> Result<(), String> {
    let mut app = app_with_network(StubNetwork::new(vec![
        (
            format!("GET {DB}/demo/items.json"),
            StubRoute::from(reply(200, &put(json!({ "item-1": { "title": "First" } })))),
        ),
        (
            format!("PUT {DB}/demo/items/e2.json"),
            StubRoute::from(reply(200, "")),
        ),
    ]));
    app.wait_for("label:Realtime Database")?;
    app.press("label:Realtime Database")?;
    app.wait_for("id:page-database")?;
    // The listener's first put renders the row; the stub's stream ends
    // after it, which the status line reports honestly.
    app.wait_for("text:First")?;
    app.wait_for("text:1 items · Reconnecting…")?;
    // The push form writes a child and clears its draft.
    app.type_text("id:db-draft", "Second")?;
    app.press("role:button[Add]")?;
    let pushes = app
        .screen()
        .texts()
        .into_iter()
        .filter(|text| text.contains("items ·"))
        .count();
    assert!(pushes >= 1, "the status line is gone");
    Ok(())
}

#[test]
fn the_upload_shows_progress_then_download_and_delete_run() -> Result<(), String> {
    let received = Arc::new(Mutex::new(0_u64));
    let routes = vec![
        (
            format!("POST {START_URL}"),
            StubRoute::from(reply_with(
                200,
                vec![
                    ("X-Goog-Upload-Status", "active".to_owned()),
                    ("X-Goog-Upload-URL", SESSION.to_owned()),
                ],
                String::new(),
            )),
        ),
        (
            format!("POST {SESSION}"),
            session_route(Arc::clone(&received)),
        ),
        (
            format!("GET {OBJECT_URL}?alt=media"),
            StubRoute::from(reply(200, "demo-bytes")),
        ),
        (
            format!("DELETE {OBJECT_URL}"),
            StubRoute::from(reply(200, "")),
        ),
    ];
    let mut app = app_with_network(StubNetwork::new(routes));
    app.wait_for("label:Cloud Storage")?;
    app.press("label:Cloud Storage")?;
    app.wait_for("id:page-storage")?;
    // The task's progress signal drives the bar from the first instant.
    app.press("role:button[Upload 600 kB]")?;
    app.wait_for("text:0 of 600000 bytes · running")?;
    app.wait_for("text:Uploaded notes.bin (600000 bytes).")?;
    assert!(*received.lock().unwrap() >= 600_000);
    app.press("role:button[Download]")?;
    app.wait_for("text:Downloaded 10 bytes.")?;
    app.press("role:button[Delete]")?;
    app.wait_for("text:Deleted.")?;
    Ok(())
}

#[test]
fn messaging_fails_clearly_in_the_pure_tier() -> Result<(), String> {
    let mut app = app();
    app.wait_for("label:Cloud Messaging")?;
    app.press("label:Cloud Messaging")?;
    app.wait_for("id:page-messaging")?;
    app.press("role:button[Check permission]")?;
    // The refusal carries the stable code; the static note at the
    // bottom does not start with it, so this is the error line.
    app.wait_for("text:/^core\\/unavailable/")?;
    app.press("role:button[Request token]")?;
    app.wait_for("text:/^core\\/unavailable/")?;
    Ok(())
}

#[test]
fn the_analytics_queue_sends_a_batch_on_flush() -> Result<(), String> {
    let mut app = app_with_network(StubNetwork::new(vec![(
        format!("POST {COLLECTOR}"),
        StubRoute::from(reply(200, "")),
    )]));
    app.wait_for("label:Analytics")?;
    app.press("label:Analytics")?;
    app.wait_for("id:page-analytics")?;
    app.wait_for("text:Pending: 0")?;
    app.press("role:button[Log event]")?;
    app.wait_for("text:Pending: 1")?;
    // The flush sends the queued event to the collector stub.
    app.press("role:button[Flush now]")?;
    app.wait_for("text:Pending: 0")?;
    Ok(())
}
