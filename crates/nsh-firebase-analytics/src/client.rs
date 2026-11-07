//! The analytics client: the pure tier's queue owner and the native
//! tier's post router. The queue is driven by [`Analytics::poll`] with a
//! clock the caller controls (manual in tests, the system clock in an
//! app), so flushing and retries are deterministic and no timer thread
//! exists.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use serde_json::Value as JsonValue;

use crate::{
    clock::{Clock, SystemClock},
    model::{AnalyticsConfig, AnalyticsEvent, Consent},
    queue::{Action, QueueState},
    storage::{KeyValueStorage, MemoryStorage, ensure_client_id},
    transport::{CollectorTransport, InMemoryTransport, MeasurementProtocolTransport, Transport},
    validate::{validate_event_name, validate_params, validate_user_property},
};
use nsh_firebase_core::{FirebaseApp, FirebaseError, FirebaseHost};

/// The `firebase.analytics.*` handler names the native areas register;
/// [`NativeAnalytics`] posts to exactly these.
pub const NATIVE_LOG_EVENT: &str = "firebase.analytics.logEvent";
pub const NATIVE_SET_USER_PROPERTY: &str = "firebase.analytics.setUserProperty";
pub const NATIVE_SET_USER_ID: &str = "firebase.analytics.setUserId";
pub const NATIVE_LOG_SCREEN_VIEW: &str = "firebase.analytics.logScreenView";
pub const NATIVE_SET_CONSENT: &str = "firebase.analytics.setConsent";
pub const NATIVE_SET_DEBUG_VIEW: &str = "firebase.analytics.setDebugViewEnabled";

/// Resets the send-in-progress flag no matter how the poll future ends,
/// including cancellation mid-await.
struct SendingGuard<'a>(&'a AtomicBool);

impl Drop for SendingGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// The pure tier's analytics client: validation, the consent gate, the
/// bounded batching queue with retrying sends, and the pluggable
/// transport.
pub struct Analytics {
    app: Arc<FirebaseApp>,
    transport: Arc<dyn Transport>,
    clock: Arc<dyn Clock>,
    state: Mutex<QueueState>,
    sending: AtomicBool,
}

impl Analytics {
    /// A client over `transport` with the system clock and in-memory
    /// storage (the client id then lives for the process only).
    pub fn new(
        app: &Arc<FirebaseApp>,
        transport: Arc<dyn Transport>,
        config: AnalyticsConfig,
    ) -> Result<Self, FirebaseError> {
        Self::with_parts(
            app,
            transport,
            Arc::new(SystemClock),
            Arc::new(MemoryStorage::new()),
            config,
        )
    }

    /// The client tests drive: any clock and storage.
    pub fn with_parts(
        app: &Arc<FirebaseApp>,
        transport: Arc<dyn Transport>,
        clock: Arc<dyn Clock>,
        storage: Arc<dyn KeyValueStorage>,
        config: AnalyticsConfig,
    ) -> Result<Self, FirebaseError> {
        config.validate()?;
        let client_id = ensure_client_id(storage.as_ref());
        let next_send_at = clock.now_ms().saturating_add(config.flush_interval_ms);
        let mut state = QueueState::new(config, next_send_at);
        state.client_id = client_id;
        Ok(Self {
            app: Arc::clone(app),
            transport,
            clock,
            state: Mutex::new(state),
            sending: AtomicBool::new(false),
        })
    }

    /// The client the sandbox's collector receives events from.
    pub fn with_collector(
        app: &Arc<FirebaseApp>,
        endpoint: impl Into<String>,
        config: AnalyticsConfig,
    ) -> Result<Self, FirebaseError> {
        Self::new(app, Arc::new(CollectorTransport::new(endpoint)), config)
    }

    /// The client GA4's Measurement Protocol receives events from. See
    /// [`crate::transport::MeasurementProtocolTransport`] for the
    /// protocol's limits.
    pub fn with_measurement_protocol(
        app: &Arc<FirebaseApp>,
        base_url: impl Into<String>,
        api_secret: impl Into<String>,
        measurement_id: impl Into<String>,
        config: AnalyticsConfig,
    ) -> Result<Self, FirebaseError> {
        Self::new(
            app,
            Arc::new(MeasurementProtocolTransport::new(
                base_url,
                api_secret,
                measurement_id,
            )),
            config,
        )
    }

    /// An in-memory-transport client over the given clock and storage,
    /// for tests that assert batches directly.
    pub fn recording(
        app: &Arc<FirebaseApp>,
        clock: Arc<dyn Clock>,
        storage: Arc<dyn KeyValueStorage>,
        config: AnalyticsConfig,
    ) -> Result<(Self, Arc<InMemoryTransport>), FirebaseError> {
        let transport = Arc::new(InMemoryTransport::new());
        let client = Self::with_parts(
            app,
            Arc::clone(&transport) as Arc<dyn Transport>,
            clock,
            storage,
            config,
        )?;
        Ok((client, transport))
    }

    /// Validates and queues `name`; sending happens in [`Self::poll`].
    /// While consent denies analytics storage the event is dropped and
    /// counted, matching the SDK's collection-off behavior.
    pub fn log_event(&self, name: &str, params: Option<JsonValue>) -> Result<(), FirebaseError> {
        validate_event_name(name)?;
        validate_params(params.as_ref())?;
        let event = AnalyticsEvent::new(
            name,
            params.unwrap_or_else(|| JsonValue::Object(serde_json::Map::new())),
            self.clock.now_ms(),
        );
        self.lock().push(event, self.clock.now_ms());
        Ok(())
    }

    /// `screen_view` with GA4's `screen_name`/`screen_class` params.
    pub fn log_screen_view(
        &self,
        screen_name: &str,
        screen_class: Option<&str>,
    ) -> Result<(), FirebaseError> {
        let mut params = serde_json::json!({ "screen_name": screen_name });
        if let Some(class) = screen_class {
            params["screen_class"] = JsonValue::from(class);
        }
        self.log_event("screen_view", Some(params))
    }

    /// Merges one property; a later call for the same key wins.
    pub fn set_user_property(&self, key: &str, value: JsonValue) -> Result<(), FirebaseError> {
        validate_user_property(key, &value)?;
        self.lock().set_user_property(key, value);
        Ok(())
    }

    /// `None` clears the id.
    pub fn set_user_id(&self, user_id: Option<&str>) {
        self.lock().set_user_id(user_id);
    }

    /// The gate: while `analytics_storage` is denied, nothing queued
    /// sends and nothing new queues.
    pub fn set_consent(&self, consent: Consent) {
        self.lock().set_consent(consent);
    }

    /// Pure tier: a documented no-op. There is no debug view to switch
    /// on without the SDK; callers keep the flag for their own logging.
    /// The native tier routes it through [`NativeAnalytics`].
    pub fn set_debug_view_enabled(&self, _enabled: bool) {}

    /// The app the client was built for (its options and event bus).
    pub fn app(&self) -> &Arc<FirebaseApp> {
        &self.app
    }

    /// The client id this client stamps every batch with, stable across
    /// clients over the same storage.
    pub fn client_id(&self) -> String {
        self.lock().client_id.clone()
    }

    /// Events held (queued or in flight).
    pub fn pending(&self) -> usize {
        self.lock().pending()
    }

    pub fn dropped(&self) -> u64 {
        self.lock().dropped_events
    }

    /// Earliest time the next send may fire (the clock's ms).
    pub fn next_send_at_ms(&self) -> u64 {
        self.lock().next_send_at_ms
    }

    /// The scheduler tick: sends one batch if one is due (interval
    /// elapsed, queue full, or a retry's backoff elapsed), gated on
    /// consent. Concurrent and reentrant calls are safe: only one send
    /// runs at a time, and a cancelled poll leaves its batch queued for
    /// the next poll's retry.
    pub async fn poll(&self, now_ms: u64) -> Result<(), FirebaseError> {
        self.poll_inner(now_ms, false).await
    }

    /// Sends now, consent permitting, even if the interval has not
    /// elapsed and the batch is partial. A batch already in flight keeps
    /// its retry schedule.
    pub async fn flush(&self) -> Result<(), FirebaseError> {
        self.poll_inner(self.clock.now_ms(), true).await
    }

    async fn poll_inner(&self, now_ms: u64, force: bool) -> Result<(), FirebaseError> {
        if self.sending.swap(true, Ordering::SeqCst) {
            // A send is running; its completion reschedules the queue.
            return Ok(());
        }
        let _guard = SendingGuard(&self.sending);
        let action = self.lock().take_action(now_ms, force);
        let Action::Send { batch } = action else {
            return Ok(());
        };
        // The lock is released before the await: the transport may take
        // as long as the network does.
        let result = self.transport.send(&batch).await;
        match result {
            Ok(()) => self.lock().record_success(now_ms),
            Err(error) => {
                self.lock().record_failure(now_ms);
                return Err(error);
            }
        }
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, QueueState> {
        self.state.lock().expect("analytics state lock")
    }
}

/// The native tier's analytics: the same validation, posted to the
/// `firebase.analytics.*` handlers the native areas register. A refused
/// or failed post surfaces as its [`FirebaseError`].
pub struct NativeAnalytics {
    host: Arc<dyn FirebaseHost>,
}

impl NativeAnalytics {
    pub fn new(host: Arc<dyn FirebaseHost>) -> Self {
        Self { host }
    }

    pub fn log_event(&self, name: &str, params: Option<JsonValue>) -> Result<(), FirebaseError> {
        validate_event_name(name)?;
        validate_params(params.as_ref())?;
        self.host.post(
            NATIVE_LOG_EVENT,
            serde_json::json!({
                "name": name,
                "params": params.unwrap_or_else(|| JsonValue::Object(serde_json::Map::new())),
            }),
        )
    }

    pub fn log_screen_view(
        &self,
        screen_name: &str,
        screen_class: Option<&str>,
    ) -> Result<(), FirebaseError> {
        self.host.post(
            NATIVE_LOG_SCREEN_VIEW,
            serde_json::json!({
                "screenName": screen_name,
                "screenClass": screen_class,
            }),
        )
    }

    pub fn set_user_property(&self, key: &str, value: JsonValue) -> Result<(), FirebaseError> {
        validate_user_property(key, &value)?;
        self.host.post(
            NATIVE_SET_USER_PROPERTY,
            serde_json::json!({ "key": key, "value": value }),
        )
    }

    pub fn set_user_id(&self, user_id: Option<&str>) -> Result<(), FirebaseError> {
        self.host
            .post(NATIVE_SET_USER_ID, serde_json::json!({ "userId": user_id }))
    }

    pub fn set_consent(&self, consent: &Consent) -> Result<(), FirebaseError> {
        let status = |status: Option<crate::model::ConsentStatus>| {
            status.map(|status| match status {
                crate::model::ConsentStatus::Granted => "granted",
                crate::model::ConsentStatus::Denied => "denied",
            })
        };
        self.host.post(
            NATIVE_SET_CONSENT,
            serde_json::json!({
                "analyticsStorage": status(Some(consent.analytics_storage)),
                "adStorage": status(consent.ad_storage),
                "adUserData": status(consent.ad_user_data),
                "adPersonalization": status(consent.ad_personalization),
            }),
        )
    }

    pub fn set_debug_view_enabled(&self, enabled: bool) -> Result<(), FirebaseError> {
        self.host.post(
            NATIVE_SET_DEBUG_VIEW,
            serde_json::json!({ "enabled": enabled }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use crate::model::ConsentStatus;
    use nsh_firebase_core::error::{INVALID_ARGUMENT, NETWORK};
    use serde_json::json;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering::SeqCst;

    fn app() -> Arc<FirebaseApp> {
        nsh_firebase_core::initialize_app(
            nsh_firebase_core::FirebaseOptions::demo(),
            "analytics-client-test",
        )
        .unwrap()
    }

    /// Records what a native tier would hand the embedding host.
    #[derive(Default)]
    struct RecordingHost {
        posts: Mutex<Vec<(String, JsonValue)>>,
    }

    impl RecordingHost {
        fn posts(&self) -> Vec<(String, JsonValue)> {
            self.posts.lock().expect("recording host lock").clone()
        }
    }

    impl FirebaseHost for RecordingHost {
        fn call(&self, name: &str, _args: JsonValue) -> nsh_firebase_core::host::HostCallFuture {
            let name = name.to_owned();
            Box::pin(async move {
                Err(FirebaseError::from_host_code(
                    "ERR_NSH_HOST_ERROR",
                    format!("`{name}` unhandled"),
                ))
            })
        }

        fn post(&self, name: &str, args: JsonValue) -> Result<(), FirebaseError> {
            self.posts
                .lock()
                .expect("recording host lock")
                .push((name.to_owned(), args));
            Ok(())
        }
    }

    fn recording_native() -> (NativeAnalytics, Arc<RecordingHost>) {
        let host = Arc::new(RecordingHost::default());
        let native = NativeAnalytics::new(Arc::clone(&host) as Arc<dyn FirebaseHost>);
        (native, host)
    }

    #[test]
    fn native_log_event_posts_the_handler_payload() {
        let (native, host) = recording_native();
        native.log_event("level_up", None).unwrap();
        assert_eq!(
            host.posts(),
            vec![(
                NATIVE_LOG_EVENT.to_owned(),
                json!({"name": "level_up", "params": {}}),
            )]
        );
    }

    #[test]
    fn native_set_consent_posts_all_four_keys_with_nulls() {
        let (native, host) = recording_native();
        native
            .set_consent(&Consent {
                analytics_storage: ConsentStatus::Granted,
                ad_storage: Some(ConsentStatus::Denied),
                ..Consent::default()
            })
            .unwrap();
        assert_eq!(
            host.posts(),
            vec![(
                NATIVE_SET_CONSENT.to_owned(),
                json!({
                    "analyticsStorage": "granted",
                    "adStorage": "denied",
                    "adUserData": null,
                    "adPersonalization": null,
                }),
            )]
        );
    }

    #[test]
    fn native_screen_view_nulls_the_absent_class() {
        let (native, host) = recording_native();
        native.log_screen_view("Map", None).unwrap();
        native
            .log_screen_view("Inventory", Some("InventoryScreen"))
            .unwrap();
        assert_eq!(
            host.posts(),
            vec![
                (
                    NATIVE_LOG_SCREEN_VIEW.to_owned(),
                    json!({"screenName": "Map", "screenClass": null}),
                ),
                (
                    NATIVE_LOG_SCREEN_VIEW.to_owned(),
                    json!({"screenName": "Inventory", "screenClass": "InventoryScreen"}),
                ),
            ]
        );
    }

    #[test]
    fn native_validation_rejects_before_any_post() {
        let (native, host) = recording_native();
        let error = native.log_event("session_start", None).unwrap_err();
        assert_eq!(error.code, INVALID_ARGUMENT);
        let error = native
            .set_user_property("google_guild", json!("nsh"))
            .unwrap_err();
        assert_eq!(error.code, INVALID_ARGUMENT);
        assert!(host.posts().is_empty(), "a rejected call must not post");
    }

    /// A transport whose first `failures` sends fail.
    struct FlakyTransport {
        failures: AtomicU32,
    }

    impl Transport for FlakyTransport {
        fn send<'a>(
            &'a self,
            _batch: &'a crate::model::Batch,
        ) -> Pin<Box<dyn Future<Output = Result<(), FirebaseError>> + 'a>> {
            // Consume one of the configured failures, if any remain.
            let mut current = self.failures.load(SeqCst);
            let will_fail = loop {
                if current == 0 {
                    break false;
                }
                match self
                    .failures
                    .compare_exchange(current, current - 1, SeqCst, SeqCst)
                {
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
    async fn interval_flush_is_clock_driven() {
        let clock = Arc::new(ManualClock::new(0));
        let (client, transport) = Analytics::recording(
            &app(),
            Arc::clone(&clock) as Arc<dyn Clock>,
            Arc::new(MemoryStorage::new()),
            AnalyticsConfig::default(),
        )
        .unwrap();
        client
            .log_event("level_up", Some(json!({"level": 2})))
            .unwrap();
        client.poll(clock.now_ms()).await.unwrap();
        assert_eq!(transport.batches().len(), 0, "the interval has not elapsed");
        clock.advance(10_000);
        client.poll(clock.now_ms()).await.unwrap();
        let batches = transport.batches();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].events[0].name, "level_up");
    }

    #[tokio::test]
    async fn consent_denied_blocks_the_transport_and_regrant_resumes() {
        let clock = Arc::new(ManualClock::new(0));
        let (client, transport) = Analytics::recording(
            &app(),
            Arc::clone(&clock) as Arc<dyn Clock>,
            Arc::new(MemoryStorage::new()),
            AnalyticsConfig::default(),
        )
        .unwrap();
        client.log_event("level_up", None).unwrap();
        client.set_consent(Consent {
            analytics_storage: ConsentStatus::Denied,
            ..Consent::default()
        });
        clock.advance(60_000);
        client.poll(clock.now_ms()).await.unwrap();
        client.flush().await.unwrap();
        assert!(
            transport.batches().is_empty(),
            "denied consent must stop sends"
        );
        client.set_consent(Consent::default());
        client.poll(clock.now_ms()).await.unwrap();
        assert_eq!(transport.batches().len(), 1);
    }

    #[tokio::test]
    async fn retry_then_success_after_backoff() {
        let clock = Arc::new(ManualClock::new(0));
        let transport = Arc::new(FlakyTransport {
            failures: AtomicU32::new(1),
        });
        let client = Analytics::with_parts(
            &app(),
            Arc::clone(&transport) as Arc<dyn Transport>,
            Arc::clone(&clock) as Arc<dyn Clock>,
            Arc::new(MemoryStorage::new()),
            AnalyticsConfig::default(),
        )
        .unwrap();
        client.log_event("level_up", None).unwrap();
        clock.advance(10_000);
        let error = client.poll(clock.now_ms()).await.unwrap_err();
        assert_eq!(error.code, NETWORK);
        // The retry waits for its backoff even though the interval has
        // long elapsed.
        clock.advance(500);
        client.poll(clock.now_ms()).await.unwrap();
        assert_eq!(client.pending(), 1, "the batch waits out its backoff");
        clock.advance(500);
        client.poll(clock.now_ms()).await.unwrap();
        assert_eq!(client.pending(), 0, "the retry landed");
    }

    #[tokio::test]
    async fn screen_views_and_user_data_shape_the_batch() {
        let clock = Arc::new(ManualClock::new(0));
        let (client, transport) = Analytics::recording(
            &app(),
            Arc::clone(&clock) as Arc<dyn Clock>,
            Arc::new(MemoryStorage::new()),
            AnalyticsConfig::default(),
        )
        .unwrap();
        client.set_user_id(Some("user-7"));
        client
            .set_user_property("difficulty", json!("hard"))
            .unwrap();
        client
            .log_screen_view("Settings", Some("SettingsScreen"))
            .unwrap();
        client.flush().await.unwrap();
        let batches = transport.batches();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].user_id.as_deref(), Some("user-7"));
        assert_eq!(
            batches[0].user_properties,
            json!({"difficulty": {"value": "hard"}})
        );
        assert_eq!(batches[0].events[0].name, "screen_view");
        assert_eq!(
            batches[0].events[0].params,
            json!({"screen_name": "Settings", "screen_class": "SettingsScreen"})
        );
    }

    #[tokio::test]
    async fn the_client_id_survives_a_new_client_over_one_storage() {
        let storage = Arc::new(MemoryStorage::new());
        let clock = Arc::new(ManualClock::new(0));
        let (first, _) = Analytics::recording(
            &app(),
            Arc::clone(&clock) as Arc<dyn Clock>,
            Arc::clone(&storage) as Arc<dyn KeyValueStorage>,
            AnalyticsConfig::default(),
        )
        .unwrap();
        let (second, _) = Analytics::recording(
            &app(),
            Arc::clone(&clock) as Arc<dyn Clock>,
            Arc::clone(&storage) as Arc<dyn KeyValueStorage>,
            AnalyticsConfig::default(),
        )
        .unwrap();
        assert_eq!(first.client_id(), second.client_id());
    }
}
