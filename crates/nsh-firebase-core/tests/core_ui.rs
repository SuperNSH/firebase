//! Headless UI tests of the core: the demo options, the event bus, the
//! pure-tier transport over the engine's stubbed network and the tier
//! facade, all through a test-only component on a manual clock.

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, SystemTime},
};

use nsh_core::{JsonValue, prelude::*};
use nsh_engine::{Clock, EngineConfig, Root};
use nsh_firebase_core::{
    EventBus, FirebaseError, FirebaseHost, FirebaseOptions, NativeHost, PureHost, get_app,
    initialize_app, send,
};
use nsh_testing::{Headless, StubNetwork, StubResponse};
use serde_json::json;

fn start(root: Root) -> Headless {
    let mut config = EngineConfig::new(root);
    config.clock = Clock::manual(SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_258_000));
    Headless::start(config).unwrap()
}

fn start_with_network(root: Root) -> Headless {
    // `StubResponse` is `#[non_exhaustive]`: set fields on the default.
    let mut response = StubResponse::default();
    response.status = 200;
    response.body = br#"[{"id":1,"title":"one"}]"#.to_vec();
    let network =
        StubNetwork::new([("GET http://127.0.0.1:19000/todos.json".to_owned(), response)]);
    let mut config = EngineConfig::new(root);
    config.clock = Clock::manual(SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_258_000));
    config.network = Some(Arc::new(network));
    Headless::start(config).unwrap()
}

#[component]
fn OptionsApp() -> impl IntoElement {
    let options = FirebaseOptions::demo();
    let line = signal(String::new());
    let urls = options.clone();
    view()
        .flex_1()
        .gap_1()
        .child(text(move || {
            format!(
                "database: {} auth: {} storage: {}",
                urls.database_base_url().unwrap_or_default(),
                urls.auth_base_url().unwrap_or_default(),
                urls.storage_base_url().unwrap_or_default(),
            )
        }))
        .child(
            view()
                .test_id("init")
                .on_press(move || {
                    let app = initialize_app(FirebaseOptions::demo(), "core-ui").unwrap();
                    line.set(format!("app: {} {}", app.name, app.options.project_id));
                })
                .child(text("init")),
        )
        .child(text(move || line.get()))
}

#[test]
fn demo_options_resolve_the_emulator_suite_urls() {
    let mut app = start(Root::of::<OptionsApp>());
    app.wait_for(
        "text:database: http://127.0.0.1:19000 auth: http://127.0.0.1:19099 storage: http://127.0.0.1:19199",
    )
    .unwrap();
    app.press("id:init").unwrap();
    app.wait_for("text:app: core-ui demo-nsh").unwrap();
    // The same name hands back the same app.
    let again = get_app("core-ui").unwrap();
    assert_eq!(again.options.project_id, "demo-nsh");
}

#[component]
fn BusApp() -> impl IntoElement {
    let bus = Arc::new(EventBus::new());
    let a = signal(0_i64);
    let b = signal(0_i64);
    let (bus_a, a_handle) = (Arc::clone(&bus), a);
    bus_a.on("tick", move |payload: &JsonValue| {
        a_handle.update(|v| *v += payload.as_i64().unwrap_or(0));
    });
    let (bus_b, b_handle) = (Arc::clone(&bus), b);
    let b_id = bus_b.on("tick", move |payload: &JsonValue| {
        b_handle.update(|v| *v += payload.as_i64().unwrap_or(0));
    });
    let bus_emit = Arc::clone(&bus);
    let bus_off = Arc::clone(&bus);
    view()
        .flex_1()
        .gap_1()
        .child(text(move || format!("a={} b={}", a.get(), b.get())))
        .child(
            view()
                .test_id("emit")
                .on_press(move || bus_emit.emit("tick", &json!(2)))
                .child(text("emit")),
        )
        .child(
            view()
                .test_id("off-b")
                .on_press(move || bus_off.off(b_id))
                .child(text("off b")),
        )
}

#[test]
fn the_bus_fans_out_and_off_stops_one_listener() {
    let mut app = start(Root::of::<BusApp>());
    app.wait_for("text:a=0 b=0").unwrap();
    app.press("id:emit").unwrap();
    app.wait_for("text:a=2 b=2").unwrap();
    app.press("id:off-b").unwrap();
    app.press("id:emit").unwrap();
    app.wait_for("text:a=4 b=2").unwrap();
}

#[component]
fn TransportApp() -> impl IntoElement {
    let line = signal(String::new());
    view()
        .flex_1()
        .gap_1()
        .child(
            view()
                .test_id("fetch")
                .on_press(move || {
                    let line = line;
                    spawn(async move {
                        // A trailing slash on purpose: join_url must not
                        // double it, or the stub route never matches.
                        let url =
                            nsh_firebase_core::join_url("http://127.0.0.1:19000/", "todos.json");
                        match send(nsh_core::platform::HttpRequest::get(url)).await {
                            Ok(response) => line.set(format!(
                                "status: {} body: {}",
                                response.status(),
                                response.text(),
                            )),
                            Err(error) => line.set(format!("error: {error}")),
                        }
                    });
                })
                .child(text("fetch todos")),
        )
        .child(text(move || line.get()))
}

#[test]
fn the_pure_transport_goes_through_the_stubbed_network() {
    let mut app = start_with_network(Root::of::<TransportApp>());
    app.wait_for("text:fetch todos").unwrap();
    app.press("id:fetch").unwrap();
    app.wait_for(r#"text:status: 200 body: [{"id":1,"title":"one"}]"#)
        .unwrap();
}

struct DeniedChannel;

impl nsh_firebase_core::HostChannel for DeniedChannel {
    fn call(
        &self,
        name: String,
        _args: JsonValue,
    ) -> Pin<Box<dyn Future<Output = Result<JsonValue, FirebaseError>> + Send>> {
        Box::pin(async move {
            Err(FirebaseError::from_host_code(
                "ERR_ACCESS_DENIED",
                format!("`{name}` is not granted"),
            ))
        })
    }

    fn post(&self, name: String, _args: JsonValue) -> Result<(), FirebaseError> {
        Err(FirebaseError::from_host_code(
            "ERR_ACCESS_DENIED",
            format!("`{name}` is not granted"),
        ))
    }
}

#[component]
fn TierApp() -> impl IntoElement {
    let native: Arc<dyn FirebaseHost> = Arc::new(NativeHost::new(Arc::new(DeniedChannel)));
    let pure: Arc<dyn FirebaseHost> = Arc::new(PureHost);
    let line = signal(String::new());
    let (native_host, native_line) = (Arc::clone(&native), line);
    let (pure_host, pure_line) = (Arc::clone(&pure), line);
    let (post_native, post_native_line) = (Arc::clone(&native), line);
    let (post_pure, post_pure_line) = (Arc::clone(&pure), line);
    view()
        .flex_1()
        .gap_1()
        .child(text(move || line.get()))
        .child(
            view()
                .test_id("native")
                .on_press(move || {
                    let (host, line) = (Arc::clone(&native_host), native_line);
                    spawn(async move {
                        let error = host
                            .call("firebase.auth.getUser", JsonValue::Null)
                            .await
                            .unwrap_err();
                        line.set(error.to_string());
                    });
                })
                .child(text("call native")),
        )
        .child(
            view()
                .test_id("pure")
                .on_press(move || {
                    let (host, line) = (Arc::clone(&pure_host), pure_line);
                    spawn(async move {
                        let error = host
                            .call("firebase.core.ping", JsonValue::Null)
                            .await
                            .unwrap_err();
                        line.set(error.to_string());
                    });
                })
                .child(text("call pure")),
        )
        .child(
            view()
                .test_id("post-native")
                .on_press(move || {
                    let line = post_native_line;
                    let error = post_native
                        .post("firebase.analytics.logEvent", JsonValue::Null)
                        .unwrap_err();
                    line.set(error.to_string());
                })
                .child(text("post native")),
        )
        .child(
            view()
                .test_id("post-pure")
                .on_press(move || {
                    let line = post_pure_line;
                    let error = post_pure
                        .post("firebase.analytics.logEvent", JsonValue::Null)
                        .unwrap_err();
                    line.set(error.to_string());
                })
                .child(text("post pure")),
        )
}

#[test]
fn the_tier_facade_surfaces_stable_codes_through_the_ui() {
    let mut app = start(Root::of::<TierApp>());
    app.wait_for("text:call native").unwrap();
    app.press("id:native").unwrap();
    app.wait_for(
        "text:core/access-denied (ERR_ACCESS_DENIED): `firebase.auth.getUser` is not granted",
    )
    .unwrap();
    app.press("id:pure").unwrap();
    app.wait_for("text:core/unavailable (ERR_FEATURE_UNAVAILABLE_ON_PLATFORM): no host answers `firebase.core.ping` in the pure tier; use the product's fetch transport")
        .unwrap();
    app.press("id:post-native").unwrap();
    app.wait_for(
        "text:core/access-denied (ERR_ACCESS_DENIED): `firebase.analytics.logEvent` is not granted",
    )
    .unwrap();
    app.press("id:post-pure").unwrap();
    app.wait_for("text:core/unavailable (ERR_FEATURE_UNAVAILABLE_ON_PLATFORM): no host answers `firebase.analytics.logEvent` in the pure tier; use the product's fetch transport")
        .unwrap();
}
