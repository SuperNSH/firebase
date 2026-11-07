//! The facade itself: the explicit calls ride the app's [`FirebaseHost`].
//! No host→app events and no queue: the native SDK already captures
//! crashes on its own, so nothing flows back and nothing is buffered
//! here — buffering crash reports without a reporter behind it would be
//! a fake recorder, the one thing this facade refuses to be.

use std::sync::Arc;

use serde_json::{Value as JsonValue, json};

use nsh_firebase_core::{
    error::{HOST_ERROR, INVALID_ARGUMENT, FirebaseError},
    host::FirebaseHost,
};

use crate::types::{RecordError, is_custom_value, unavailable_error, wire};

/// The crashlytics facade over one host. Cheap to clone.
#[derive(Clone)]
pub struct Crashlytics {
    host: Arc<dyn FirebaseHost>,
}

impl Crashlytics {
    /// The crashlytics of the host the embedding app injected: a
    /// [`nsh_firebase_core::host::NativeHost`] where the native tier
    /// registered the handlers, a [`nsh_firebase_core::host::PureHost`]
    /// everywhere else.
    pub fn new(host: Arc<dyn FirebaseHost>) -> Self {
        Self { host }
    }

    /// The one call path: a no-host refusal (or a host whose crashlytics
    /// handlers are missing) becomes
    /// [`crate::types::UNAVAILABLE`]; every other failure keeps the
    /// platform's code and message.
    async fn call(&self, name: &str, args: JsonValue) -> Result<JsonValue, FirebaseError> {
        match self.host.call(name, args).await {
            Ok(value) => Ok(value),
            Err(error) if error.code == nsh_firebase_core::error::UNAVAILABLE => {
                Err(unavailable_error())
            }
            Err(error) => Err(error),
        }
    }

    /// Adds a breadcrumb shown alongside the crash report.
    pub async fn log(&self, message: &str) -> Result<(), FirebaseError> {
        self.call(wire::LOG, json!({ "message": message })).await?;
        Ok(())
    }

    /// Attributes the reports to `id` (an empty id clears it, as the
    /// SDK's own call does).
    pub async fn set_user_id(&self, id: &str) -> Result<(), FirebaseError> {
        self.call(wire::SET_USER_ID, json!({ "id": id })).await?;
        Ok(())
    }

    /// Sets one custom key. The value must be a string, number or
    /// boolean — the SDK's own key types; anything else is refused
    /// before the host with `core/invalid-argument`.
    pub async fn set_attribute(&self, key: &str, value: JsonValue) -> Result<(), FirebaseError> {
        if !is_custom_value(&value) {
            return Err(FirebaseError::new(
                INVALID_ARGUMENT,
                "crashlytics custom keys take a string, number or boolean",
            ));
        }
        self.call(
            wire::SET_CUSTOM_KEY,
            json!({ "key": key, "value": value }),
        )
        .await?;
        Ok(())
    }

    /// One call per key, in the slice's order; an empty slice calls
    /// nothing.
    pub async fn set_attributes(
        &self,
        attributes: &[(String, JsonValue)],
    ) -> Result<(), FirebaseError> {
        for (key, value) in attributes {
            self.set_attribute(key, value.clone()).await?;
        }
        Ok(())
    }

    /// Records a non-fatal as `{code, message, stack?}`; the stack rides
    /// only when the caller gave one.
    pub async fn record_error(&self, error: &RecordError) -> Result<(), FirebaseError> {
        let mut args = json!({ "code": error.code, "message": &error.message });
        if let Some(stack) = &error.stack {
            args["stack"] = json!(stack);
        }
        self.call(wire::RECORD_ERROR, args).await?;
        Ok(())
    }

    /// Asks the native SDK to send reports it has unsent; what it holds
    /// and when it sends is the SDK's own state machine.
    pub async fn send_unsent_reports(&self) -> Result<(), FirebaseError> {
        self.call(wire::SEND_UNSENT_REPORTS, JsonValue::Null).await?;
        Ok(())
    }

    /// Whether the previous run ended in a crash (`{crashed}`). A reply
    /// without the flag is a host error, not a silent `false` — a silent
    /// false would tell the app "no crash" when the host said nothing.
    pub async fn did_crash_on_previous_execution(&self) -> Result<bool, FirebaseError> {
        let reply = self.call(wire::DID_CRASH, JsonValue::Null).await?;
        reply
            .get("crashed")
            .and_then(JsonValue::as_bool)
            .ok_or_else(|| {
                FirebaseError::new(
                    HOST_ERROR,
                    format!("`{}` answered without a crashed flag", wire::DID_CRASH),
                )
            })
    }
}
