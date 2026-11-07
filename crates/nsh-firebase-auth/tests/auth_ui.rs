//! Headless UI tests of the auth flows: every step goes through a test
//! component on a manual clock (no real time) and a stubbed network (no
//! real requests), with the wire replies the emulator and the real
//! services give. The sandbox run against the live emulator suite is
//! `tests/emulator_auth.rs`, run by scripts/test-emulator.sh only.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use nsh_core::JsonValue;
use nsh_core::element::IntoElement;
use nsh_core::platform::{HttpBody, HttpRequest};
use nsh_core::prelude::*;
use nsh_engine::{Clock, EngineConfig, Root};
use nsh_firebase_auth::{Auth, AuthUser, Credential, KeyValueStore, MemoryStore};
use nsh_firebase_core::{FirebaseError, FirebaseOptions, initialize_app};
use nsh_testing::{Headless, StubNetwork, StubResponse, StubRoute};
use serde_json::json;

/// The manual clock's fixed instant; every expected timestamp below
/// derives from it.
const T0_MS: u64 = 1_767_258_000;
const SIGN_UP_URL: &str = "http://127.0.0.1:19099/identitytoolkit.googleapis.com/v1/accounts:signUp?key=demo-nsh-fake-api-key";
const SIGN_IN_URL: &str = "http://127.0.0.1:19099/identitytoolkit.googleapis.com/v1/accounts:signInWithPassword?key=demo-nsh-fake-api-key";
const CUSTOM_URL: &str = "http://127.0.0.1:19099/identitytoolkit.googleapis.com/v1/accounts:signInWithCustomToken?key=demo-nsh-fake-api-key";
const VERIFY_URL: &str = "http://127.0.0.1:19099/identitytoolkit.googleapis.com/v1/accounts:verifyCustomToken?key=demo-nsh-fake-api-key";
const REFRESH_URL: &str =
    "http://127.0.0.1:19099/securetoken.googleapis.com/v1/token?key=demo-nsh-fake-api-key";

fn t0() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(T0_MS)
}

fn reply(status: u16, body: &str) -> StubResponse {
    let mut response = StubResponse::default();
    response.status = status;
    response.body = body.as_bytes().to_vec();
    response
}

fn wire_error(wire_code: &str) -> StubResponse {
    reply(
        400,
        &json!({ "error": { "code": 400, "message": wire_code } }).to_string(),
    )
}

fn failing(message: &str) -> StubResponse {
    let mut response = StubResponse::default();
    response.error = Some(message.to_owned());
    response
}

fn ok_session(id_token: &str, refresh_token: &str, email: &str, uid: &str) -> StubResponse {
    reply(
        200,
        &json!({
            "idToken": id_token,
            "refreshToken": refresh_token,
            "expiresIn": "3600",
            "email": email,
            "localId": uid,
            "registered": true,
        })
        .to_string(),
    )
}

/// Records each request's method, URL and body while answering with
/// `response`, so a test can assert what the flow sent.
type RequestLog = Arc<Mutex<Vec<String>>>;

fn recording_route(log: &RequestLog, response: StubResponse) -> StubRoute {
    let log = Arc::clone(log);
    StubRoute::Handler(Arc::new(move |request: HttpRequest, reply_channel| {
        let body = match &request.body {
            HttpBody::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            HttpBody::Empty => String::new(),
            HttpBody::Stream => "<stream>".to_owned(),
        };
        log.lock()
            .expect("request log")
            .push(format!("{} {} body={body}", request.method, request.url));
        reply_channel.send(response.clone());
    }))
}

fn requests(log: &RequestLog) -> Vec<String> {
    log.lock().expect("request log").clone()
}

fn empty_routes() -> StubNetwork {
    StubNetwork::new(Vec::<(String, StubResponse)>::new())
}

/// A JWT whose payload carries `claims`; the code under test reads only
/// the payload, so header and signature are fixed text.
fn jwt(claims: &JsonValue) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    fn encode(bytes: &[u8]) -> String {
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                chunk.get(1).copied().unwrap_or(0),
                chunk.get(2).copied().unwrap_or(0),
            ];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            out.push(TABLE[(n >> 18) as usize & 63] as char);
            out.push(TABLE[(n >> 12) as usize & 63] as char);
            if chunk.len() > 1 {
                out.push(TABLE[(n >> 6) as usize & 63] as char);
            }
            if chunk.len() > 2 {
                out.push(TABLE[n as usize & 63] as char);
            }
        }
        out
    }
    format!("header.{}.signature", encode(claims.to_string().as_bytes()))
}

/// The store the restore component reads; its test seeds it before
/// starting the engine, as a previous launch would have written it.
static RESTORE_STORE: OnceLock<Arc<MemoryStore>> = OnceLock::new();

fn start(root: Root, network: StubNetwork) -> Headless {
    let mut config = EngineConfig::new(root);
    config.clock = Clock::manual(t0());
    config.network = Some(Arc::new(network));
    Headless::start(config).unwrap()
}

fn user_line(user: Option<&AuthUser>) -> String {
    match user {
        None => "user: none".to_owned(),
        Some(user) => format!(
            "user: {} email={} anon={} token={} refresh={} expires={} next={}",
            user.uid,
            user.email.as_deref().unwrap_or("none"),
            user.is_anonymous,
            user.id_token,
            user.refresh_token,
            user.expires_at_ms,
            user.next_refresh_at_ms,
        ),
    }
}

fn describe(result: Result<AuthUser, FirebaseError>) -> String {
    match result {
        Ok(user) => format!("ok {}", user.uid),
        Err(error) => error.to_string(),
    }
}

/// One exercise component: a button per flow, one result line, and the
/// currentUser signal as a text. Every flow runs in `spawn`, as an app's
/// handlers would.
#[component]
fn AuthApp() -> impl IntoElement {
    let app = initialize_app(FirebaseOptions::demo(), "auth-ui").unwrap();
    let store: Arc<dyn KeyValueStore> = Arc::new(MemoryStore::new());
    let auth = Arc::new(Auth::new(app, Arc::clone(&store)));
    let line = signal(String::new());
    let restored = signal("restored: never".to_owned());

    let sign_up = {
        let (auth, line) = (Arc::clone(&auth), line);
        move || {
            let (auth, line) = (Arc::clone(&auth), line);
            spawn(async move {
                let outcome = auth
                    .create_user_with_email_and_password("demo@demo-nsh.test", "fake-password-1")
                    .await;
                let text = match outcome {
                    Ok(user) => format!("signup: ok {}", user.uid),
                    // "exists", what the sandbox run falls through to
                    // sign-in on; the code decides, so the mapping stays
                    // pinned either way.
                    Err(error) if error.code == "auth/email-already-in-use" => {
                        "signup: exists".to_owned()
                    }
                    Err(error) => format!("signup: {error}"),
                };
                line.set(text);
            });
        }
    };
    let sign_in = {
        let (auth, line) = (Arc::clone(&auth), line);
        move || {
            let (auth, line) = (Arc::clone(&auth), line);
            spawn(async move {
                let outcome = auth
                    .sign_in_with_email_and_password("demo@demo-nsh.test", "fake-password-1")
                    .await;
                line.set(format!("signin: {}", describe(outcome)));
            });
        }
    };
    let sign_in_empty = {
        let (auth, line) = (Arc::clone(&auth), line);
        move || {
            let (auth, line) = (Arc::clone(&auth), line);
            spawn(async move {
                let outcome = auth.sign_in_with_email_and_password("", "").await;
                line.set(format!("empty: {}", describe(outcome)));
            });
        }
    };
    let anonymous = {
        let (auth, line) = (Arc::clone(&auth), line);
        move || {
            let (auth, line) = (Arc::clone(&auth), line);
            spawn(async move {
                let outcome = auth.sign_in_anonymously().await;
                line.set(format!("anon: {}", describe(outcome)));
            });
        }
    };
    let custom = {
        let (auth, line) = (Arc::clone(&auth), line);
        move || {
            let (auth, line) = (Arc::clone(&auth), line);
            spawn(async move {
                let outcome = auth.sign_in_with_custom_token("custom-token-abc").await;
                line.set(format!("custom: {}", describe(outcome)));
            });
        }
    };
    let credential = {
        let (auth, line) = (Arc::clone(&auth), line);
        move || {
            let (auth, line) = (Arc::clone(&auth), line);
            spawn(async move {
                let credential = Credential::IdToken("foreign-id-token".to_owned());
                let outcome = auth.sign_in_with_credential(&credential).await;
                line.set(format!("credential: {}", describe(outcome)));
            });
        }
    };
    let due = {
        let (auth, line) = (Arc::clone(&auth), line);
        move || line.set(format!("due: {}", auth.refresh_due()))
    };
    let refresh_now = {
        let (auth, line) = (Arc::clone(&auth), line);
        move || {
            let (auth, line) = (Arc::clone(&auth), line);
            spawn(async move {
                let outcome = auth.refresh_now().await;
                line.set(format!("refresh: {}", describe(outcome)));
            });
        }
    };
    let sign_out = {
        let (auth, line) = (Arc::clone(&auth), line);
        move || {
            let (auth, line) = (Arc::clone(&auth), line);
            auth.sign_out();
            line.set("signed out".to_owned());
        }
    };
    // Restore-on-init: a fresh Auth over the same store, what the app's
    // next launch does. Sync, so no spawn; the store read must already
    // have happened by the time the line renders.
    let reauth = {
        let (store, restored) = (Arc::clone(&store), restored);
        move || {
            let app = initialize_app(FirebaseOptions::demo(), "auth-ui").unwrap();
            let auth = Auth::new(app, Arc::clone(&store));
            restored.set(format!(
                "restored: {}",
                user_line(auth.current_user().as_ref())
            ));
        }
    };

    view()
        .flex_1()
        .gap_1()
        // The user line re-renders through every sign-in, refresh and
        // sign-out because it reads the currentUser signal.
        .child(text(move || user_line(auth.current_user().as_ref())))
        .child(text(move || line.get()))
        .child(text(move || restored.get()))
        .child(
            view()
                .test_id("signup")
                .on_press(sign_up)
                .child(text("sign up")),
        )
        .child(
            view()
                .test_id("signin")
                .on_press(sign_in)
                .child(text("sign in")),
        )
        .child(
            view()
                .test_id("empty")
                .on_press(sign_in_empty)
                .child(text("empty")),
        )
        .child(
            view()
                .test_id("anon")
                .on_press(anonymous)
                .child(text("anonymous")),
        )
        .child(
            view()
                .test_id("custom")
                .on_press(custom)
                .child(text("custom token")),
        )
        .child(
            view()
                .test_id("credential")
                .on_press(credential)
                .child(text("credential")),
        )
        .child(view().test_id("due").on_press(due).child(text("due?")))
        .child(
            view()
                .test_id("refresh-now")
                .on_press(refresh_now)
                .child(text("refresh")),
        )
        .child(
            view()
                .test_id("signout")
                .on_press(sign_out)
                .child(text("sign out")),
        )
        .child(
            view()
                .test_id("reauth")
                .on_press(reauth)
                .child(text("reauth")),
        )
}

/// Restore-on-init from a record a previous launch (or the TypeScript
/// tier) wrote, seeded by the test.
#[component]
fn RestoreApp() -> impl IntoElement {
    let store = RESTORE_STORE.get().expect("seeded store").clone();
    let app = initialize_app(FirebaseOptions::demo(), "auth-ui").unwrap();
    let auth = Arc::new(Auth::new(app, store));
    let ready = Arc::clone(&auth);
    view()
        .flex_1()
        .gap_1()
        .child(text(move || format!("ready: {}", ready.state_ready())))
        .child(text(move || user_line(auth.current_user().as_ref())))
}

const STORE_KEY: &str = "nsh.firebase.auth.auth-ui";

#[test]
fn sign_up_signs_in_and_persists_the_record_on_the_manual_clock() {
    let log: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let network = StubNetwork::new([(
        format!("POST {SIGN_UP_URL}"),
        recording_route(
            &log,
            ok_session("id-token-1", "refresh-1", "demo@demo-nsh.test", "uid-1"),
        ),
    )]);
    let mut app = start(Root::of::<AuthApp>(), network);
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    app.press("id:signup").unwrap();
    app.wait_for("text:signup: ok uid-1").unwrap();
    // T0 + 3600 s minus the five-minute margin, on the manual clock.
    app.wait_for(&format!(
        "text:user: uid-1 email=demo@demo-nsh.test anon=false token=id-token-1 refresh=refresh-1 expires={} next={}",
        T0_MS * 1000 + 3_600_000,
        T0_MS * 1000 + 3_600_000 - 300_000,
    ))
    .unwrap();
    // What the flow sent: JSON with returnSecureToken, the API key in the
    // query, the emulator path under the base URL.
    let sent = requests(&log);
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0].starts_with(&format!("POST {SIGN_UP_URL} ")),
        "{}",
        sent[0]
    );
    assert!(
        sent[0].contains(r#""email":"demo@demo-nsh.test""#),
        "{}",
        sent[0]
    );
    assert!(
        sent[0].contains(r#""returnSecureToken":true"#),
        "{}",
        sent[0]
    );
    // The record the store now holds is the one a later launch restores.
    app.press("id:reauth").unwrap();
    app.wait_for(
        "text:restored: user: uid-1 email=demo@demo-nsh.test anon=false token=id-token-1 refresh=refresh-1 expires=1767261600000 next=1767261300000",
    )
    .unwrap();
}

#[test]
fn sign_in_maps_the_wire_errors_and_keeps_the_user_signed_out() {
    for (wire, stable, human) in [
        (
            "EMAIL_NOT_FOUND",
            "auth/user-not-found",
            "no account with that email",
        ),
        (
            "INVALID_PASSWORD",
            "auth/wrong-password",
            "that password is wrong for the account",
        ),
    ] {
        let network = StubNetwork::new([(format!("POST {SIGN_IN_URL}"), wire_error(wire))]);
        let mut app = start(Root::of::<AuthApp>(), network);
        app.first_render().unwrap();
        assert!(app.shows("user: none"));
        app.first_render().unwrap();
        assert!(app.shows("user: none"));
        app.press("id:signin").unwrap();
        app.wait_for(&format!("text:signin: {stable}: {human} ({wire})"))
            .unwrap();
        app.first_render().unwrap();
        assert!(app.shows("user: none"));
    }
}

#[test]
fn empty_arguments_fail_before_any_request_is_made() {
    let network = StubNetwork::new([(
        format!("POST {SIGN_IN_URL}"),
        failing("the stub must never answer"),
    )]);
    let mut app = start(Root::of::<AuthApp>(), network);
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    app.press("id:empty").unwrap();
    app.wait_for("text:empty: auth/invalid-argument: email is empty")
        .unwrap();
}

#[test]
fn anonymous_sign_in_sends_no_email_and_no_password() {
    let log: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let network = StubNetwork::new([(
        format!("POST {SIGN_UP_URL}"),
        recording_route(
            &log,
            reply(
                200,
                r#"{"idToken":"anon-token","refreshToken":"anon-refresh","expiresIn":"3600","localId":"anon-uid"}"#,
            ),
        ),
    )]);
    let mut app = start(Root::of::<AuthApp>(), network);
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    app.press("id:anon").unwrap();
    app.wait_for("text:anon: ok anon-uid").unwrap();
    app.wait_for("text:user: anon-uid email=none anon=true token=anon-token refresh=anon-refresh expires=1767261600000 next=1767261300000")
        .unwrap();
    let sent = requests(&log);
    assert_eq!(sent.len(), 1);
    // The anonymous shape: returnSecureToken only, no credentials.
    assert!(
        sent[0].contains(r#""returnSecureToken":true"#),
        "{}",
        sent[0]
    );
    assert!(!sent[0].contains("password"), "{}", sent[0]);
}

#[test]
fn a_custom_token_signs_in_with_the_uid_from_the_token_claims() {
    let claims = json!({ "sub": "custom-uid", "provider_id": "custom" });
    let network = StubNetwork::new([(
        format!("POST {CUSTOM_URL}"),
        reply(
            200,
            &json!({
                "idToken": jwt(&claims),
                "refreshToken": "custom-refresh",
                "expiresIn": "3600",
            })
            .to_string(),
        ),
    )]);
    let mut app = start(Root::of::<AuthApp>(), network);
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    app.press("id:custom").unwrap();
    app.wait_for("text:custom: ok custom-uid").unwrap();
    // No email on a custom-token session: the uid came from the token's
    // sub claim, and the expiry sits on the manual clock.
    let token = jwt(&claims);
    app.wait_for(&format!(
        "text:user: custom-uid email=none anon=false token={token} refresh=custom-refresh expires=1767261600000 next=1767261300000",
    ))
    .unwrap();
}

#[test]
fn a_credential_from_a_foreign_id_token_posts_verify_custom_token() {
    let log: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let claims = json!({ "sub": "foreign-uid", "email": "other@library.test" });
    let network = StubNetwork::new([(
        format!("POST {VERIFY_URL}"),
        recording_route(
            &log,
            reply(
                200,
                &json!({
                    "idToken": jwt(&claims),
                    "refreshToken": "foreign-refresh",
                    "expiresIn": "3600",
                })
                .to_string(),
            ),
        ),
    )]);
    let mut app = start(Root::of::<AuthApp>(), network);
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    app.press("id:credential").unwrap();
    app.wait_for("text:credential: ok foreign-uid").unwrap();
    let sent = requests(&log);
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0].starts_with(&format!("POST {VERIFY_URL} ")),
        "{}",
        sent[0]
    );
    assert!(
        sent[0].contains(r#""token":"foreign-id-token""#),
        "{}",
        sent[0]
    );
}

#[test]
fn the_refresh_schedule_follows_the_moved_clock() {
    let log: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let network = StubNetwork::new([
        (
            format!("POST {SIGN_IN_URL}"),
            ok_session("id-token-1", "refresh-1", "demo@demo-nsh.test", "uid-1").into(),
        ),
        (
            format!("POST {REFRESH_URL}"),
            recording_route(
                &log,
                reply(
                    200,
                    r#"{"access_token":"id-token-2","expires_in":"3600","token_type":"Bearer","refresh_token":"refresh-2","user_id":"uid-1","project_id":"demo-nsh"}"#,
                ),
            ),
        ),
    ]);
    let mut app = start(Root::of::<AuthApp>(), network);
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    app.press("id:signin").unwrap();
    app.wait_for("text:signin: ok uid-1").unwrap();

    // The schedule-check: not due at once, due once the clock moves past
    // the stored absolute refresh time (55 minutes after sign-in).
    app.press("id:due").unwrap();
    app.wait_for("text:due: false").unwrap();
    app.set_system_time(t0() + Duration::from_secs(3_300))
        .unwrap();
    app.press("id:due").unwrap();
    app.wait_for("text:due: true").unwrap();

    app.press("id:refresh-now").unwrap();
    app.wait_for("text:refresh: ok uid-1").unwrap();
    let at = (T0_MS + 3_300) * 1000;
    app.wait_for(&format!(
        "text:user: uid-1 email=demo@demo-nsh.test anon=false token=id-token-2 refresh=refresh-2 expires={} next={}",
        at + 3_600_000,
        at + 3_600_000 - 300_000,
    ))
    .unwrap();
    let sent = requests(&log);
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0].starts_with(&format!("POST {REFRESH_URL} ")),
        "{}",
        sent[0]
    );
    assert!(sent[0].contains("grant_type=refresh_token"), "{}", sent[0]);
    assert!(sent[0].contains("refresh_token=refresh-1"), "{}", sent[0]);
}

#[test]
fn a_lost_session_maps_to_user_token_expired_and_signs_out() {
    let network = StubNetwork::new([
        (
            format!("POST {SIGN_IN_URL}"),
            ok_session("id-token-1", "refresh-1", "demo@demo-nsh.test", "uid-1"),
        ),
        (format!("POST {REFRESH_URL}"), wire_error("TOKEN_EXPIRED")),
    ]);
    let mut app = start(Root::of::<AuthApp>(), network);
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    app.press("id:signin").unwrap();
    app.wait_for("text:signin: ok uid-1").unwrap();
    app.set_system_time(t0() + Duration::from_secs(3_300))
        .unwrap();
    app.press("id:refresh-now").unwrap();
    app.wait_for(
        "text:refresh: auth/user-token-expired: the session expired server-side; sign in again (TOKEN_EXPIRED)",
    )
    .unwrap();
    // The session cannot recover, so the user is gone from the signal,
    // the store (a reauth restores nothing), and a further refresh has
    // nothing to work with.
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    app.press("id:reauth").unwrap();
    app.wait_for("text:restored: user: none").unwrap();
    app.press("id:refresh-now").unwrap();
    app.wait_for("text:refresh: auth/no-current-user: no user is signed in; nothing to refresh")
        .unwrap();
}

#[test]
fn a_network_failure_keeps_the_core_network_code() {
    let network = StubNetwork::new([(
        format!("POST {SIGN_IN_URL}"),
        failing("the connection was reset"),
    )]);
    let mut app = start(Root::of::<AuthApp>(), network);
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    app.press("id:signin").unwrap();
    app.wait_for("text:signin: core/network: the connection was reset")
        .unwrap();
}

#[test]
fn sign_out_clears_the_signal_and_the_store() {
    let network = StubNetwork::new([(
        format!("POST {SIGN_IN_URL}"),
        ok_session("id-token-1", "refresh-1", "demo@demo-nsh.test", "uid-1"),
    )]);
    let mut app = start(Root::of::<AuthApp>(), network);
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    app.press("id:signin").unwrap();
    app.wait_for("text:signin: ok uid-1").unwrap();
    app.press("id:signout").unwrap();
    app.wait_for("text:signed out").unwrap();
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    // The record left the store with it.
    app.press("id:reauth").unwrap();
    app.wait_for("text:restored: user: none").unwrap();
    app.press("id:refresh-now").unwrap();
    app.wait_for("text:refresh: auth/no-current-user: no user is signed in; nothing to refresh")
        .unwrap();
}

#[test]
fn a_second_auth_restores_the_record_another_tier_wrote() {
    // The camelCase record the TypeScript side writes, handed to the Rust
    // side verbatim: either tier restores the other's session record.
    RESTORE_STORE
        .set(Arc::new(MemoryStore::seeded([(
            STORE_KEY.to_owned(),
            r#"{"v":1,"uid":"stored-uid","email":"stored@demo-nsh.test","isAnonymous":false,"idToken":"stored-id-token","refreshToken":"stored-refresh","expiresAtMs":1999999999999,"nextRefreshAtMs":1999999699999}"#.to_owned(),
        )])))
        .ok();
    let mut app = start(Root::of::<RestoreApp>(), empty_routes());
    app.first_render().unwrap();
    assert!(app.shows("ready: true"));
    app.wait_for("text:user: stored-uid email=stored@demo-nsh.test anon=false token=stored-id-token refresh=stored-refresh expires=1999999999999 next=1999999699999")
        .unwrap();
}

#[test]
fn sign_up_refuses_an_already_registered_email() {
    let network = StubNetwork::new([(format!("POST {SIGN_UP_URL}"), wire_error("EMAIL_EXISTS"))]);
    let mut app = start(Root::of::<AuthApp>(), network);
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    app.press("id:signup").unwrap();
    // "exists" prints only when the error carried the stable
    // auth/email-already-in-use code, so this still pins the mapping.
    app.wait_for("text:signup: exists").unwrap();
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
}

#[test]
fn a_deleted_user_signs_out_on_refresh() {
    // USER_NOT_FOUND reaches refresh only from Secure Token, where it
    // means the account was deleted server-side: the session cannot
    // recover, so the user must not stay signed in.
    let network = StubNetwork::new([
        (
            format!("POST {SIGN_IN_URL}"),
            ok_session("id-token-1", "refresh-1", "demo@demo-nsh.test", "uid-1"),
        ),
        (format!("POST {REFRESH_URL}"), wire_error("USER_NOT_FOUND")),
    ]);
    let mut app = start(Root::of::<AuthApp>(), network);
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    app.press("id:signin").unwrap();
    app.wait_for("text:signin: ok uid-1").unwrap();
    app.press("id:refresh-now").unwrap();
    app.wait_for(
        "text:refresh: auth/user-not-found: no such account on the server (USER_NOT_FOUND)",
    )
    .unwrap();
    app.first_render().unwrap();
    assert!(app.shows("user: none"));
    app.press("id:reauth").unwrap();
    app.wait_for("text:restored: user: none").unwrap();
}
