//! Network monitoring and Performance Monitoring for Native Shell Rust
//! apps, the pure tier with a native-tier hand-off:
//!
//! - [`NetworkEvent`], the exchange shape PROMPT.md fixes for this repo
//!   (the same JSON `nsh-http` and the TypeScript package use),
//! - [`Monitor`], the ring buffer with a subscribe API,
//! - [`aggregate`] and [`view`], the per-host/per-path aggregates and the
//!   screen's sorted, filterable rows,
//! - [`instrumented_send`], the fetch wrapper over the core transport,
//! - [`Perfmon`], traces and HTTP metrics with a pluggable transport
//!   that route through the `firebase.perf.*` native handlers when they
//!   exist.
//!
//! The TypeScript package `packages/nsh-firebase/src/{monitor,perf}`
//! mirrors this behavior name for name. Performance has no public
//! ingestion REST, so the pure tier delivers records to a pluggable
//! transport (`tools/collector` in the sandbox) and cannot reach the
//! Firebase console; the native tier adds the SDK's real traces and its
//! automatic HTTP capture (see docs/status/perfmon.md for what that
//! adds and what stays invisible).

pub mod aggregate;
pub mod event;
pub mod instrument;
pub mod monitor;
pub mod perf;
pub mod view;

pub use crate::{
    aggregate::{Aggregate, aggregate, aggregates_by_host, aggregates_by_path, percentile},
    event::{NetworkEvent, next_event_id, split_host_path},
    instrument::{INITIATOR, instrumented_send},
    monitor::{DEFAULT_CAPACITY, Monitor, MonitorId},
    perf::{
        BatchingTransport, HttpLineTransport, PROBE_TRACE_NAME, PerfRecord, PerfTransport, Perfmon,
        Trace, TraceRecord,
    },
    view::{MonitorFilter, rows},
};
