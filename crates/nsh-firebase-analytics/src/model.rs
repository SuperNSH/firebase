//! The data shapes one analytics client passes around: events, the batch
//! a transport receives, consent, and the queue's configuration.

use serde_json::Value as JsonValue;

use nsh_firebase_core::{FirebaseError, error::INVALID_ARGUMENT};

/// One logged event: a validated name, flat params, and the log-time
/// stamp in milliseconds (the clock's, so tests control it).
#[derive(Debug, Clone, PartialEq)]
pub struct AnalyticsEvent {
    pub name: String,
    pub params: JsonValue,
    pub timestamp_ms: u64,
}

impl AnalyticsEvent {
    pub fn new(name: impl Into<String>, params: JsonValue, timestamp_ms: u64) -> Self {
        Self {
            name: name.into(),
            params,
            timestamp_ms,
        }
    }

    /// GA4's per-event payload: `timestamp_micros` is ours times 1000.
    pub fn to_mp_event(&self) -> JsonValue {
        let mut event = serde_json::json!({
            "name": self.name,
            "params": self.params,
        });
        event["timestamp_micros"] = JsonValue::from(self.timestamp_ms as i64 * 1000);
        event
    }
}

/// What a [`crate::transport::Transport`] receives: one drained slice of
/// the queue with the identity the transports attach.
#[derive(Debug, Clone, PartialEq)]
pub struct Batch {
    pub client_id: String,
    pub user_id: Option<String>,
    /// MP-shaped: `{"key": {"value": ...}}`.
    pub user_properties: JsonValue,
    /// From consent: an ad-personalization denial forces this true in
    /// the MP body.
    pub non_personalized_ads: bool,
    pub events: Vec<AnalyticsEvent>,
}

impl Batch {
    /// The GA4 Measurement Protocol request body.
    pub fn to_mp_body(&self) -> JsonValue {
        let mut body = serde_json::json!({
            "client_id": self.client_id,
            "user_properties": self.user_properties,
            "events": self
                .events
                .iter()
                .map(AnalyticsEvent::to_mp_event)
                .collect::<Vec<_>>(),
            "non_personalized_ads": self.non_personalized_ads,
        });
        if let Some(user_id) = &self.user_id {
            body["user_id"] = JsonValue::from(user_id.as_str());
        }
        body
    }

    /// One NDJSON line per event, the shape the collector transport
    /// posts: identity first, then the event, so a collector can assert
    /// on a substring of one line.
    pub fn to_collector_lines(&self) -> String {
        self.events
            .iter()
            .map(|event| {
                let mut line = serde_json::json!({
                    "client_id": self.client_id,
                    "user_properties": self.user_properties,
                    "name": event.name,
                    "params": event.params,
                    "timestamp_ms": event.timestamp_ms,
                });
                if let Some(user_id) = &self.user_id {
                    line["user_id"] = JsonValue::from(user_id.as_str());
                }
                line.to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// `granted` or `denied`, as `setConsent` takes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConsentStatus {
    #[default]
    Granted,
    Denied,
}

impl ConsentStatus {
    pub fn parse(value: &str) -> Result<Self, FirebaseError> {
        match value {
            "granted" => Ok(Self::Granted),
            "denied" => Ok(Self::Denied),
            other => Err(FirebaseError::new(
                INVALID_ARGUMENT,
                format!("setConsent: expected \"granted\" or \"denied\", got \"{other}\""),
            )),
        }
    }
}

/// The consent flags `setConsent` takes; the optional ones stay as they
/// were until set. Only `analytics_storage` gates this queue's sends,
/// which is the one flag the pure tier's own pipeline honors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Consent {
    pub analytics_storage: ConsentStatus,
    pub ad_storage: Option<ConsentStatus>,
    pub ad_user_data: Option<ConsentStatus>,
    pub ad_personalization: Option<ConsentStatus>,
}

impl Consent {
    /// MP's `non_personalized_ads` is forced when ad personalization was
    /// denied; otherwise it follows the platform default (true on the
    /// web stream without signals, so the honest sandbox sends `true`
    /// only on an explicit denial).
    pub fn non_personalized_ads(&self) -> bool {
        self.ad_personalization == Some(ConsentStatus::Denied)
    }
}

/// The queue's sizing and retry policy, deliberately bounded: 20 per
/// batch stays under the GA4 Measurement Protocol's 25-event request
/// limit, and the queue holds at most 500 events (oldest dropped,
/// counted) so a misbehaving app cannot grow it without end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsConfig {
    pub flush_interval_ms: u64,
    pub max_batch_size: usize,
    pub max_queue_size: usize,
    pub retry_base_ms: u64,
    pub retry_max_ms: u64,
    pub max_retries: u32,
}

impl Default for AnalyticsConfig {
    fn default() -> Self {
        Self {
            flush_interval_ms: 10_000,
            max_batch_size: 20,
            max_queue_size: 500,
            retry_base_ms: 1_000,
            retry_max_ms: 30_000,
            max_retries: 5,
        }
    }
}

impl AnalyticsConfig {
    pub fn validate(&self) -> Result<(), FirebaseError> {
        if self.max_batch_size == 0 || self.max_batch_size > 25 {
            return Err(FirebaseError::new(
                INVALID_ARGUMENT,
                "AnalyticsConfig.max_batch_size must be between 1 and 25 (the Measurement \
                 Protocol's per-request limit)",
            ));
        }
        if self.max_queue_size < self.max_batch_size {
            return Err(FirebaseError::new(
                INVALID_ARGUMENT,
                "AnalyticsConfig.max_queue_size must hold at least one batch",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_are_deliberate() {
        let config = AnalyticsConfig::default();
        assert_eq!(config.max_batch_size, 20);
        assert!(config.max_batch_size <= 25, "the MP request limit is 25");
        config.validate().unwrap();
    }

    #[test]
    fn a_batch_over_the_mp_limit_is_refused() {
        let error = AnalyticsConfig {
            max_batch_size: 26,
            ..AnalyticsConfig::default()
        }
        .validate()
        .unwrap_err();
        assert_eq!(error.code, INVALID_ARGUMENT);
    }

    #[test]
    fn a_queue_smaller_than_a_batch_is_refused() {
        let error = AnalyticsConfig {
            max_batch_size: 20,
            max_queue_size: 10,
            ..AnalyticsConfig::default()
        }
        .validate()
        .unwrap_err();
        assert_eq!(error.code, INVALID_ARGUMENT);
    }

    #[test]
    fn consent_statuses_parse_from_the_api_strings() {
        assert_eq!(ConsentStatus::parse("granted"), Ok(ConsentStatus::Granted));
        assert_eq!(ConsentStatus::parse("denied"), Ok(ConsentStatus::Denied));
        assert_eq!(
            ConsentStatus::parse("maybe").unwrap_err().code,
            INVALID_ARGUMENT
        );
    }

    #[test]
    fn mp_body_carries_identity_and_micros() {
        let batch = Batch {
            client_id: "cid".to_owned(),
            user_id: Some("u-1".to_owned()),
            user_properties: json!({"difficulty": {"value": "hard"}}),
            non_personalized_ads: false,
            events: vec![AnalyticsEvent::new("level_up", json!({"level": 3}), 1_000)],
        };
        let body = batch.to_mp_body();
        assert_eq!(body["client_id"], "cid");
        assert_eq!(body["user_id"], "u-1");
        assert_eq!(body["events"][0]["name"], "level_up");
        assert_eq!(body["events"][0]["timestamp_micros"], 1_000_000);
        assert_eq!(body["non_personalized_ads"], false);
        // An ad-personalization denial rides the batch, not the consent
        // struct, so the transport needs no consent knowledge.
        let denied = Consent {
            ad_personalization: Some(ConsentStatus::Denied),
            ..Consent::default()
        };
        assert!(denied.non_personalized_ads());
    }

    #[test]
    fn collector_lines_are_one_json_object_per_event() {
        let batch = Batch {
            client_id: "cid".to_owned(),
            user_id: None,
            user_properties: json!({}),
            non_personalized_ads: false,
            events: vec![
                AnalyticsEvent::new("a", json!({}), 1),
                AnalyticsEvent::new("b", json!({}), 2),
            ],
        };
        let lines = batch.to_collector_lines();
        let parsed: Vec<JsonValue> = lines
            .split('\n')
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(parsed[0]["name"], "a");
        assert_eq!(parsed[1]["name"], "b");
        assert!(parsed[0].get("user_id").is_none());
    }
}
