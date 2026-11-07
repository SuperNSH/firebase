//! Integration tests of the analytics client on a manual clock with the
//! in-memory transport: batching at the size limit, interval flushes,
//! the consent gate, retry-then-success, screen-view shapes and user
//! property merging. No real network and no real time.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use nsh_firebase_analytics::{
    Analytics, AnalyticsConfig, Clock, Consent, ConsentStatus, InMemoryTransport, ManualClock,
    MemoryStorage, Transport, validate_event_name, validate_params, validate_user_property,
};
use nsh_firebase_core::{FirebaseError, error::NETWORK, initialize_app};
use serde_json::json;

fn app() -> Arc<nsh_firebase_core::FirebaseApp> {
    initialize_app(
        nsh_firebase_core::FirebaseOptions::demo(),
        "analytics-tests",
    )
    .unwrap()
}

fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(1_000_000))
}

fn config() -> AnalyticsConfig {
    AnalyticsConfig {
        flush_interval_ms: 10_000,
        max_batch_size: 5,
        max_queue_size: 12,
        retry_base_ms: 1_000,
        retry_max_ms: 8_000,
        max_retries: 3,
    }
}

fn client_with(clock: &Arc<ManualClock>) -> (Analytics, Arc<InMemoryTransport>) {
    Analytics::recording(
        &app(),
        Arc::clone(clock) as Arc<dyn Clock>,
        Arc::new(MemoryStorage::new()),
        config(),
    )
    .unwrap()
}

/// A transport whose first `failures` sends fail, for the retry test.
struct FlakyTransport {
    failures: AtomicU32,
}

impl Transport for FlakyTransport {
    fn send<'a>(
        &'a self,
        _batch: &'a nsh_firebase_analytics::Batch,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), FirebaseError>> + 'a>> {
        let mut current = self.failures.load(Ordering::SeqCst);
        let will_fail = loop {
            if current == 0 {
                break false;
            }
            match self.failures.compare_exchange(
                current,
                current - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break true,
                Err(actual) => current = actual,
            }
        };
        Box::pin(async move {
            if will_fail {
                Err(FirebaseError::new(NETWORK, "transport down"))
            } else {
                Ok(())
            }
        })
    }
}

#[tokio::test]
async fn events_batch_at_max_batch_size_without_waiting_for_the_interval() {
    let clock = clock();
    let (client, transport) = client_with(&clock);
    for level in 0..5_i64 {
        client
            .log_event("level_up", Some(json!({ "level": level })))
            .unwrap();
    }
    assert_eq!(client.pending(), 5);
    // The fifth event filled the batch: a poll sends immediately, well
    // before the 10_000 ms interval.
    client.poll(clock.now_ms()).await.unwrap();
    let batches = transport.batches();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].events.len(), 5);
    assert_eq!(batches[0].events[4].params, json!({ "level": 4 }));
    assert_eq!(client.pending(), 0);
    // Events after the flush wait for the interval, not the (passed)
    // size trigger.
    client.log_event("level_up", None).unwrap();
    client.poll(clock.now_ms()).await.unwrap();
    assert_eq!(transport.batches().len(), 1);
}

#[tokio::test]
async fn events_flush_on_the_interval_and_no_earlier() {
    let clock = clock();
    let (client, transport) = client_with(&clock);
    client.log_event("quest_done", None).unwrap();
    client.poll(clock.now_ms()).await.unwrap();
    assert!(
        transport.batches().is_empty(),
        "the interval has not elapsed"
    );
    clock.advance(9_999);
    client.poll(clock.now_ms()).await.unwrap();
    assert!(transport.batches().is_empty(), "one millisecond early");
    clock.advance(1);
    client.poll(clock.now_ms()).await.unwrap();
    let batches = transport.batches();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].events[0].name, "quest_done");
    // The next interval starts at the flush.
    assert_eq!(client.next_send_at_ms(), 1_000_000 + 10_000 + 10_000);
}

#[tokio::test]
async fn denied_consent_blocks_the_transport_until_it_is_reganted() {
    let clock = clock();
    let (client, transport) = client_with(&clock);
    client.log_event("pre_denial", None).unwrap();
    client.set_consent(Consent {
        analytics_storage: ConsentStatus::Denied,
        ad_storage: Some(ConsentStatus::Denied),
        ..Consent::default()
    });
    // Nothing new queues while denied.
    client.log_event("while_denied", None).unwrap();
    clock.advance(60_000);
    client.poll(clock.now_ms()).await.unwrap();
    client.flush().await.unwrap();
    assert!(
        transport.batches().is_empty(),
        "denied consent must stop every transport send"
    );
    assert_eq!(client.dropped(), 1, "the event logged while denied");
    assert_eq!(client.pending(), 1, "the queued event is held, not sent");
    // Regranting resumes: the held event goes out on the next poll.
    client.set_consent(Consent::default());
    client.poll(clock.now_ms()).await.unwrap();
    let batches = transport.batches();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].events[0].name, "pre_denial");
}

#[tokio::test]
async fn a_failed_send_retries_with_backoff_and_then_succeeds() {
    let clock = clock();
    let transport = Arc::new(FlakyTransport {
        failures: AtomicU32::new(1),
    });
    let client = Analytics::with_parts(
        &app(),
        Arc::clone(&transport) as Arc<dyn Transport>,
        Arc::clone(&clock) as Arc<dyn nsh_firebase_analytics::Clock>,
        Arc::new(MemoryStorage::new()),
        config(),
    )
    .unwrap();
    client.log_event("retry_me", None).unwrap();
    clock.advance(10_000);
    let error = client.poll(clock.now_ms()).await.unwrap_err();
    assert_eq!(error.code, NETWORK);
    assert_eq!(client.pending(), 1, "the failed batch is held for retry");
    // Halfway through the 1_000 ms backoff: no send.
    clock.advance(500);
    client.poll(clock.now_ms()).await.unwrap();
    assert_eq!(client.pending(), 1);
    // Backoff elapsed: the retry succeeds with the same event.
    clock.advance(500);
    client.poll(clock.now_ms()).await.unwrap();
    assert_eq!(client.pending(), 0);
}

#[tokio::test]
async fn screen_views_carry_the_ga4_param_names() {
    let clock = clock();
    let (client, transport) = client_with(&clock);
    client
        .log_screen_view("Inventory", Some("InventoryScreen"))
        .unwrap();
    client.log_screen_view("Map", None).unwrap();
    client.flush().await.unwrap();
    let batches = transport.batches();
    assert_eq!(batches.len(), 1);
    let events = &batches[0].events;
    assert_eq!(events[0].name, "screen_view");
    assert_eq!(
        events[0].params,
        json!({ "screen_name": "Inventory", "screen_class": "InventoryScreen" })
    );
    assert_eq!(events[1].params, json!({ "screen_name": "Map" }));
}

#[tokio::test]
async fn user_properties_and_the_user_id_merge_into_every_batch() {
    let clock = clock();
    let (client, transport) = client_with(&clock);
    client
        .set_user_property("difficulty", json!("hard"))
        .unwrap();
    client
        .set_user_property("difficulty", json!("easy"))
        .unwrap();
    client.set_user_property("guild", json!("nsh")).unwrap();
    client.set_user_id(Some("user-41"));
    client.log_event("quest_done", None).unwrap();
    client.flush().await.unwrap();
    let batches = transport.batches();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].user_id.as_deref(), Some("user-41"));
    // The MP shape, with the second difficulty winning the merge.
    assert_eq!(
        batches[0].user_properties,
        json!({ "difficulty": { "value": "easy" }, "guild": { "value": "nsh" } })
    );
    // Clearing the user id clears it in the next batch too.
    client.set_user_id(None);
    client.log_event("quest_done", None).unwrap();
    client.flush().await.unwrap();
    assert_eq!(transport.batches()[1].user_id, None);
}

#[tokio::test]
async fn the_client_id_is_stable_across_clients_over_one_storage() {
    let storage = Arc::new(MemoryStorage::new());
    let (first, first_transport) = Analytics::recording(
        &app(),
        Arc::clone(&clock()) as Arc<dyn Clock>,
        Arc::clone(&storage) as Arc<dyn nsh_firebase_analytics::KeyValueStorage>,
        config(),
    )
    .unwrap();
    let (second, second_transport) = Analytics::recording(
        &app(),
        Arc::clone(&clock()) as Arc<dyn Clock>,
        Arc::clone(&storage) as Arc<dyn nsh_firebase_analytics::KeyValueStorage>,
        config(),
    )
    .unwrap();
    assert_eq!(first.client_id(), second.client_id());
    second.log_event("from_second", None).unwrap();
    second.flush().await.unwrap();
    assert_eq!(
        second_transport.batches()[0].client_id,
        first.client_id(),
        "batches carry the shared id"
    );
    assert!(
        first_transport.batches().is_empty(),
        "the first client sent nothing"
    );
}

#[tokio::test]
async fn an_overfull_queue_drops_the_oldest_and_counts_them() {
    let clock = clock();
    let (client, transport) = client_with(&clock);
    for i in 0..15_i64 {
        client
            .log_event("queue_filler", Some(json!({ "i": i })))
            .unwrap();
    }
    assert_eq!(client.dropped(), 3, "15 queued into a 12-slot queue");
    // Two full batches drain back to back; the remainder waits.
    client.poll(clock.now_ms()).await.unwrap();
    client.poll(clock.now_ms()).await.unwrap();
    let sent: i64 = transport
        .batches()
        .iter()
        .map(|batch| batch.events.len() as i64)
        .sum();
    assert_eq!(sent, 10);
    assert_eq!(client.pending(), 2);
}

#[test]
fn validation_refuses_bad_names_params_and_properties() {
    assert!(validate_event_name("level_up").is_ok());
    assert!(validate_event_name("session_start").is_err());
    assert!(validate_event_name("firebase_screen").is_err());
    assert!(validate_event_name("2fast").is_err());
    assert!(validate_event_name(&"a".repeat(41)).is_err());
    assert!(validate_params(Some(&json!({ "ok": 1 }))).is_ok());
    assert!(validate_params(Some(&json!({ "deep": { "x": 1 } }))).is_err());
    assert!(validate_user_property("guild", &json!("nsh")).is_ok());
    assert!(validate_user_property("google_guild", &json!("nsh")).is_err());
    // And the client surfaces the same errors through logEvent.
    let clock = clock();
    let (client, _transport) = client_with(&clock);
    let error = client.log_event("session_start", None).unwrap_err();
    assert_eq!(error.code, "core/invalid-argument");
}
