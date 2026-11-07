//! The monitor: a ring buffer of [`NetworkEvent`]s with a subscribe API.
//! The TypeScript package (`packages/nsh-firebase/src/monitor`) keeps the
//! same behavior; the aggregates and the screen's view model live in
//! [`crate::aggregate`] and [`crate::view`].

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use crate::event::NetworkEvent;

/// The ring buffer keeps 500 events unless `Monitor::with_capacity` says
/// otherwise.
pub const DEFAULT_CAPACITY: usize = 500;

/// Identifies one subscription, for [`Monitor::unsubscribe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorId(pub u64);

type Listener = Arc<dyn Fn(&NetworkEvent) + Send + Sync>;

/// A ring buffer of captured network events. Cheap to clone around tasks
/// as an `Arc<Monitor>`; `record` runs from any task or thread.
#[derive(Default)]
pub struct Monitor {
    events: Mutex<VecDeque<NetworkEvent>>,
    listeners: Mutex<Subscriptions>,
    capacity: usize,
}

#[derive(Default)]
struct Subscriptions {
    by_id: Vec<(u64, Listener)>,
    next_id: u64,
}

impl Monitor {
    /// A monitor keeping the last [`DEFAULT_CAPACITY`] events.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// A monitor keeping the last `capacity` events; each `record` beyond
    /// it drops the oldest.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            events: Mutex::new(VecDeque::with_capacity(capacity)),
            listeners: Mutex::new(Subscriptions::default()),
            capacity: capacity.max(1),
        }
    }

    /// Records `event`: it joins the ring (the oldest falls out past the
    /// capacity), then the listeners hear it, in subscription order, over
    /// the snapshot taken here.
    pub fn record(&self, event: NetworkEvent) {
        // The listeners run outside the ring's lock: one that reads the
        // monitor (`len`, `events`) would otherwise deadlock on it.
        let last = {
            let mut events = self.events.lock().expect("monitor lock");
            while events.len() >= self.capacity {
                events.pop_front();
            }
            events.push_back(event);
            events.back().expect("an event was just pushed").clone()
        };
        let snapshot: Vec<Listener> = {
            let listeners = self.listeners.lock().expect("monitor lock");
            listeners.by_id.iter().map(|(_, l)| Arc::clone(l)).collect()
        };
        for listener in snapshot {
            listener(&last);
        }
    }

    /// The kept events, oldest first.
    pub fn events(&self) -> Vec<NetworkEvent> {
        self.events
            .lock()
            .expect("monitor lock")
            .iter()
            .cloned()
            .collect()
    }

    /// How many events are kept now.
    pub fn len(&self) -> usize {
        self.events.lock().expect("monitor lock").len()
    }

    /// Whether the ring is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Subscribes to every recorded event; the id unsubscribes later.
    pub fn subscribe(&self, listener: impl Fn(&NetworkEvent) + Send + Sync + 'static) -> MonitorId {
        let mut listeners = self.listeners.lock().expect("monitor lock");
        listeners.next_id += 1;
        let id = listeners.next_id;
        listeners.by_id.push((id, Arc::new(listener)));
        MonitorId(id)
    }

    /// Removes one listener; one that never subscribed changes nothing.
    pub fn unsubscribe(&self, id: MonitorId) {
        self.listeners
            .lock()
            .expect("monitor lock")
            .by_id
            .retain(|(known, _)| *known != id.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn event(id: &str, duration_ms: f64) -> NetworkEvent {
        NetworkEvent {
            id: id.to_owned(),
            method: "GET".to_owned(),
            url: "http://api.test/a".to_owned(),
            host: "api.test".to_owned(),
            path: "/a".to_owned(),
            status: Some(200),
            started_at_ms: 1,
            duration_ms,
            ttfb_ms: None,
            request_bytes: None,
            response_bytes: None,
            error: None,
            initiator: None,
        }
    }

    #[test]
    fn the_ring_evicts_the_oldest_past_the_capacity() {
        let monitor = Monitor::with_capacity(3);
        for n in 1..=5_i32 {
            monitor.record(event(&format!("e{n}"), n as f64));
        }
        let kept: Vec<String> = monitor
            .events()
            .iter()
            .map(|event| event.id.clone())
            .collect();
        assert_eq!(kept, ["e3", "e4", "e5"]);
    }

    #[test]
    fn listeners_hear_in_subscription_order_and_off_stops_one() {
        let monitor = Monitor::new();
        let (tx, rx) = mpsc::channel::<String>();
        let first = tx.clone();
        monitor.subscribe(move |event| first.send(format!("first:{}", event.id)).unwrap());
        let second = tx.clone();
        let id =
            monitor.subscribe(move |event| second.send(format!("second:{}", event.id)).unwrap());
        monitor.record(event("e1", 1.0));
        monitor.unsubscribe(id);
        monitor.unsubscribe(MonitorId(999));
        monitor.record(event("e2", 2.0));
        assert_eq!(rx.recv(), Ok("first:e1".to_owned()));
        assert_eq!(rx.recv(), Ok("second:e1".to_owned()));
        assert_eq!(rx.recv(), Ok("first:e2".to_owned()));
        assert!(rx.try_recv().is_err(), "the removed listener stayed silent");
    }

    #[test]
    fn a_listener_sees_the_event_already_in_the_ring() {
        let monitor = Arc::new(Monitor::new());
        let reader = Arc::clone(&monitor);
        let (tx, rx) = mpsc::channel::<usize>();
        monitor.subscribe(move |_| tx.send(reader.len()).unwrap());
        monitor.record(event("e1", 1.0));
        assert_eq!(rx.recv(), Ok(1));
    }
}
