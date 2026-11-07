//! The facade's types: the permission names, the content of a local
//! notification, the events, and the wire names they ride.

use nsh_firebase_core::error::FirebaseError;

/// The pure tier's one failure, verbatim in the contract both language
/// facades carry.
pub const NEEDS_NATIVE_TIER: &str =
    "Firebase Messaging needs the native tier: iOS/Android host with firebase.messaging.* handlers";

/// The handler names the native tier registers; the app declares
/// `firebase.messaging.*` under `capabilities.host` for them.
pub mod wire {
    pub const TOKEN: &str = "firebase.messaging.token";
    pub const PERMISSION: &str = "firebase.messaging.permission";
    pub const REQUEST_PERMISSION: &str = "firebase.messaging.requestPermission";
    pub const SHOW: &str = "firebase.messaging.show";
    pub const CLOSE: &str = "firebase.messaging.close";
    pub const SET_BADGE: &str = "firebase.messaging.setBadge";
    pub const SUBSCRIBE: &str = "firebase.messaging.subscribe";
    pub const UNSUBSCRIBE: &str = "firebase.messaging.unsubscribe";
}

/// Whether the user lets the app show notifications. The strings are the
/// unreleased NSH Notifications module's, so the released module later
/// drops in behind the same names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Permission {
    /// The user was not asked yet.
    #[default]
    Default,
    /// Notifications show.
    Granted,
    /// The user refused: only the system's settings change that.
    Denied,
    /// This platform or build shows none.
    Unavailable,
}

impl Permission {
    /// Its name in the hosts' answers.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Granted => "granted",
            Self::Denied => "denied",
            Self::Unavailable => "unavailable",
        }
    }

    /// The permission named `name` ([`Self::as_str`]).
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Default,
            Self::Granted,
            Self::Denied,
            Self::Unavailable,
        ]
        .into_iter()
        .find(|permission| permission.as_str() == name)
    }
}

/// What [`Messaging::show`](crate::Messaging::show) shows. The same shape
/// as the unreleased Notifications module's `Notification`, so moving to
/// the released module later is one small change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageNotification {
    /// The app's name for it: showing another with the same id replaces
    /// it, and a press reports it.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its text.
    pub body: String,
    /// Shows it without a sound or vibration.
    pub silent: bool,
}

impl MessageNotification {
    /// A notification called `id`, titled `title`, with `body`.
    pub fn new(id: impl Into<String>, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            body: body.into(),
            silent: false,
        }
    }

    /// Shows it without a sound or vibration ([`Self::silent`]).
    pub fn silent(mut self, silent: bool) -> Self {
        self.silent = silent;
        self
    }
}

/// The host→app events the facade turns into listener calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The user pressed a notification: `{ "id": ... }`.
    Press,
    /// An FCM message arrived: the message payload as the native tier
    /// serializes it.
    Message,
    /// The token was refreshed: `{ "token": ... }`.
    Token,
}

impl Event {
    /// The listener-side name (`messaging.on`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Press => "press",
            Self::Message => "message",
            Self::Token => "token",
        }
    }

    /// The wire name the native tier sends.
    pub const fn host_event(self) -> &'static str {
        match self {
            Self::Press => "firebase.messaging.press",
            Self::Message => "firebase.messaging.message",
            Self::Token => "firebase.messaging.tokenRefreshed",
        }
    }

    /// The event whose wire name is `name`.
    pub fn from_host_event(name: &str) -> Option<Self> {
        [Self::Press, Self::Message, Self::Token]
            .into_iter()
            .find(|event| event.host_event() == name)
    }
}

/// The pure tier's one error: every call names what is missing. A
/// different native code (a refusal, a handler's failure) is never
/// rewritten into this.
pub(crate) fn unavailable_error() -> FirebaseError {
    FirebaseError::from_host_code("ERR_FEATURE_UNAVAILABLE_ON_PLATFORM", NEEDS_NATIVE_TIER)
}
