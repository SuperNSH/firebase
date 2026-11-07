//! The facade itself: calls ride the app's [`FirebaseHost`], events fan
//! out from an [`EventBus`] the Rust embedding host feeds through
//! [`Messaging::dispatch`] (the TypeScript side reaches `host.on` of the
//! runtime directly, so it needs no seam).

use std::sync::Arc;

use serde_json::{Value as JsonValue, json};

use nsh_firebase_core::{
    bus::{EventBus, ListenerId},
    error::{FirebaseError, HOST_ERROR, INVALID_ARGUMENT, UNAVAILABLE},
    host::FirebaseHost,
};

use crate::types::{Event, MessageNotification, Permission, unavailable_error, wire};

/// The messaging facade over one host. Cheap to clone; the listeners are
/// shared across the clones.
#[derive(Clone)]
pub struct Messaging {
    host: Arc<dyn FirebaseHost>,
    events: Arc<EventBus>,
}

impl Messaging {
    /// The messaging of the host the embedding app injected: a
    /// [`nsh_firebase_core::host::NativeHost`] where the native tier
    /// registered the handlers, a [`nsh_firebase_core::host::PureHost`]
    /// everywhere else.
    pub fn new(host: Arc<dyn FirebaseHost>) -> Self {
        Self {
            host,
            events: Arc::new(EventBus::new()),
        }
    }

    /// The one call path: a no-host refusal becomes the error that names
    /// what is missing; every other failure keeps the platform's code and
    /// message.
    async fn call(&self, name: &str, args: JsonValue) -> Result<JsonValue, FirebaseError> {
        match self.host.call(name, args).await {
            Ok(value) => Ok(value),
            Err(error) if error.code == UNAVAILABLE => Err(unavailable_error()),
            Err(error) => Err(error),
        }
    }

    /// The FCM registration token. Rejects with the host's own message
    /// when the request fails, so a fake project's registration failure
    /// reaches the app unchanged.
    pub async fn token(&self) -> Result<String, FirebaseError> {
        let reply = self.call(wire::TOKEN, JsonValue::Null).await?;
        reply
            .get("token")
            .and_then(JsonValue::as_str)
            .filter(|token| !token.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                FirebaseError::new(
                    HOST_ERROR,
                    "`firebase.messaging.token` answered without a token",
                )
            })
    }

    /// The permission without asking.
    pub async fn permission(&self) -> Result<Permission, FirebaseError> {
        self.read_permission(wire::PERMISSION).await
    }

    /// The permission, showing the system's prompt the first time.
    pub async fn request_permission(&self) -> Result<Permission, FirebaseError> {
        self.read_permission(wire::REQUEST_PERMISSION).await
    }

    async fn read_permission(&self, name: &str) -> Result<Permission, FirebaseError> {
        let reply = self.call(name, JsonValue::Null).await?;
        let raw = reply
            .get("permission")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        Permission::from_name(raw).ok_or_else(|| {
            FirebaseError::new(
                HOST_ERROR,
                format!("`{name}` answered `{raw}`, which is not a permission name"),
            )
        })
    }

    /// Shows a local notification under `id`; showing again with the same
    /// id replaces it. Resolves with whether the system took it (the iOS
    /// host answers `{shown}`, the Android host the bare boolean). The
    /// id and the title must be non-empty, as the Notifications module
    /// this mirrors requires; the body may be empty, which it shows as
    /// none.
    pub async fn show(&self, notification: &MessageNotification) -> Result<bool, FirebaseError> {
        if notification.id.is_empty() || notification.title.is_empty() {
            return Err(FirebaseError::new(
                INVALID_ARGUMENT,
                "messaging.show needs a non-empty id and title",
            ));
        }
        let reply = self
            .call(
                wire::SHOW,
                json!({
                    "id": notification.id,
                    "title": notification.title,
                    "body": notification.body,
                    "silent": notification.silent,
                }),
            )
            .await?;
        Ok(match reply {
            JsonValue::Bool(shown) => shown,
            reply => reply
                .get("shown")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false),
        })
    }

    /// Removes the notification called `id`, shown or waiting to show.
    pub async fn close(&self, id: &str) -> Result<(), FirebaseError> {
        if id.is_empty() {
            return Err(FirebaseError::new(
                INVALID_ARGUMENT,
                "messaging.close needs a non-empty id",
            ));
        }
        self.call(wire::CLOSE, json!({ "id": id })).await?;
        Ok(())
    }

    /// Shows `count` on the app's icon; 0 clears it.
    pub async fn set_badge(&self, count: u32) -> Result<(), FirebaseError> {
        self.call(wire::SET_BADGE, json!({ "count": count }))
            .await?;
        Ok(())
    }

    pub async fn subscribe(&self, topic: &str) -> Result<(), FirebaseError> {
        if topic.is_empty() {
            return Err(FirebaseError::new(
                INVALID_ARGUMENT,
                "messaging.subscribe needs a non-empty topic",
            ));
        }
        self.call(wire::SUBSCRIBE, json!({ "topic": topic }))
            .await?;
        Ok(())
    }

    pub async fn unsubscribe(&self, topic: &str) -> Result<(), FirebaseError> {
        if topic.is_empty() {
            return Err(FirebaseError::new(
                INVALID_ARGUMENT,
                "messaging.unsubscribe needs a non-empty topic",
            ));
        }
        self.call(wire::UNSUBSCRIBE, json!({ "topic": topic }))
            .await?;
        Ok(())
    }

    /// Subscribes to `event`; the id stops it again with
    /// [`Messaging::off`]. Listeners registered before the host delivers
    /// see every event the host has for them. The pure tier rejects: a
    /// listener that never fires is the silent no-op this facade refuses
    /// to be.
    pub fn on(
        &self,
        event: Event,
        listener: impl Fn(&JsonValue) + Send + Sync + 'static,
    ) -> Result<ListenerId, FirebaseError> {
        if !self.host.available() {
            return Err(unavailable_error());
        }
        Ok(self.events.on(event.name(), listener))
    }

    /// Removes one listener; one that never subscribed changes nothing.
    pub fn off(&self, id: ListenerId) {
        self.events.off(id);
    }

    /// The seam the embedding host delivers host→app events through:
    /// `true` when `event` was one of the facade's and reached its
    /// listeners, so a host can forward everything and ignore the rest.
    /// Payloads pass through untouched.
    pub fn dispatch(&self, event: &str, payload: &JsonValue) -> bool {
        match Event::from_host_event(event) {
            Some(event) => {
                self.events.emit(event.name(), payload);
                true
            }
            None => false,
        }
    }
}
