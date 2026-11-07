//! Per-host and per-path aggregates over [`NetworkEvent`]s: count, error
//! rate and the p50/p95 of the durations. The math is fixed so the Rust
//! and TypeScript sides give the same numbers for the same events:
//! nearest-rank percentiles over the durations sorted ascending, and an
//! exchange errors when [`NetworkEvent::is_error`] holds.

use std::collections::BTreeMap;

use crate::event::NetworkEvent;

/// The aggregate of one host or path.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Aggregate {
    pub count: u64,
    /// Failures over count (`0.0`..`1.0`); `0.0` for an empty group.
    pub error_rate: f64,
    /// The nearest-rank 50th percentile of the durations; `None` for an
    /// empty group.
    pub p50_ms: Option<f64>,
    /// The nearest-rank 95th percentile of the durations; `None` for an
    /// empty group.
    pub p95_ms: Option<f64>,
}

/// The nearest-rank percentile: the value at `ceil(q * n)` of the
/// durations sorted ascending. Ties keep the smaller index (the value at
/// that rank is the same anyway).
pub fn percentile(sorted: &[f64], q: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (q * sorted.len() as f64).ceil();
    let index = rank.clamp(1.0, sorted.len() as f64) as usize - 1;
    sorted.get(index).copied()
}

/// The aggregate of one group of events.
pub fn aggregate(events: &[NetworkEvent]) -> Aggregate {
    let borrowed: Vec<&NetworkEvent> = events.iter().collect();
    aggregate_of(&borrowed)
}

fn aggregate_of(events: &[&NetworkEvent]) -> Aggregate {
    let count = events.len() as u64;
    if count == 0 {
        return Aggregate::default();
    }
    let mut durations: Vec<f64> = events.iter().map(|event| event.duration_ms).collect();
    durations.sort_by(|a, b| a.total_cmp(b));
    let errors = events.iter().filter(|event| event.is_error()).count();
    Aggregate {
        count,
        error_rate: errors as f64 / count as f64,
        p50_ms: percentile(&durations, 0.50),
        p95_ms: percentile(&durations, 0.95),
    }
}

/// One aggregate per host, keyed by the event's `host` (as the URL names
/// it, port kept); hosts in ascending order.
pub fn aggregates_by_host(events: &[NetworkEvent]) -> BTreeMap<String, Aggregate> {
    let mut by_host: BTreeMap<&str, Vec<&NetworkEvent>> = BTreeMap::new();
    for event in events {
        by_host.entry(event.host.as_str()).or_default().push(event);
    }
    by_host
        .into_iter()
        .map(|(host, group)| (host.to_owned(), aggregate_of(&group)))
        .collect()
}

/// One aggregate per path, keyed by the event's `path` (no query or
/// hash); paths in ascending order.
pub fn aggregates_by_path(events: &[NetworkEvent]) -> BTreeMap<String, Aggregate> {
    let mut by_path: BTreeMap<&str, Vec<&NetworkEvent>> = BTreeMap::new();
    for event in events {
        by_path.entry(event.path.as_str()).or_default().push(event);
    }
    by_path
        .into_iter()
        .map(|(path, group)| (path.to_owned(), aggregate_of(&group)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(host: &str, path: &str, duration_ms: f64, error: bool) -> NetworkEvent {
        NetworkEvent {
            id: format!("{host}{path}{duration_ms}"),
            method: "GET".to_owned(),
            url: format!("http://{host}{path}"),
            host: host.to_owned(),
            path: path.to_owned(),
            status: if error { Some(500) } else { Some(200) },
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
    fn percentiles_are_nearest_rank_over_sorted_durations() {
        let mut durations: Vec<f64> = [10.0, 50.0, 20.0, 30.0, 40.0].into();
        durations.sort_by(|a, b| a.total_cmp(b));
        assert_eq!(percentile(&durations, 0.50), Some(30.0));
        assert_eq!(percentile(&durations, 0.95), Some(50.0));
        // A single sample is every percentile of itself.
        assert_eq!(percentile(&[7.5], 0.95), Some(7.5));
        assert_eq!(percentile(&[], 0.50), None);
        // Twenty samples: the 95th is the 19th of 20.
        let twenty: Vec<f64> = (1..=20).map(|n| n as f64).collect();
        assert_eq!(percentile(&twenty, 0.95), Some(19.0));
    }

    #[test]
    fn aggregates_count_errors_and_percentiles_per_host() {
        let events = [
            event("api.test", "/fast", 10.0, false),
            event("api.test", "/slow", 100.0, false),
            event("api.test", "/broken", 30.0, true),
            event("cdn.test", "/fast", 20.0, false),
        ];
        let by_host = aggregates_by_host(&events);
        assert_eq!(
            by_host.keys().cloned().collect::<Vec<_>>(),
            ["api.test", "cdn.test"]
        );
        let api = &by_host["api.test"];
        assert_eq!(api.count, 3);
        assert!((api.error_rate - 1.0 / 3.0).abs() < 1e-12);
        assert_eq!(api.p50_ms, Some(30.0));
        assert_eq!(api.p95_ms, Some(100.0));
        assert_eq!(by_host["cdn.test"].error_rate, 0.0);
    }

    #[test]
    fn aggregates_by_path_group_the_same_events_by_their_path() {
        let events = [
            event("api.test", "/fast", 10.0, false),
            event("cdn.test", "/fast", 20.0, false),
            event("api.test", "/slow", 100.0, false),
        ];
        let by_path = aggregates_by_path(&events);
        assert_eq!(
            by_path.keys().cloned().collect::<Vec<_>>(),
            ["/fast", "/slow"]
        );
        // Both hosts' /fast calls meet in one group.
        assert_eq!(by_path["/fast"].count, 2);
        assert_eq!(by_path["/fast"].p50_ms, Some(10.0));
    }

    #[test]
    fn an_empty_group_is_all_zeros() {
        assert_eq!(
            aggregate(&[]),
            Aggregate {
                count: 0,
                error_rate: 0.0,
                p50_ms: None,
                p95_ms: None
            }
        );
    }
}
