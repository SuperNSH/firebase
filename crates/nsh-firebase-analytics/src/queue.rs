//! The batching queue's state machine, pure and synchronous so the tests
//! (and the client) drive it with any clock. It owns: the bounded pending
//! queue, the in-flight batch a failed or cancelled send retries, the
//! consent gate, the user identity, and the send schedule. The client
//! locks it only across synchronous work, never across a transport
//! await.

use std::collections::VecDeque;

use serde_json::{Map, Value as JsonValue};

use crate::model::{AnalyticsConfig, AnalyticsEvent, Batch, Consent, ConsentStatus};

/// What [`QueueState::take_action`] decided is due.
pub(crate) enum Action {
    /// Nothing is due (or consent withholds sending).
    None,
    /// Send this batch — either a fresh drain (moved into `in_flight`
    /// first, so a cancelled poll retries it) or a retry of one.
    Send { batch: Batch },
}

pub(crate) struct QueueState {
    pub config: AnalyticsConfig,
    /// The storage-backed client id, written once by the client.
    pub client_id: String,
    queue: VecDeque<AnalyticsEvent>,
    in_flight: Option<Batch>,
    /// Retry attempts of the current in-flight batch.
    attempt: u32,
    /// Earliest time the next send may fire: the interval after a
    /// success, `now` on a size trigger, backoff after a failure.
    pub next_send_at_ms: u64,
    pub consent: Consent,
    pub user_id: Option<String>,
    pub user_properties: JsonValue,
    pub dropped_events: u64,
}

impl QueueState {
    pub fn new(config: AnalyticsConfig, next_send_at_ms: u64) -> Self {
        Self {
            config,
            client_id: String::new(),
            queue: VecDeque::new(),
            in_flight: None,
            attempt: 0,
            next_send_at_ms,
            consent: Consent::default(),
            user_id: None,
            user_properties: JsonValue::Object(Map::new()),
            dropped_events: 0,
        }
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
            + self
                .in_flight
                .as_ref()
                .map_or(0, |batch| batch.events.len())
    }

    /// Queues `event`, unless consent denies collection, in which case it
    /// is dropped on the floor (the SDK's own behavior with
    /// `analytics_storage: denied`). Hitting the batch size makes the
    /// next poll due immediately.
    pub fn push(&mut self, event: AnalyticsEvent, now_ms: u64) {
        if self.consent.analytics_storage == ConsentStatus::Denied {
            self.dropped_events += 1;
            return;
        }
        if self.queue.len() >= self.config.max_queue_size {
            self.queue.pop_front();
            self.dropped_events += 1;
        }
        self.queue.push_back(event);
        if self.queue.len() >= self.config.max_batch_size {
            self.next_send_at_ms = now_ms;
        }
    }

    /// Whether a send may fire now. `force` (an explicit flush) skips the
    /// schedule, never the consent gate.
    pub fn take_action(&mut self, now_ms: u64, force: bool) -> Action {
        if self.consent.analytics_storage == ConsentStatus::Denied {
            return Action::None;
        }
        if let Some(in_flight) = self.in_flight.clone() {
            // A retry waits for its backoff; a flush does not cut it
            // short, because the earlier batch may already have landed.
            if now_ms >= self.next_send_at_ms {
                return Action::Send { batch: in_flight };
            }
            return Action::None;
        }
        let full = self.queue.len() >= self.config.max_batch_size;
        if self.queue.is_empty() || !(force || full || now_ms >= self.next_send_at_ms) {
            return Action::None;
        }
        let events: Vec<_> = self
            .queue
            .drain(..self.config.max_batch_size.min(self.queue.len()))
            .collect();
        let batch = Batch {
            client_id: self.client_id.clone(),
            user_id: self.user_id.clone(),
            user_properties: self.user_properties_mp(),
            non_personalized_ads: self.consent.non_personalized_ads(),
            events,
        };
        self.in_flight = Some(batch.clone());
        Action::Send { batch }
    }

    pub fn record_success(&mut self, now_ms: u64) {
        self.in_flight = None;
        self.attempt = 0;
        self.next_send_at_ms = now_ms.saturating_add(self.config.flush_interval_ms);
    }

    /// Backoff after a failure; past the retry cap the batch is dropped
    /// and the schedule resumes (an analytics queue must not block the
    /// app on a dead transport). Duplicates are possible either way — a
    /// send that failed after landing retries by design, which native
    /// analytics tolerates too.
    pub fn record_failure(&mut self, now_ms: u64) {
        let Some(batch) = &self.in_flight else {
            return;
        };
        self.attempt += 1;
        if self.attempt > self.config.max_retries {
            self.dropped_events += batch.events.len() as u64;
            self.in_flight = None;
            self.attempt = 0;
            self.next_send_at_ms = now_ms.saturating_add(self.config.flush_interval_ms);
            return;
        }
        let shift = self.attempt.saturating_sub(1).min(16);
        let backoff = self
            .config
            .retry_base_ms
            .saturating_mul(1u64 << shift)
            .min(self.config.retry_max_ms);
        self.next_send_at_ms = now_ms.saturating_add(backoff);
    }

    /// Consent updates merge, matching the TS client and gtag's
    /// consent-update semantics: analytics storage always comes from the
    /// new call, each optional flag keeps its previous value until one
    /// is stated.
    pub fn set_consent(&mut self, consent: Consent) {
        self.consent = Consent {
            analytics_storage: consent.analytics_storage,
            ad_storage: consent.ad_storage.or(self.consent.ad_storage),
            ad_user_data: consent.ad_user_data.or(self.consent.ad_user_data),
            ad_personalization: consent
                .ad_personalization
                .or(self.consent.ad_personalization),
        };
    }

    pub fn set_user_id(&mut self, user_id: Option<&str>) {
        self.user_id = user_id.map(str::to_owned);
    }

    /// `key`/`value` were validated by the caller; later calls for a key
    /// win, so the map always holds the merged properties.
    pub fn set_user_property(&mut self, key: &str, value: JsonValue) {
        if let Some(object) = self.user_properties.as_object_mut() {
            object.insert(key.to_owned(), value);
        }
    }

    /// The MP shape: `{"key": {"value": ...}}`.
    pub fn user_properties_mp(&self) -> JsonValue {
        let mut shaped = Map::new();
        if let Some(object) = self.user_properties.as_object() {
            for (key, value) in object {
                shaped.insert(key.clone(), serde_json::json!({ "value": value }));
            }
        }
        JsonValue::Object(shaped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config() -> AnalyticsConfig {
        AnalyticsConfig {
            flush_interval_ms: 10_000,
            max_batch_size: 3,
            max_queue_size: 6,
            retry_base_ms: 1_000,
            retry_max_ms: 8_000,
            max_retries: 2,
        }
    }

    fn event(name: &str, now_ms: u64) -> AnalyticsEvent {
        AnalyticsEvent::new(name, json!({}), now_ms)
    }

    #[test]
    fn a_full_batch_is_due_immediately_and_bounded() {
        // First send scheduled at 10_000: only the size trigger can fire
        // earlier.
        let mut state = QueueState::new(config(), 10_000);
        state.client_id = "cid".to_owned();
        state.push(event("a", 1), 1);
        state.push(event("b", 2), 2);
        assert!(matches!(state.take_action(1, false), Action::None));
        state.push(event("c", 3), 3);
        let Action::Send { batch } = state.take_action(3, false) else {
            panic!("a full batch must be due");
        };
        assert_eq!(batch.events.len(), 3);
        assert_eq!(batch.events[0].name, "a");
    }

    #[test]
    fn the_queue_bounded_drops_the_oldest_and_counts() {
        let mut state = QueueState::new(config(), 0);
        state.client_id = "cid".to_owned();
        for i in 0..8_u64 {
            state.push(event(&format!("e{i}"), i), i);
        }
        assert_eq!(state.dropped_events, 2);
        assert_eq!(state.pending(), 6);
    }

    #[test]
    fn denied_consent_drops_new_events_and_blocks_sends() {
        let mut state = QueueState::new(config(), 0);
        state.client_id = "cid".to_owned();
        state.push(event("a", 1), 1);
        state.set_consent(Consent {
            analytics_storage: ConsentStatus::Denied,
            ..Consent::default()
        });
        state.push(event("b", 2), 2);
        assert_eq!(state.dropped_events, 1);
        assert!(matches!(state.take_action(9_999, true), Action::None));
    }

    #[test]
    fn consent_optionals_merge_across_calls_like_the_ts_client() {
        let mut state = QueueState::new(config(), 0);
        state.client_id = "cid".to_owned();
        state.set_consent(Consent {
            analytics_storage: ConsentStatus::Granted,
            ad_storage: Some(ConsentStatus::Denied),
            ad_personalization: Some(ConsentStatus::Denied),
            ..Consent::default()
        });
        // Re-stating analytics storage alone keeps the standing optional
        // denials, as the TS client's consent update does.
        state.set_consent(Consent::default());
        state.push(event("a", 1), 1);
        state.push(event("b", 2), 2);
        state.push(event("c", 3), 3);
        let Action::Send { batch } = state.take_action(3, false) else {
            panic!("a full batch must be due after re-granting");
        };
        assert!(
            batch.non_personalized_ads,
            "the standing ad-personalization denial must survive an analytics-storage-only update"
        );
    }

    #[test]
    fn a_failure_schedules_backoff_then_the_cap_drops_the_batch() {
        let mut state = QueueState::new(config(), 0);
        state.client_id = "cid".to_owned();
        state.push(event("a", 1), 1);
        state.push(event("b", 2), 2);
        state.push(event("c", 3), 3);
        let Action::Send { .. } = state.take_action(3, false) else {
            panic!("full batch due");
        };
        state.record_failure(3);
        // Backoff after the first failure: 1_000 ms, so due at 1_003.
        assert!(matches!(state.take_action(1_002, false), Action::None));
        let Action::Send { batch } = state.take_action(1_003, false) else {
            panic!("backoff elapsed at 1003");
        };
        // The retry resends the whole earlier batch.
        assert_eq!(batch.events.len(), 3);
        state.record_failure(1_003);
        // Backoff doubles: 2_000 ms, due at 3_003.
        let Action::Send { .. } = state.take_action(3_003, false) else {
            panic!("second backoff elapsed");
        };
        state.record_failure(3_003);
        // The cap (max_retries: 2) dropped the batch.
        assert_eq!(state.dropped_events, 3);
        assert_eq!(state.pending(), 0);
        assert!(matches!(state.take_action(3_002, false), Action::None));
    }

    #[test]
    fn user_properties_merge_and_shape_for_mp() {
        let mut state = QueueState::new(config(), 0);
        state.set_user_property("difficulty", json!("hard"));
        state.set_user_property("difficulty", json!("easy"));
        state.set_user_property("level", json!(7));
        assert_eq!(
            state.user_properties_mp(),
            json!({"difficulty": {"value": "easy"}, "level": {"value": 7}})
        );
    }
}
