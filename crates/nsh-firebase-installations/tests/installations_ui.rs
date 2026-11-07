//! Headless UI tests of the pure-tier REST flows: every step goes through
//! a test component on a manual clock (no real time) and a stubbed
//! network (no real requests), with the reply shapes the real service
//! gives (verified against the firebase-js-sdk). The native-tier routing
//! is tests/installations.rs.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use nsh_core::JsonValue;
use nsh_core::element::IntoElement;
use nsh_core::platform::{HttpBody, HttpRequest};
use nsh_core::prelude::*;
use nsh_engine::{Clock, EngineConfig, Root};
use nsh_firebase_core::FirebaseOptions;
use nsh_firebase_core::host::{FirebaseHost, PureHost};
use nsh_firebase_core::initialize_app;
use nsh_firebase_installations::{Installations, KeyValueStore, MemoryStore, is_valid_fid};
use nsh_testing::{Headless, StubNetwork, StubResponse, StubRoute};
use serde_json::json;

/// The manual clock's fixed instant; every expected timestamp below
/// derives from it.
const T0_SECS: u64 = 1_767_258_000;
const T0_MS: u64 = T0_SECS * 1000;
const REGISTER_URL: &str =
    "https://firebaseinstallations.googleapis.com/v1/projects/demo-nsh/installations";
const TOKEN_GLOB: &str = "POST https://firebaseinstallations.googleapis.com/v1/projects/demo-nsh/installations/*/authTokens:generate";
/// The register reply's token stays valid a week, and a token minted at
/// the due point expires a week past it.
const REFRESHED_EXPIRES_MS: u64 = T0_MS + 604_800_000 - 3_600_000 + 604_800_000;
/// The store key the record lives under (one name per app).
const STORE_KEY: &str = "nsh.firebase.installations.inst-ui";
/// The demo- project's refusal: the body shape the real service answers a
/// fake project with.
const DENIED_BODY: &str = r#"{"error":{"code":403,"message":"Firebase Installations service is disabled for this project","status":"PERMISSION_DENIED"}}"#;
const BACKEND_BODY: &str = r#"{"error":{"code":500,"message":"backend"}}"#;

fn t0() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(T0_SECS)
}

fn reply(status: u16, body: &str) -> StubResponse {
    let mut response = StubResponse::default();
    response.status = status;
    response.body = body.as_bytes().to_vec();
    response
}

fn failing(message: &str) -> StubResponse {
    let mut response = StubResponse::default();
    response.error = Some(message.to_owned());
    response
}

/// One request as the flow sent it, so a test can assert what went out.
#[derive(Clone)]
struct Sent {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: String,
}

type RequestLog = Arc<Mutex<Vec<Sent>>>;

fn snapshot(log: &RequestLog, request: &HttpRequest) {
    let body = match &request.body {
        HttpBody::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        HttpBody::Empty => String::new(),
        HttpBody::Stream => "<stream>".to_owned(),
    };
    log.lock().expect("request log").push(Sent {
        method: request.method.clone(),
        url: request.url.clone(),
        headers: request.headers.clone(),
        body,
    });
}

fn requests(log: &RequestLog) -> Vec<Sent> {
    log.lock().expect("request log").clone()
}

fn header_of(sent: &Sent, name: &str) -> String {
    sent.headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
        .unwrap_or_default()
}

/// The register route: records the request, echoes the FID it carried as
/// the server does, and answers with the reply shape the service gives.
fn register_route(log: &RequestLog) -> StubRoute {
    let log = Arc::clone(log);
    StubRoute::Handler(Arc::new(move |request: HttpRequest, reply_channel| {
        snapshot(&log, &request);
        let fid = serde_json::from_str::<JsonValue>(&request_body(&request))
            .ok()
            .and_then(|body| {
                body.get("fid")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        let reply_body = json!({
            "name": format!("projects/demo-nsh/installations/{fid}"),
            "fid": fid,
            "refreshToken": "refresh-token-1",
            "authToken": { "token": "auth-token-1", "expiresIn": "604800s" },
        });
        reply_channel.send(reply(200, &reply_body.to_string()));
    }))
}

fn token_route(log: &RequestLog, token: &str) -> StubRoute {
    let log = Arc::clone(log);
    let token = token.to_owned();
    StubRoute::Handler(Arc::new(move |request: HttpRequest, reply_channel| {
        snapshot(&log, &request);
        let reply_body = json!({ "token": token, "expiresIn": "604800s" });
        reply_channel.send(reply(200, &reply_body.to_string()));
    }))
}

fn request_body(request: &HttpRequest) -> String {
    match &request.body {
        HttpBody::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        HttpBody::Empty => String::new(),
        HttpBody::Stream => "<stream>".to_owned(),
    }
}

fn start(root: Root, network: StubNetwork) -> Headless {
    let mut config = EngineConfig::new(root);
    config.clock = Clock::manual(t0());
    config.network = Some(Arc::new(network));
    Headless::start(config).unwrap()
}

/// One exercise component: a button per flow, one result line, the
/// restore-on-init read behind a press. Every network flow runs in
/// `spawn`, as an app's handlers would.
#[component]
fn InstallationsApp() -> impl IntoElement {
    let app = initialize_app(FirebaseOptions::demo(), "inst-ui").unwrap();
    let store: Arc<dyn KeyValueStore> = Arc::new(MemoryStore::new());
    let host: Arc<dyn FirebaseHost> = Arc::new(PureHost);
    let installations = Arc::new(Installations::new(
        Arc::clone(&app),
        Arc::clone(&store),
        host,
    ));
    let line = signal(String::new());
    let restored = signal("restored: never".to_owned());

    let get_id = {
        let (installations, line) = (Arc::clone(&installations), line);
        move || {
            let (installations, line) = (Arc::clone(&installations), line);
            spawn(async move {
                let text = match installations.get_id().await {
                    Ok(fid) => format!("id: {fid}"),
                    Err(error) => format!("id: {error}"),
                };
                line.set(text);
            });
        }
    };
    let get_token = {
        let (installations, line) = (Arc::clone(&installations), line);
        move || {
            let (installations, line) = (Arc::clone(&installations), line);
            spawn(async move {
                let text = match installations.get_token(false).await {
                    Ok(token) => format!("token: {token}"),
                    Err(error) => format!("token: {error}"),
                };
                line.set(text);
            });
        }
    };
    let force_refresh = {
        let (installations, line) = (Arc::clone(&installations), line);
        move || {
            let (installations, line) = (Arc::clone(&installations), line);
            spawn(async move {
                let text = match installations.get_token(true).await {
                    Ok(token) => format!("refresh: {token}"),
                    Err(error) => format!("refresh: {error}"),
                };
                line.set(text);
            });
        }
    };
    let due = {
        let (installations, line) = (Arc::clone(&installations), line);
        move || line.set(format!("due: {}", installations.token_due()))
    };
    let expires = {
        let (installations, line) = (Arc::clone(&installations), line);
        move || {
            line.set(match installations.token_expires_at_ms() {
                Some(at) => format!("expires: {at}"),
                None => "expires: none".to_owned(),
            })
        }
    };
    // Restore-on-init: a fresh Installations over the same store, what
    // the app's next launch does. Sync, so no spawn; the store read must
    // already have happened by the time the line renders.
    let reinstall = {
        let (app, store, restored) = (app, Arc::clone(&store), restored);
        move || {
            let host: Arc<dyn FirebaseHost> = Arc::new(PureHost);
            let second = Installations::new(Arc::clone(&app), Arc::clone(&store), host);
            restored.set(format!(
                "restored: fid={} refresh={}",
                second.id().as_deref().unwrap_or("none"),
                second.refresh_token().as_deref().unwrap_or("none"),
            ));
        }
    };

    view()
        .flex_1()
        .gap_1()
        .child(text(move || line.get()))
        .child(text(move || restored.get()))
        .child(
            view()
                .test_id("get-id")
                .on_press(get_id)
                .child(text("get id")),
        )
        .child(
            view()
                .test_id("get-token")
                .on_press(get_token)
                .child(text("get token")),
        )
        .child(
            view()
                .test_id("force")
                .on_press(force_refresh)
                .child(text("force")),
        )
        .child(view().test_id("due").on_press(due).child(text("due?")))
        .child(
            view()
                .test_id("expires")
                .on_press(expires)
                .child(text("expires")),
        )
        .child(
            view()
                .test_id("reinstall")
                .on_press(reinstall)
                .child(text("reinstall")),
        )
}

/// Restore-on-init from a record a previous launch (or the TypeScript
/// tier) wrote, seeded by the test.
static RESTORE_STORE: OnceLock<Arc<MemoryStore>> = OnceLock::new();

#[component]
fn RestoreApp() -> impl IntoElement {
    let store = RESTORE_STORE.get().expect("seeded store").clone();
    let app = initialize_app(FirebaseOptions::demo(), "inst-ui").unwrap();
    let host: Arc<dyn FirebaseHost> = Arc::new(PureHost);
    let installations = Arc::new(Installations::new(app, store, host));
    let ready = Arc::clone(&installations);
    let line = Arc::clone(&installations);
    view()
        .flex_1()
        .gap_1()
        .child(text(move || format!("ready: {}", ready.restore_ready())))
        .child(text(move || {
            format!(
                "restored: fid={} refresh={}",
                line.id().as_deref().unwrap_or("none"),
                line.refresh_token().as_deref().unwrap_or("none"),
            )
        }))
}

#[test]
fn register_persists_and_get_token_serves_the_cached_token_until_it_is_due() {
    let log: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let network = StubNetwork::new([
        (format!("POST {REGISTER_URL}"), register_route(&log)),
        (TOKEN_GLOB.to_owned(), token_route(&log, "auth-token-2")),
    ]);
    let mut app = start(Root::of::<InstallationsApp>(), network);
    app.first_render().unwrap();
    assert!(app.shows("restored: never"));

    app.press("id:get-id").unwrap();
    app.wait_for("text:/^id: [cdef][A-Za-z0-9_-]{21}/").unwrap();
    let sent = requests(&log);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, "POST");
    assert_eq!(sent[0].url, REGISTER_URL);
    assert_eq!(
        header_of(&sent[0], "x-goog-api-key"),
        "demo-nsh-fake-api-key"
    );
    assert_eq!(header_of(&sent[0], "Content-Type"), "application/json");
    assert_eq!(header_of(&sent[0], "Accept"), "application/json");
    let body: JsonValue = serde_json::from_str(&sent[0].body).unwrap();
    let fid = body["fid"].as_str().unwrap().to_owned();
    assert!(is_valid_fid(&fid), "{fid}");
    assert_eq!(body["authVersion"], json!("FIS_v2"));
    assert_eq!(body["appId"], json!("1:000000000000:web:demo-nsh"));
    assert_eq!(body["sdkVersion"], json!("w:0.1.0"));

    // The record the store now holds is the one a later launch restores.
    app.press("id:reinstall").unwrap();
    app.wait_for(&format!("text:restored: fid={fid} refresh=refresh-token-1"))
        .unwrap();

    // The register reply's token is already cached: no second request.
    app.press("id:get-token").unwrap();
    app.wait_for("text:token: auth-token-1").unwrap();
    assert_eq!(requests(&log).len(), 1);
    app.press("id:expires").unwrap();
    app.wait_for(&format!("text:expires: {}", T0_MS + 604_800_000))
        .unwrap();

    // Due only once the clock passes the one-hour margin before expiry.
    app.press("id:due").unwrap();
    app.wait_for("text:due: false").unwrap();
    app.set_system_time(t0() + Duration::from_millis(604_800_000 - 3_600_000))
        .unwrap();
    app.press("id:due").unwrap();
    app.wait_for("text:due: true").unwrap();

    app.press("id:get-token").unwrap();
    app.wait_for("text:token: auth-token-2").unwrap();
    let sent = requests(&log);
    assert_eq!(sent.len(), 2);
    assert!(
        sent[1]
            .url
            .starts_with(&format!("{REGISTER_URL}/{fid}/authTokens:generate"))
    );
    assert_eq!(
        header_of(&sent[1], "Authorization"),
        "FIS_v2 refresh-token-1"
    );
    assert_eq!(
        serde_json::from_str::<JsonValue>(&sent[1].body).unwrap(),
        json!({ "installation": { "sdkVersion": "w:0.1.0", "appId": "1:000000000000:web:demo-nsh" } })
    );
    app.press("id:expires").unwrap();
    app.wait_for(&format!("text:expires: {REFRESHED_EXPIRES_MS}"))
        .unwrap();
}

#[test]
fn a_force_refresh_skips_the_cache_but_a_plain_get_token_keeps_it() {
    let log: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let network = StubNetwork::new([
        (format!("POST {REGISTER_URL}"), register_route(&log)),
        (TOKEN_GLOB.to_owned(), token_route(&log, "auth-token-gen-1")),
    ]);
    let mut app = start(Root::of::<InstallationsApp>(), network);
    app.first_render().unwrap();

    // Force on an unregistered installation registers first, then mints.
    app.press("id:force").unwrap();
    app.wait_for("text:refresh: auth-token-gen-1").unwrap();
    let sent = requests(&log);
    assert_eq!(sent.len(), 2);

    // The fresh token is cached: a plain get_token stays off the network.
    app.press("id:get-token").unwrap();
    app.wait_for("text:token: auth-token-gen-1").unwrap();
    assert_eq!(requests(&log).len(), 2);
}

#[test]
fn a_fake_project_refusal_maps_to_a_clear_request_failed_and_persists_nothing() {
    let log: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let network = StubNetwork::new([(format!("POST {REGISTER_URL}"), reply(403, DENIED_BODY))]);
    let mut app = start(Root::of::<InstallationsApp>(), network);
    app.first_render().unwrap();
    app.press("id:get-id").unwrap();
    app.wait_for(
        "text:id: installations/request-failed: Create Installation request failed with error \
         \"403 PERMISSION_DENIED: Firebase Installations service is disabled for this project\"",
    )
    .unwrap();
    assert_eq!(requests(&log).len(), 1);
    // Nothing was persisted: the next launch starts unregistered.
    app.press("id:reinstall").unwrap();
    app.wait_for("text:restored: fid=none refresh=none")
        .unwrap();
}

#[test]
fn a_server_failure_maps_to_internal_error() {
    let network = StubNetwork::new([(format!("POST {REGISTER_URL}"), reply(500, BACKEND_BODY))]);
    let mut app = start(Root::of::<InstallationsApp>(), network);
    app.first_render().unwrap();
    app.press("id:get-id").unwrap();
    app.wait_for(
        "text:id: installations/internal-error: the installations server failed (HTTP 500)",
    )
    .unwrap();
}

#[test]
fn a_transport_failure_keeps_the_core_network_code() {
    let network = StubNetwork::new([(
        format!("POST {REGISTER_URL}"),
        failing("the connection was reset"),
    )]);
    let mut app = start(Root::of::<InstallationsApp>(), network);
    app.first_render().unwrap();
    app.press("id:get-id").unwrap();
    app.wait_for("text:id: core/network: the connection was reset")
        .unwrap();
}

#[test]
fn a_fresh_installations_restores_the_record_another_tier_wrote() {
    // The camelCase record the TypeScript side writes, handed to the Rust
    // side verbatim: either tier restores the other's record.
    RESTORE_STORE
        .set(Arc::new(MemoryStore::seeded([(
            STORE_KEY.to_owned(),
            r#"{"v":1,"fid":"c0123456789abcdef-_xyz","refreshToken":"stored-refresh"}"#.to_owned(),
        )])))
        .ok();
    let mut app = start(
        Root::of::<RestoreApp>(),
        StubNetwork::new(Vec::<(String, StubResponse)>::new()),
    );
    app.first_render().unwrap();
    assert!(app.shows("ready: true"));
    app.wait_for("text:restored: fid=c0123456789abcdef-_xyz refresh=stored-refresh")
        .unwrap();
}
