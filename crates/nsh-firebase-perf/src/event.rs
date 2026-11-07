//! The `NetworkEvent` shared type and the pieces of one: the URL split,
//! the error rule and the id source. The shape is fixed by PROMPT.md and
//! is the same JSON `nsh-http` and the TypeScript package
//! (`packages/nsh-firebase/src/monitor`) exchange, so a collector line or
//! a host-handler payload parses on either side.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// One measured HTTP exchange. `status`, `ttfb_ms`, the byte counts,
/// `error` and `initiator` are null when the transport cannot know them:
/// a failed exchange has no status or bytes, and the Rust transport
/// (the engine's `fetch` resolves with the whole response) has no
/// headers-ready milestone to time, so its `ttfb_ms` stays null while the
/// TypeScript one (whose `fetch` resolves at the headers) has it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkEvent {
    pub id: String,
    pub method: String,
    pub url: String,
    pub host: String,
    pub path: String,
    pub status: Option<i64>,
    pub started_at_ms: i64,
    pub duration_ms: f64,
    pub ttfb_ms: Option<f64>,
    pub request_bytes: Option<u64>,
    pub response_bytes: Option<u64>,
    pub error: Option<String>,
    pub initiator: Option<String>,
}

impl NetworkEvent {
    /// An exchange that failed: it reports an error, or its status is
    /// outside 2xx. This is the rule the aggregates' `error_rate` uses.
    pub fn is_error(&self) -> bool {
        self.error.is_some()
            || self
                .status
                .is_some_and(|status| !(200..300).contains(&status))
    }
}

/// Splits a URL into its `host` (authority, port kept, userinfo dropped)
/// and `path` (a leading `/`, no query or hash; `/` when the URL has no
/// path). A URL that does not parse gives empty strings, so the event is
/// still recorded.
pub fn split_host_path(url: &str) -> (String, String) {
    let rest = match url.split_once("://") {
        Some((_, after_scheme)) => after_scheme,
        None => return (String::new(), String::new()),
    };
    // Everything to the first `/`, `?` or `#` is the authority.
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, without_userinfo)| without_userinfo)
        .to_owned();
    let tail = &rest[authority_end..];
    let path = match tail.find(['?', '#']) {
        Some(at) if at > 0 => tail[..at].to_owned(),
        Some(_) => "/".to_owned(),
        None if tail.is_empty() => "/".to_owned(),
        None => tail.to_owned(),
    };
    (host, path)
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A locally unique event id (`evt-<unix-ms>-<counter>`); the TypeScript
/// side uses `crypto.randomUUID()`, and both are plain strings.
pub fn next_event_id() -> String {
    let n = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("evt-{}-{n}", nsh_core::time::unix_ms() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> NetworkEvent {
        NetworkEvent {
            id: "evt-1".to_owned(),
            method: "GET".to_owned(),
            url: "http://127.0.0.1:19000/todos.json".to_owned(),
            host: "127.0.0.1:19000".to_owned(),
            path: "/todos.json".to_owned(),
            status: Some(200),
            started_at_ms: 1_767_258_000_000,
            duration_ms: 120.0,
            ttfb_ms: Some(40.5),
            request_bytes: None,
            response_bytes: Some(24),
            error: None,
            initiator: Some("firebase-fetch".to_owned()),
        }
    }

    #[test]
    fn serde_uses_the_exact_prompt_json() {
        let event = sample();
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(
            value,
            json!({
                "id": "evt-1",
                "method": "GET",
                "url": "http://127.0.0.1:19000/todos.json",
                "host": "127.0.0.1:19000",
                "path": "/todos.json",
                "status": 200,
                "startedAtMs": 1_767_258_000_000_i64,
                "durationMs": 120.0,
                "ttfbMs": 40.5,
                "requestBytes": null,
                "responseBytes": 24,
                "error": null,
                "initiator": "firebase-fetch",
            })
        );
    }

    #[test]
    fn a_json_null_field_round_trips_as_none() {
        // A TypeScript-produced event (nulls where the transport could
        // not know) parses to the same struct this crate records.
        let text = r#"{"id":"e2","method":"POST","url":"https://api.test/a","host":"api.test",
            "path":"/a","status":null,"startedAtMs":42,"durationMs":12.5,"ttfbMs":null,
            "requestBytes":9,"responseBytes":null,"error":"boom","initiator":null}"#;
        let event: NetworkEvent = serde_json::from_str(text).unwrap();
        assert_eq!(event.status, None);
        assert_eq!(event.ttfb_ms, None);
        assert_eq!(event.error.as_deref(), Some("boom"));
        assert!(event.is_error());
        assert_eq!(event.duration_ms, 12.5);
    }

    #[test]
    fn an_http_error_status_is_an_error_without_an_error_field() {
        let mut event = sample();
        event.status = Some(503);
        assert!(event.is_error());
        event.status = Some(204);
        assert!(!event.is_error());
        event.status = None;
        event.error = None;
        // No status at all (failed in flight) only errors when the
        // transport said why; otherwise it is an unknown, not a failure.
        assert!(!event.is_error());
        event.error = Some("offline".to_owned());
        assert!(event.is_error());
    }

    #[test]
    fn urls_split_into_host_and_path_like_the_typescript_side() {
        let cases = [
            ("http://api.test/a/b?x=1#frag", ("api.test", "/a/b")),
            ("https://api.test:8080", ("api.test:8080", "/")),
            (
                "http://user:pw@api.test/deep/path",
                ("api.test", "/deep/path"),
            ),
            ("http://api.test?x=1", ("api.test", "/")),
            ("not a url", ("", "")),
        ];
        for (url, want) in cases {
            assert_eq!(
                split_host_path(url),
                (want.0.to_owned(), want.1.to_owned()),
                "{url}"
            );
        }
    }

    #[test]
    fn ids_are_unique() {
        let a = next_event_id();
        let b = next_event_id();
        assert_ne!(a, b);
        assert!(a.starts_with("evt-"));
    }
}
