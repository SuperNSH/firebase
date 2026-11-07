//! Firebase Analytics for Native Shell Rust apps.
//!
//! Events, user properties, screen views, consent, a clock-driven
//! batching queue with retrying sends, and pluggable transports: the
//! in-memory recorder, the GA4 Measurement Protocol (limits documented
//! on the transport), and the sandbox collector. The native tier routes
//! the same calls to the `firebase.analytics.*` handlers an embedding
//! host registers ([`NativeAnalytics`]). The TypeScript package
//! `packages/nsh-firebase/src/analytics` mirrors this behavior name for
//! name.

pub mod client;
pub mod clock;
pub mod model;
pub mod queue;
pub mod storage;
pub mod transport;
pub mod validate;

pub use crate::{
    client::{Analytics, NativeAnalytics},
    clock::{Clock, ManualClock, SystemClock},
    model::{AnalyticsConfig, AnalyticsEvent, Batch, Consent, ConsentStatus},
    storage::{KeyValueStorage, MemoryStorage, ensure_client_id},
    transport::{
        CollectorTransport, InMemoryTransport, MeasurementProtocolTransport, Transport,
        collector_request, mp_collect_request, mp_collect_url,
    },
    validate::{validate_event_name, validate_params, validate_user_property},
};
