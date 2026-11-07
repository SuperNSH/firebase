//! The facade's types: the stable code, the wire names, the recordable
//! error, and the custom-key rule the SDK enforces.

use nsh_firebase_core::error::FirebaseError;

/// The pure tier's one stable code. Product-scoped, not
/// `core/unavailable`: every degradable product reports that shared code,
/// Crashlytics refuses with its own — there is no REST to be honest
/// with, so the pure tier is a failing facade, not a degraded recorder.
pub const UNAVAILABLE: &str = "crashlytics/unavailable";

/// The pure tier's one failure message, verbatim in the contract both
/// language facades carry.
pub const NEEDS_NATIVE_TIER: &str = "Firebase Crashlytics needs the native tier: iOS/Android host with firebase.crashlytics.* handlers (Crashlytics has no REST — no honest pure tier exists)";

/// The handler names the native tier registers; the app declares
/// `firebase.crashlytics.*` under `capabilities.host` for them.
pub mod wire {
    pub const LOG: &str = "firebase.crashlytics.log";
    pub const SET_USER_ID: &str = "firebase.crashlytics.setUserId";
    pub const SET_CUSTOM_KEY: &str = "firebase.crashlytics.setCustomKey";
    pub const RECORD_ERROR: &str = "firebase.crashlytics.recordError";
    pub const SEND_UNSENT_REPORTS: &str = "firebase.crashlytics.sendUnsentReports";
    pub const DID_CRASH: &str = "firebase.crashlytics.didCrashDuringPreviousExecution";
}

/// The non-fatal
/// [`Crashlytics::record_error`](crate::Crashlytics::record_error)
/// records: `{code, message, stack?}` on the wire. The stack is text the
/// caller already has — the facade never captures one itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordError {
    /// The error's numeric code (an HTTP status, an app error number).
    pub code: i64,
    /// What went wrong.
    pub message: String,
    /// The stack text, when the caller has one; rides only when set.
    pub stack: Option<String>,
}

impl RecordError {
    /// A recordable error with the code and message; chain
    /// [`RecordError::stack`] to add the stack.
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            stack: None,
        }
    }

    /// Attaches the stack text.
    pub fn stack(mut self, stack: impl Into<String>) -> Self {
        self.stack = Some(stack.into());
        self
    }
}

/// The SDK's custom-key value types (string, number, boolean). Anything
/// else is refused before the host: a serialized object would arrive on
/// the far side of the wire as a different type than the caller sent.
pub(crate) fn is_custom_value(value: &JsonValue) -> bool {
    value.is_string() || value.is_number() || value.is_boolean()
}

/// The pure tier's one error: every call names what is missing. A
/// different native code (a refusal, a handler's failure) is never
/// rewritten into this.
pub(crate) fn unavailable_error() -> FirebaseError {
    FirebaseError {
        code: UNAVAILABLE.to_owned(),
        message: NEEDS_NATIVE_TIER.to_owned(),
        native_code: Some("ERR_FEATURE_UNAVAILABLE_ON_PLATFORM".to_owned()),
    }
}
