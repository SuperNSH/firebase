//! Live listeners: one SSE connection per [`crate::DatabaseReference::listen`],
//! its events applied to a per-listener cache, and the honest behavior
//! on network loss.
//!
//! On network loss (the stream ends, fails, or never answered with a
//! success status), the listener reports
//! [`ListenStatus::Reconnecting`], waits [`ReconnectPolicy::delay_ms`]
//! on the caller's [`ReconnectClock`] and opens a new stream. The server
//! answers a fresh stream with a `put` at `/` — the ref's whole,
//! current value — so the cache is rebuilt from the server's truth, not
//! patched on top of what the client guessed while offline. Locally
//! issued writes do not queue offline: a write that cannot reach the
//! server rejects. `cancel` or `auth_revoked`, or a stream answered
//! with a failure status, stops reconnecting: the caller re-authenticates
//! and listens again. This pure tier has no server-side `onDisconnect`:
//! what the SDK registers there (a write or a presence flip when the
//! connection drops) has no documented REST surface, and is not faked.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nsh_core::platform::HttpRequest;
use serde_json::Value as JsonValue;

use crate::sse::{FrameParser, SseEvent, apply_event};

/// The request a connection opens, rebuilt per attempt so a refreshed
/// auth token lands in the reconnect's URL.
type RequestFactory = Arc<dyn Fn() -> HttpRequest + Send + Sync>;

/// What a listener reports: the ref's current value (none before the
/// first `put`) and the connection's state.
pub type Listener = Arc<dyn Fn(Option<&JsonValue>, ListenStatus) + Send + Sync>;

/// What one SSE connection delivers, in order: `head` once, then any
/// number of `data`, then `end` — or `failed` at any point. Every
/// callback runs where the transport runs it: on the engine thread when
/// the transport delivers there, on the transport's own thread when it
/// does not (a callback that touches signals needs the engine thread).
pub trait StreamSink: Send + Sync {
    /// The response status arrived (2xx: the stream is live).
    fn head(&self, status: u16);
    /// Bytes of the body, in order.
    fn data(&self, chunk: Vec<u8>);
    /// The stream ended (a live one ending is a network loss).
    fn end(&self);
    /// The request failed (no response, or the body broke off).
    fn failed(&self, message: String);
}

/// A transport's handle on one open stream: cancelled, the delivery
/// stops.
pub trait StreamTask: Send {
    fn cancel(self: Box<Self>);
}

/// The streaming GET a live listener holds. nsh 0.4.0 hands a Rust app
/// no incremental fetch (`element::fetch` returns the whole body, and a
/// live SSE stream never completes), so an app supplies this itself —
/// from its embedding host's stream, or a stub in tests. The TypeScript
/// package reads `response.body.getReader()` and needs no injection.
pub trait StreamTransport: Send + Sync {
    /// Opens `request` (a GET asking for `text/event-stream`) and feeds
    /// it to `sink` until the stream ends. The returned task is
    /// cancelled once the listener goes off or its stream is lost.
    fn open(&self, request: HttpRequest, sink: Arc<dyn StreamSink>) -> Box<dyn StreamTask>;
}

/// A listener's connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenStatus {
    /// The first stream is opening.
    Connecting,
    /// A stream is open and answered with a success status.
    Live,
    /// A stream was lost; `attempt` names the reconnect in waiting.
    Reconnecting { attempt: u32 },
    /// `cancel` or `auth_revoked` arrived (or the stream answered with a
    /// failure status): the listener stopped for good until listened again.
    Revoked,
}

impl ListenStatus {
    /// The string the TypeScript side reports.
    pub fn label(&self) -> &'static str {
        match self {
            ListenStatus::Connecting => "connecting",
            ListenStatus::Live => "live",
            ListenStatus::Reconnecting { .. } => "reconnecting",
            ListenStatus::Revoked => "revoked",
        }
    }
}

/// Runs the reconnect's wait after a stream loss. The fire runs wherever
/// the clock runs it.
pub trait ReconnectClock: Send + Sync {
    fn after(&self, delay_ms: u64, fire: Box<dyn FnOnce() + Send>);
}

/// The engine's timers: the wait lands on the app's clock, so a manual
/// test clock moves it. `after` must be called on the engine thread —
/// a sink delivering there (the engine's own network does) qualifies.
pub struct EngineReconnect;

impl ReconnectClock for EngineReconnect {
    fn after(&self, delay_ms: u64, fire: Box<dyn FnOnce() + Send>) {
        nsh_core::prelude::set_timeout(Duration::from_millis(delay_ms), fire);
    }
}

/// A thread of its own on the system's time: where no engine runs (a
/// labelled sandbox run driving a listener from its test's threads).
pub struct ThreadReconnect;

impl ReconnectClock for ThreadReconnect {
    fn after(&self, delay_ms: u64, fire: Box<dyn FnOnce() + Send>) {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(delay_ms));
            fire();
        });
    }
}

/// The reconnect waits: one second after the first loss, doubling, at
/// thirty seconds at most (the SDK's order of magnitude, without its
/// jitter, to stay deterministic under a test clock). A stream that
/// opens resets the count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectPolicy {
    pub base_ms: u64,
    pub cap_ms: u64,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            base_ms: 1_000,
            cap_ms: 30_000,
        }
    }
}

impl ReconnectPolicy {
    pub fn new(base_ms: u64, cap_ms: u64) -> Self {
        Self { base_ms, cap_ms }
    }

    /// The wait before reconnect number `attempt` (1, 2, ...).
    pub fn delay_ms(&self, attempt: u32) -> u64 {
        self.base_ms
            .saturating_mul(1_u64 << attempt.saturating_sub(1).min(31))
            .min(self.cap_ms)
    }
}

/// Stops a listener: its open stream is cancelled and no reconnect
/// fires. [`crate::DatabaseReference::listen`] and
/// [`crate::DatabaseReference::on_value`] return one.
#[derive(Clone)]
pub struct ListenHandle {
    state: Arc<ListenState>,
}

impl ListenHandle {
    pub fn off(&self) {
        self.state.off();
    }
}

/// What [`crate::DatabaseReference::on_value`] gives: the value and the
/// status as signals, and the handle to stop with.
pub struct ListenSignals {
    /// The ref's value; none until the first `put` arrives.
    pub value: nsh_core::prelude::Signal<Option<JsonValue>>,
    /// The connection's state.
    pub status: nsh_core::prelude::Signal<ListenStatus>,
    pub handle: ListenHandle,
}

impl ListenSignals {
    pub fn off(&self) {
        self.handle.off();
    }
}

/// What one listener's callbacks share, on whatever threads the
/// transport uses.
struct ListenState {
    cancelled: AtomicBool,
    /// Set on `off`, `revoked` or a failure status: no reconnect, and
    /// further events change nothing.
    stopped: AtomicBool,
    attempts: AtomicU32,
    cache: Mutex<Option<JsonValue>>,
    status: Mutex<ListenStatus>,
    task: Mutex<Option<Box<dyn StreamTask>>>,
}

impl ListenState {
    fn off(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(task) = self.task.lock().expect("listener task").take() {
            task.cancel();
        }
    }

    /// Reports the current cache and status.
    fn report(&self, listener: &Listener) {
        let cache = self.cache.lock().expect("listener cache");
        let status = *self.status.lock().expect("listener status");
        listener(cache.as_ref(), status);
    }
}

/// Starts a listener: connects at once, applies every stream event to a
/// per-listener cache, reports the value and the status through
/// `listener`, and reconnects per `policy` on `clock` after a loss. Off
/// with the returned handle.
pub fn start_listen(
    request: impl Fn() -> HttpRequest + Send + Sync + 'static,
    transport: Arc<dyn StreamTransport>,
    clock: Arc<dyn ReconnectClock>,
    policy: ReconnectPolicy,
    listener: Listener,
) -> ListenHandle {
    let state = Arc::new(ListenState {
        cancelled: AtomicBool::new(false),
        stopped: AtomicBool::new(false),
        attempts: AtomicU32::new(0),
        cache: Mutex::new(None),
        status: Mutex::new(ListenStatus::Connecting),
        task: Mutex::new(None),
    });
    let handle = ListenHandle {
        state: Arc::clone(&state),
    };
    let factory: RequestFactory = Arc::new(request);
    connect(&state, &factory, &transport, &clock, &policy, &listener);
    handle
}

fn connect(
    state: &Arc<ListenState>,
    request: &RequestFactory,
    transport: &Arc<dyn StreamTransport>,
    clock: &Arc<dyn ReconnectClock>,
    policy: &ReconnectPolicy,
    listener: &Listener,
) {
    if state.cancelled.load(Ordering::SeqCst) || state.stopped.load(Ordering::SeqCst) {
        return;
    }
    state.report(listener);
    let sink: Arc<dyn StreamSink> = Arc::new(Sink {
        state: Arc::clone(state),
        request: Arc::clone(request),
        transport: Arc::clone(transport),
        clock: Arc::clone(clock),
        policy: *policy,
        listener: Arc::clone(listener),
        parser: Mutex::new(FrameParser::new()),
    });
    let task = transport.open(request(), sink);
    *state.task.lock().expect("listener task") = Some(task);
    // A head delivered inside `open` (or an off in the window) may have
    // stopped the listener before the store: the task then goes the way
    // off puts it, or it would outlive the listener.
    if (state.cancelled.load(Ordering::SeqCst) || state.stopped.load(Ordering::SeqCst))
        && let Some(task) = state.task.lock().expect("listener task").take()
    {
        task.cancel();
    }
}

/// The stream's events, shared with the transport on whatever thread it
/// delivers.
struct Sink {
    state: Arc<ListenState>,
    request: RequestFactory,
    transport: Arc<dyn StreamTransport>,
    clock: Arc<dyn ReconnectClock>,
    policy: ReconnectPolicy,
    listener: Listener,
    parser: Mutex<FrameParser>,
}

impl Sink {
    /// The stream was lost (ended or failed) while not stopped: mark
    /// reconnecting, then open a new stream after the policy's wait.
    fn lost(&self) {
        if self.state.stopped.load(Ordering::SeqCst) || self.state.cancelled.load(Ordering::SeqCst)
        {
            return;
        }
        let attempt = self.state.attempts.fetch_add(1, Ordering::SeqCst) + 1;
        *self.state.status.lock().expect("listener status") =
            ListenStatus::Reconnecting { attempt };
        self.state.report(&self.listener);
        let wait = self.policy.delay_ms(attempt);
        let (state, request, transport, clock, policy, listener) = (
            Arc::clone(&self.state),
            Arc::clone(&self.request),
            Arc::clone(&self.transport),
            Arc::clone(&self.clock),
            self.policy,
            Arc::clone(&self.listener),
        );
        self.clock.after(
            wait,
            Box::new(move || {
                connect(&state, &request, &transport, &clock, &policy, &listener);
            }),
        );
    }

    /// A self-stop (a failure status, `cancel`, `auth_revoked`): no
    /// reconnect, and the open stream goes the way `off` puts it —
    /// taken and cancelled — or it would outlive the listener.
    fn revoke(&self) {
        self.state.stopped.store(true, Ordering::SeqCst);
        *self.state.status.lock().expect("listener status") = ListenStatus::Revoked;
        if let Some(task) = self.state.task.lock().expect("listener task").take() {
            task.cancel();
        }
    }
}

impl StreamSink for Sink {
    fn head(&self, status: u16) {
        if self.state.stopped.load(Ordering::SeqCst) || self.state.cancelled.load(Ordering::SeqCst)
        {
            return;
        }
        if (200..300).contains(&status) {
            self.state.attempts.store(0, Ordering::SeqCst);
            *self.state.status.lock().expect("listener status") = ListenStatus::Live;
        } else {
            // A failure status (permission denied, auth rejected) is not
            // a network loss: retrying cannot succeed.
            self.revoke();
        }
        self.state.report(&self.listener);
    }

    fn data(&self, chunk: Vec<u8>) {
        if self.state.stopped.load(Ordering::SeqCst) || self.state.cancelled.load(Ordering::SeqCst)
        {
            return;
        }
        let events = self.parser.lock().expect("frame parser").feed(&chunk);
        if !events.is_empty() {
            let mut revoked = false;
            {
                let mut cache = self.state.cache.lock().expect("listener cache");
                for event in &events {
                    match event {
                        SseEvent::Put { .. } | SseEvent::Patch { .. } => {
                            apply_event(&mut cache, event)
                        }
                        SseEvent::KeepAlive => {}
                        SseEvent::Cancel | SseEvent::AuthRevoked => {
                            revoked = true;
                        }
                    }
                }
            }
            if revoked {
                self.revoke();
            }
            self.state.report(&self.listener);
        }
    }

    fn end(&self) {
        self.lost();
    }

    fn failed(&self, _message: String) {
        // The message names the loss for a debug log; the listener acts
        // as on an end: reconnect.
        self.lost();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_wait_doubles_to_the_cap() {
        let policy = ReconnectPolicy::default();
        let waits: Vec<u64> = (1..=6).map(|attempt| policy.delay_ms(attempt)).collect();
        assert_eq!(waits, [1_000, 2_000, 4_000, 8_000, 16_000, 30_000]);
        assert_eq!(policy.delay_ms(20), 30_000);
        let custom = ReconnectPolicy::new(40, 100);
        assert_eq!(custom.delay_ms(1), 40);
        assert_eq!(custom.delay_ms(2), 80);
        assert_eq!(custom.delay_ms(3), 100);
    }

    #[test]
    fn statuses_label_as_the_typescript_side_reports_them() {
        assert_eq!(ListenStatus::Connecting.label(), "connecting");
        assert_eq!(ListenStatus::Live.label(), "live");
        assert_eq!(
            ListenStatus::Reconnecting { attempt: 2 }.label(),
            "reconnecting"
        );
        assert_eq!(ListenStatus::Revoked.label(), "revoked");
    }

    /// Holds the reconnect fires a test releases by hand, so the waits
    /// pass with no clock and no real time.
    struct Hold(Mutex<Vec<Box<dyn FnOnce() + Send>>>);

    impl Hold {
        fn new() -> Arc<Self> {
            Arc::new(Self(Mutex::new(Vec::new())))
        }

        fn release_all(&self) {
            for fire in self.0.lock().unwrap().drain(..) {
                fire();
            }
        }
    }

    impl ReconnectClock for Hold {
        fn after(&self, _delay_ms: u64, fire: Box<dyn FnOnce() + Send>) {
            self.0.lock().unwrap().push(fire);
        }
    }

    /// A transport whose streams play scripted frames, in the order the
    /// connections open. A script's frames go out as one chunk and the
    /// stream ends; an empty script is a stream that answers and stays
    /// open.
    struct Scripted {
        scripts: Mutex<Vec<Vec<u8>>>,
        opens: Mutex<Vec<String>>,
    }

    impl Scripted {
        fn with_scripts(scripts: Vec<Vec<u8>>) -> Arc<Self> {
            Arc::new(Self {
                scripts: Mutex::new(scripts),
                opens: Mutex::new(Vec::new()),
            })
        }

        fn opens(&self) -> Vec<String> {
            self.opens.lock().unwrap().clone()
        }
    }

    impl StreamTransport for Scripted {
        fn open(&self, request: HttpRequest, sink: Arc<dyn StreamSink>) -> Box<dyn StreamTask> {
            self.opens.lock().unwrap().push(request.url);
            let script = {
                let mut scripts = self.scripts.lock().unwrap();
                if scripts.is_empty() {
                    Vec::new()
                } else {
                    scripts.remove(0)
                }
            };
            sink.head(200);
            if !script.is_empty() {
                sink.data(script);
                sink.end();
            }
            Box::new(NoTask)
        }
    }

    struct NoTask;

    impl StreamTask for NoTask {
        fn cancel(self: Box<Self>) {}
    }

    /// One entry of what a listener saw.
    type SeenEntry = (Option<JsonValue>, ListenStatus);

    /// Collects what a listener saw, off the engine (no signals here).
    #[derive(Default, Clone)]
    struct Seen {
        entries: Arc<Mutex<Vec<SeenEntry>>>,
    }

    impl Seen {
        fn listener(self) -> Listener {
            Arc::new(move |value, status| {
                self.entries.lock().unwrap().push((value.cloned(), status));
            })
        }

        fn entries(&self) -> Vec<SeenEntry> {
            self.entries.lock().unwrap().clone()
        }
    }

    fn frames(frames: &[&str]) -> Vec<u8> {
        frames.concat().into_bytes()
    }

    #[test]
    fn a_listener_receives_then_reconnects_and_resyncs_from_the_put() {
        let transport = Scripted::with_scripts(vec![
            frames(&["event: put\ndata: {\"path\":\"/\",\"data\":{\"a\":1}}\n\n"]),
            frames(&[]),
        ]);
        let clock = Hold::new();
        let seen = Seen::default();
        let handle = start_listen(
            || HttpRequest::get("http://127.0.0.1:19000/live.json"),
            transport.clone(),
            clock.clone(),
            ReconnectPolicy::default(),
            seen.clone().listener(),
        );
        // The first stream: connecting, live with the put, then the end
        // marks reconnecting attempt 1. No second connection yet.
        let entries = seen.entries();
        assert_eq!(
            entries,
            vec![
                (None, ListenStatus::Connecting),
                (None, ListenStatus::Live),
                (Some(json!({"a": 1})), ListenStatus::Live),
                (
                    Some(json!({"a": 1})),
                    ListenStatus::Reconnecting { attempt: 1 }
                ),
            ],
            "{entries:?}"
        );
        assert_eq!(transport.opens().len(), 1);
        // The wait passes: the new stream opens, and its put (here: the
        // empty script's plain head) makes it live again.
        clock.release_all();
        let entries = seen.entries();
        let last = entries.last().expect("reported after the reconnect");
        assert_eq!(*last, (Some(json!({"a": 1})), ListenStatus::Live));
        assert_eq!(transport.opens().len(), 2);
        handle.off();
    }

    #[test]
    fn off_before_the_wait_stops_the_reconnect() {
        let transport = Scripted::with_scripts(vec![frames(&[
            "event: put\ndata: {\"path\":\"/\",\"data\":true}\n\n",
        ])]);
        let clock = Hold::new();
        let seen = Seen::default();
        let handle = start_listen(
            || HttpRequest::get("http://127.0.0.1:19000/live.json"),
            transport.clone(),
            clock.clone(),
            ReconnectPolicy::default(),
            seen.clone().listener(),
        );
        let before = seen.entries().len();
        handle.off();
        clock.release_all();
        assert_eq!(transport.opens().len(), 1);
        assert_eq!(seen.entries().len(), before);
    }

    #[test]
    fn a_cancel_stops_the_listener_without_reconnecting() {
        let transport = Scripted::with_scripts(vec![frames(&["event: cancel\ndata: null\n\n"])]);
        let clock = Hold::new();
        let seen = Seen::default();
        let handle = start_listen(
            || HttpRequest::get("http://127.0.0.1:19000/live.json"),
            transport.clone(),
            clock.clone(),
            ReconnectPolicy::default(),
            seen.clone().listener(),
        );
        let entries = seen.entries();
        assert_eq!(
            entries.last().map(|(_, status)| *status),
            Some(ListenStatus::Revoked),
            "{entries:?}"
        );
        clock.release_all();
        assert_eq!(transport.opens().len(), 1, "no reconnect after a cancel");
        handle.off();
    }

    #[test]
    fn a_failure_status_revokes_instead_of_reconnecting() {
        struct Forbidden;
        impl StreamTransport for Forbidden {
            fn open(
                &self,
                _request: HttpRequest,
                sink: Arc<dyn StreamSink>,
            ) -> Box<dyn StreamTask> {
                sink.head(403);
                sink.end();
                Box::new(NoTask)
            }
        }
        let clock = Hold::new();
        let seen = Seen::default();
        let handle = start_listen(
            || HttpRequest::get("http://127.0.0.1:19000/live.json"),
            Arc::new(Forbidden),
            clock.clone(),
            ReconnectPolicy::default(),
            seen.clone().listener(),
        );
        let entries = seen.entries();
        assert_eq!(
            entries.last().map(|(_, status)| *status),
            Some(ListenStatus::Revoked),
            "{entries:?}"
        );
        clock.release_all();
        handle.off();
    }

    /// A stream the listener has revoked (a failure head, a cancel)
    /// must be cancelled even when the caller never calls off: the
    /// transport's task reports its own cancellation here.
    #[test]
    fn a_revoked_stream_is_cancelled_without_the_caller_going_off() {
        struct LateForbidden {
            sink: Mutex<Option<Arc<dyn StreamSink>>>,
            cancelled: Arc<std::sync::atomic::AtomicBool>,
        }
        struct LateTask(Arc<std::sync::atomic::AtomicBool>);
        impl StreamTask for LateTask {
            fn cancel(self: Box<Self>) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        impl StreamTransport for LateForbidden {
            fn open(
                &self,
                _request: HttpRequest,
                sink: Arc<dyn StreamSink>,
            ) -> Box<dyn StreamTask> {
                // Delivered after `open` returns, as a real transport's
                // fetch thread does.
                *self.sink.lock().unwrap() = Some(sink);
                Box::new(LateTask(Arc::clone(&self.cancelled)))
            }
        }
        let transport = Arc::new(LateForbidden {
            sink: Mutex::new(None),
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        let clock = Hold::new();
        let seen = Seen::default();
        let handle = start_listen(
            || HttpRequest::get("http://127.0.0.1:19000/live.json"),
            transport.clone(),
            clock.clone(),
            ReconnectPolicy::default(),
            seen.clone().listener(),
        );
        let sink = transport
            .sink
            .lock()
            .unwrap()
            .clone()
            .expect("open kept its sink");
        sink.head(403);
        assert!(
            transport.cancelled.load(Ordering::SeqCst),
            "the revoked stream was cancelled without the caller going off"
        );
        let entries = seen.entries();
        assert_eq!(
            entries.last().map(|(_, status)| *status),
            Some(ListenStatus::Revoked)
        );
        handle.off();
    }

    /// A failure head delivered synchronously inside `open` stops the
    /// listener before the task is stored; the store must then notice
    /// and cancel, leaving no live stream behind.
    #[test]
    fn a_stream_revoked_inside_open_is_cancelled_once_stored() {
        struct SyncForbidden {
            cancelled: Arc<std::sync::atomic::AtomicBool>,
        }
        struct SyncTask(Arc<std::sync::atomic::AtomicBool>);
        impl StreamTask for SyncTask {
            fn cancel(self: Box<Self>) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        impl StreamTransport for SyncForbidden {
            fn open(
                &self,
                _request: HttpRequest,
                sink: Arc<dyn StreamSink>,
            ) -> Box<dyn StreamTask> {
                sink.head(403);
                sink.end();
                Box::new(SyncTask(Arc::clone(&self.cancelled)))
            }
        }
        let transport = Arc::new(SyncForbidden {
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        let clock = Hold::new();
        let seen = Seen::default();
        let handle = start_listen(
            || HttpRequest::get("http://127.0.0.1:19000/live.json"),
            transport.clone(),
            clock.clone(),
            ReconnectPolicy::default(),
            seen.clone().listener(),
        );
        assert!(
            transport.cancelled.load(Ordering::SeqCst),
            "the stream revoked inside open was cancelled once stored"
        );
        let entries = seen.entries();
        assert_eq!(
            entries.last().map(|(_, status)| *status),
            Some(ListenStatus::Revoked),
            "{entries:?}"
        );
        handle.off();
    }

    #[test]
    fn a_patch_updates_the_reported_value_in_place() {
        let transport = Scripted::with_scripts(vec![frames(&[
            "event: put\ndata: {\"path\":\"/\",\"data\":{\"c\":{\"bar\":2}}}\n\n",
            "event: patch\ndata: {\"path\":\"/c\",\"data\":{\"foo\":3}}\n\n",
        ])]);
        let clock = Hold::new();
        let seen = Seen::default();
        let handle = start_listen(
            || HttpRequest::get("http://127.0.0.1:19000/live.json"),
            transport,
            clock,
            ReconnectPolicy::default(),
            seen.clone().listener(),
        );
        let entries = seen.entries();
        assert_eq!(
            entries.last().map(|(value, _)| value.clone()),
            Some(Some(json!({"c": {"bar": 2, "foo": 3}}))),
            "{entries:?}"
        );
        handle.off();
    }
}
