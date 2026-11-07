//! The event bus, shared by every product. The TypeScript package's
//! `EventBus` (packages/nsh-firebase/src/core/bus.ts) keeps the same
//! behavior: listeners run in subscription order over a snapshot taken at
//! `emit`, and an `off` during an emit affects only later emits.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde_json::Value as JsonValue;

/// Identifies one subscription, for `EventBus::off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenerId(pub u64);

type Listener = Arc<dyn Fn(&JsonValue) + Send + Sync>;

/// A thread-safe on/off/emit bus. `on` hands back an id; `off` with it
/// stops the delivery; an unknown id is a no-op.
#[derive(Default)]
pub struct EventBus(Mutex<Subscriptions>);

#[derive(Default)]
struct Subscriptions {
    by_event: HashMap<String, Vec<(u64, Listener)>>,
    next_id: u64,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribes to `event`; the id unsubscribes later.
    pub fn on(
        &self,
        event: &str,
        listener: impl Fn(&JsonValue) + Send + Sync + 'static,
    ) -> ListenerId {
        let mut state = self.0.lock().expect("event bus lock");
        state.next_id += 1;
        let id = state.next_id;
        state
            .by_event
            .entry(event.to_owned())
            .or_default()
            .push((id, Arc::new(listener)));
        ListenerId(id)
    }

    /// Removes one listener; one that never subscribed changes nothing.
    pub fn off(&self, id: ListenerId) {
        let mut state = self.0.lock().expect("event bus lock");
        for listeners in state.by_event.values_mut() {
            listeners.retain(|(known, _)| *known != id.0);
        }
    }

    /// Fans `payload` out to the subscribers of `event`, in subscription
    /// order, over the snapshot taken here.
    pub fn emit(&self, event: &str, payload: &JsonValue) {
        let snapshot: Vec<Listener> = {
            let state = self.0.lock().expect("event bus lock");
            state
                .by_event
                .get(event)
                .map(|listeners| listeners.iter().map(|(_, l)| Arc::clone(l)).collect())
                .unwrap_or_default()
        };
        for listener in snapshot {
            listener(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn emit_fans_out_in_subscription_order() {
        let (tx, rx) = mpsc::channel::<&'static str>();
        let bus = EventBus::new();
        let first = tx.clone();
        bus.on("tick", move |_| first.send("first").unwrap());
        bus.on("tick", move |_| tx.send("second").unwrap());
        bus.emit("tick", &JsonValue::Null);
        assert_eq!(rx.recv(), Ok("first"));
        assert_eq!(rx.recv(), Ok("second"));
    }

    #[test]
    fn off_stops_the_delivery_and_an_unknown_id_is_a_no_op() {
        let (tx, rx) = mpsc::channel::<usize>();
        let bus = EventBus::new();
        let first = tx.clone();
        bus.on("tick", move |payload: &JsonValue| {
            first.send(payload.as_u64().unwrap_or(0) as usize).unwrap();
        });
        let id = bus.on("tick", move |payload: &JsonValue| {
            tx.send(payload.as_u64().unwrap_or(0) as usize * 10)
                .unwrap();
        });
        bus.off(id);
        bus.off(ListenerId(9_999));
        bus.emit("tick", &JsonValue::from(7));
        assert_eq!(rx.recv(), Ok(7));
        assert!(rx.try_recv().is_err(), "the removed listener stayed silent");
    }

    #[test]
    fn an_emit_from_a_listener_still_fans_out_to_everyone() {
        let (tx, rx) = mpsc::channel::<u64>();
        let bus = Arc::new(EventBus::new());
        let inner_bus = Arc::clone(&bus);
        let outer_tx = tx.clone();
        bus.on("outer", move |payload: &JsonValue| {
            outer_tx.send(payload.as_u64().unwrap_or(0)).unwrap();
            inner_bus.emit("inner", &JsonValue::Null);
        });
        bus.on("inner", move |_| tx.send(100).unwrap());
        bus.emit("outer", &JsonValue::from(1));
        assert_eq!(rx.recv(), Ok(1));
        assert_eq!(rx.recv(), Ok(100));
    }

    #[test]
    fn events_with_no_listener_change_nothing() {
        let bus = EventBus::new();
        bus.emit("nobody listens", &JsonValue::Null);
        bus.off(ListenerId(1));
    }
}
