//! The sorted, filterable view the monitoring screen shows: the kept
//! events, slowest first, under the filters the screen applies. The
//! TypeScript side (`packages/nsh-firebase/src/monitor`) matches this
//! exactly, ties included, so the demo screen shows the same order on
//! both sides.

use crate::event::NetworkEvent;

/// What the screen filters by. `host` is an exact match, `path` a
/// substring, `min_duration_ms` keeps `durationMs` at or above it, and
/// `status` matches the recorded status exactly (`None` keeps events with
/// no status, e.g. failed ones, when the filter is unset).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MonitorFilter {
    pub host: Option<String>,
    pub path: Option<String>,
    pub min_duration_ms: Option<f64>,
    pub status: Option<i64>,
}

impl MonitorFilter {
    /// Keeps an event only when every set filter says so.
    pub fn matches(&self, event: &NetworkEvent) -> bool {
        if let Some(host) = &self.host
            && event.host != *host
        {
            return false;
        }
        if let Some(path) = &self.path
            && !event.path.contains(path.as_str())
        {
            return false;
        }
        if let Some(min) = self.min_duration_ms
            && event.duration_ms < min
        {
            return false;
        }
        if let Some(status) = self.status
            && event.status != Some(status)
        {
            return false;
        }
        true
    }
}

/// The rows the screen shows: the events `filter` keeps, sorted by
/// `durationMs` descending (slowest on top, the criterion-7 order), ties
/// broken by `startedAtMs` ascending, then by `id` ascending, so the same
/// events always give the same order.
pub fn rows(events: &[NetworkEvent], filter: &MonitorFilter) -> Vec<NetworkEvent> {
    let mut kept: Vec<NetworkEvent> = events
        .iter()
        .filter(|event| filter.matches(event))
        .cloned()
        .collect();
    kept.sort_by(|a, b| {
        b.duration_ms
            .total_cmp(&a.duration_ms)
            .then_with(|| a.started_at_ms.cmp(&b.started_at_ms))
            .then_with(|| a.id.cmp(&b.id))
    });
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        id: &str,
        host: &str,
        path: &str,
        duration_ms: f64,
        status: Option<i64>,
    ) -> NetworkEvent {
        NetworkEvent {
            id: id.to_owned(),
            method: "GET".to_owned(),
            url: format!("http://{host}{path}"),
            host: host.to_owned(),
            path: path.to_owned(),
            status,
            started_at_ms: 1,
            duration_ms,
            ttfb_ms: None,
            request_bytes: None,
            response_bytes: None,
            error: None,
            initiator: None,
        }
    }

    fn sample() -> Vec<NetworkEvent> {
        vec![
            event("a", "api.test", "/list", 10.0, Some(200)),
            event("b", "api.test", "/list", 300.0, Some(200)),
            event("c", "cdn.test", "/img", 120.0, Some(200)),
            event("d", "api.test", "/slow-route", 900.0, Some(503)),
            event("e", "api.test", "/list", 200.0, None),
        ]
    }

    #[test]
    fn rows_sort_by_duration_descending_with_stable_ties() {
        let view = rows(&sample(), &MonitorFilter::default());
        let ids: Vec<&str> = view.iter().map(|event| event.id.as_str()).collect();
        assert_eq!(ids, ["d", "b", "e", "c", "a"]);
    }

    #[test]
    fn filters_narrow_by_host_path_min_duration_and_status() {
        let events = sample();
        let host_only = rows(
            &events,
            &MonitorFilter {
                host: Some("api.test".to_owned()),
                ..MonitorFilter::default()
            },
        );
        assert_eq!(host_only.len(), 4);
        assert!(host_only.iter().all(|event| event.host == "api.test"));

        let path_part = rows(
            &events,
            &MonitorFilter {
                path: Some("list".to_owned()),
                ..MonitorFilter::default()
            },
        );
        let ids: Vec<&str> = path_part.iter().map(|event| event.id.as_str()).collect();
        assert_eq!(ids, ["b", "e", "a"]);

        let slow = rows(
            &events,
            &MonitorFilter {
                min_duration_ms: Some(100.0),
                ..MonitorFilter::default()
            },
        );
        let ids: Vec<&str> = slow.iter().map(|event| event.id.as_str()).collect();
        assert_eq!(ids, ["d", "b", "e", "c"]);

        // 100.0 keeps the 100.0 event out only when it is strictly below.
        let exactly = rows(
            &events,
            &MonitorFilter {
                min_duration_ms: Some(300.0),
                ..MonitorFilter::default()
            },
        );
        assert_eq!(
            exactly
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            ["d", "b"]
        );
    }

    #[test]
    fn a_status_filter_matches_null_only_when_unset() {
        let events = sample();
        let failures = rows(
            &events,
            &MonitorFilter {
                status: Some(503),
                ..MonitorFilter::default()
            },
        );
        assert_eq!(
            failures
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            ["d"]
        );
        // The unset filter keeps the status-less event; an impossible
        // status drops it.
        let none_status = rows(
            &events,
            &MonitorFilter {
                status: Some(418),
                ..MonitorFilter::default()
            },
        );
        assert!(none_status.is_empty());
    }

    #[test]
    fn combined_filters_intersect() {
        let view = rows(
            &sample(),
            &MonitorFilter {
                host: Some("api.test".to_owned()),
                path: Some("list".to_owned()),
                min_duration_ms: Some(150.0),
                status: None,
            },
        );
        assert_eq!(
            view.iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            // b (300) then e (200): e's null status is kept by the unset
            // status filter.
            ["b", "e"]
        );
    }
}
