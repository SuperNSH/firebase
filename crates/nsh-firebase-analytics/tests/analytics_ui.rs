//! Headless UI tests of the collector transport over the engine's
//! stubbed network: the queue's batch reaches `POST /collect` through
//! the engine's `fetch`, and a refused send surfaces as a stable error
//! with the events still queued. Manual clock, no real network.

use std::{sync::Arc, time::Duration};

use nsh_core::prelude::*;
use nsh_engine::{Clock, EngineConfig, Root};
use nsh_firebase_analytics::{
    Analytics, AnalyticsConfig, CollectorTransport, ManualClock, MemoryStorage,
};
use nsh_firebase_core::initialize_app;
use nsh_testing::{Headless, StubNetwork, StubResponse};
use serde_json::json;

const COLLECTOR: &str = "http://127.0.0.1:18742";

fn start(root: Root, network: StubNetwork) -> Headless {
    let mut config = EngineConfig::new(root);
    config.clock =
        Clock::manual(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_258_000));
    config.network = Some(Arc::new(network));
    Headless::start(config).unwrap()
}

fn collector_response() -> StubResponse {
    let mut response = StubResponse::default();
    response.status = 200;
    response.body = b"{}".to_vec();
    response
}

fn collector_error() -> StubResponse {
    let mut response = StubResponse::default();
    response.error = Some("connection refused".to_owned());
    response
}

#[component]
fn CollectorApp() -> impl IntoElement {
    let line = signal(String::new());
    let app = initialize_app(nsh_firebase_core::FirebaseOptions::demo(), "collector-ui").unwrap();
    // with_parts, not with_collector: the client's clock must be the
    // test's ManualClock, not the wall clock with_collector hardwires.
    let analytics = Arc::new(
        Analytics::with_parts(
            &app,
            Arc::new(CollectorTransport::new(COLLECTOR)),
            Arc::new(ManualClock::new(0)),
            Arc::new(MemoryStorage::new()),
            AnalyticsConfig::default(),
        )
        .unwrap(),
    );
    let (log_analytics, log_line) = (Arc::clone(&analytics), line);
    let (flush_analytics, flush_line) = (Arc::clone(&analytics), line);
    view()
        .flex_1()
        .gap_1()
        .child(text(move || line.get()))
        .child(
            view()
                .test_id("log")
                .on_press(move || {
                    log_analytics
                        .log_event("collector_event", Some(json!({ "source": "ui" })))
                        .unwrap();
                    log_analytics
                        .set_user_property("tier", json!("pure"))
                        .unwrap();
                    log_line.set(format!("pending {}", log_analytics.pending()));
                })
                .child(text("log")),
        )
        .child(
            view()
                .test_id("flush")
                .on_press(move || {
                    let (analytics, line) = (Arc::clone(&flush_analytics), flush_line);
                    spawn(async move {
                        match analytics.flush().await {
                            Ok(()) => line.set(format!("sent, pending {}", analytics.pending())),
                            Err(error) => line.set(format!("failed: {error}")),
                        }
                    });
                })
                .child(text("flush")),
        )
}

#[test]
fn the_collector_transport_posts_through_the_engine() {
    let network = StubNetwork::new([(format!("POST {COLLECTOR}/collect"), collector_response())]);
    let mut app = start(Root::of::<CollectorApp>(), network);
    // `start` can return before the first frame, so wait for something.
    app.wait_for("text:").unwrap();
    app.press("id:log").unwrap();
    app.wait_for("text:pending 1").unwrap();
    app.press("id:flush").unwrap();
    app.wait_for("text:sent, pending 0").unwrap();
}

#[test]
fn a_refused_collector_send_fails_with_the_events_still_queued() {
    let network = StubNetwork::new([(format!("POST {COLLECTOR}/collect"), collector_error())]);
    let mut app = start(Root::of::<CollectorApp>(), network);
    app.wait_for("text:").unwrap();
    app.press("id:log").unwrap();
    app.press("id:flush").unwrap();
    app.wait_for("text:failed: core/network: connection refused")
        .unwrap();
    // The failed batch is held for the retry backoff, not lost.
    app.press("id:log").unwrap();
    app.wait_for("text:pending 2").unwrap();
}
