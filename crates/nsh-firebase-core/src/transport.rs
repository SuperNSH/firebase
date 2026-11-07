//! The pure tier's transports: HTTP over the engine's `fetch`, and the
//! WebSocket handshake the products build for the platform's transport.
//! `packages/nsh-firebase/src/core/http.ts` mirrors the fetch side.

use nsh_core::{
    element::HttpResponse, platform::HttpRequest, prelude::fetch, websocket::WebSocketRequest,
};

use crate::error::FirebaseError;

/// Joins a base URL and a path with exactly one slash.
pub fn join_url(base: &str, path: &str) -> String {
    let trim_base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{trim_base}/{path}")
}

/// Sends `request` through the engine's network stack, under the app's
/// `capabilities.network`. Call it on the engine thread (a component's
/// task, `spawn`): that is where the engine's `fetch` lives. A refusal
/// surfaces as [`FirebaseError`] with the stable access-denied code.
pub async fn send(request: HttpRequest) -> Result<HttpResponse, FirebaseError> {
    fetch(request).await.map_err(|message| {
        // The engine's failure text is all a Rust caller gets: it names
        // the grant on a refusal ("... is not granted: add ...").
        FirebaseError::from_transport_message(&message)
    })
}

/// The handshake a product hands the platform's WebSocket transport
/// (`nsh-core` has no app-facing connect on 0.4.0; the engine's transport
/// takes this request). `url` is absolute, `ws://` or `wss://`.
pub fn socket_request(
    url: &str,
    protocols: Vec<String>,
    headers: Vec<(String, String)>,
) -> WebSocketRequest {
    WebSocketRequest {
        url: url.to_owned(),
        protocols,
        headers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_url_uses_one_slash_whatever_the_sides_have() {
        assert_eq!(
            join_url("http://127.0.0.1:19000", "/todos.json"),
            "http://127.0.0.1:19000/todos.json"
        );
        assert_eq!(
            join_url("http://127.0.0.1:19000/", "todos.json"),
            "http://127.0.0.1:19000/todos.json"
        );
        assert_eq!(
            join_url("http://127.0.0.1:19000", "a/b.json"),
            "http://127.0.0.1:19000/a/b.json"
        );
    }

    #[test]
    fn socket_requests_normalize_the_url() {
        let request = socket_request("wss://example.test/ws", vec!["v1".to_owned()], vec![]);
        assert_eq!(request.url, "wss://example.test/ws");
        assert_eq!(request.protocols, vec!["v1".to_owned()]);
    }
}
