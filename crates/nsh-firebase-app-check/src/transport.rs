//! The exchange transports: the trait the client sends through, the one
//! over the engine's `fetch`, and the recorder the tests (and an offline
//! demo display) use.

use std::{future::Future, pin::Pin, sync::Mutex};

use nsh_core::{
    element::HttpResponse,
    platform::{HttpHead, HttpRequest},
};
use nsh_firebase_core::{FirebaseError, send};

/// One exchange attempt. The future is deliberately not `Send`: the
/// engine's `fetch` is single-threaded, so [`FetchExchange`] (like
/// [`nsh_firebase_core::send`]) must be awaited on the engine thread,
/// never spawned across threads or blocked on.
pub trait Exchange: Send + Sync {
    fn exchange<'a>(
        &'a self,
        request: HttpRequest,
    ) -> Pin<Box<dyn Future<Output = Result<HttpResponse, FirebaseError>> + 'a>>;
}

/// The real transport, over the engine's network stack under the app's
/// `capabilities.network`. A refusal (or any failed request) surfaces as
/// the core's stable error.
pub struct FetchExchange;

impl Exchange for FetchExchange {
    fn exchange<'a>(
        &'a self,
        request: HttpRequest,
    ) -> Pin<Box<dyn Future<Output = Result<HttpResponse, FirebaseError>> + 'a>> {
        Box::pin(async move { send(request).await })
    }
}

/// What the recorder answers each exchange with.
#[derive(Default)]
enum Script {
    #[default]
    Idle,
    Reply(HttpResponse),
    Fail(FirebaseError),
}

/// Records every request and answers with one scripted reply, standing
/// until replaced. The unit tests' transport — and the shape a demo can
/// show without touching the network.
#[derive(Default)]
pub struct RecordingExchange {
    requests: Mutex<Vec<HttpRequest>>,
    script: Mutex<Script>,
}

impl RecordingExchange {
    pub fn new() -> Self {
        Self::default()
    }

    /// Answers every exchange with `status` and `body`.
    pub fn answer(&self, status: u16, body: impl Into<String>) {
        *self.script.lock().expect("recording exchange lock") = Script::Reply(HttpResponse {
            head: HttpHead {
                status,
                ..HttpHead::default()
            },
            body: body.into().into_bytes(),
        });
    }

    /// Fails every exchange like a refused or broken request would.
    pub fn fail(&self, error: FirebaseError) {
        *self.script.lock().expect("recording exchange lock") = Script::Fail(error);
    }

    /// The requests so far, in order.
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.requests
            .lock()
            .expect("recording exchange lock")
            .clone()
    }
}

impl Exchange for RecordingExchange {
    fn exchange<'a>(
        &'a self,
        request: HttpRequest,
    ) -> Pin<Box<dyn Future<Output = Result<HttpResponse, FirebaseError>> + 'a>> {
        Box::pin(async move {
            self.requests
                .lock()
                .expect("recording exchange lock")
                .push(request);
            match &*self.script.lock().expect("recording exchange lock") {
                Script::Idle => Err(FirebaseError::new(
                    nsh_firebase_core::error::NETWORK,
                    "the recording exchange has no scripted answer",
                )),
                Script::Reply(response) => Ok(response.clone()),
                Script::Fail(error) => Err(error.clone()),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_recorder_keeps_its_scripted_reply_until_replaced() {
        let exchange = RecordingExchange::new();
        exchange.answer(200, r#"{"token":"t","ttl":"60s"}"#);
        let first = exchange
            .exchange(HttpRequest::post("http://x", Vec::new()))
            .await
            .unwrap();
        assert_eq!(first.status(), 200);
        exchange.fail(FirebaseError::new(nsh_firebase_core::error::NETWORK, "down"));
        let error = exchange
            .exchange(HttpRequest::post("http://x", Vec::new()))
            .await
            .unwrap_err();
        assert_eq!(error.code, nsh_firebase_core::error::NETWORK);
        assert_eq!(exchange.requests().len(), 2);
    }

    #[tokio::test]
    async fn an_unscripted_recorder_fails_clearly() {
        let exchange = RecordingExchange::new();
        let error = exchange
            .exchange(HttpRequest::post("http://x", Vec::new()))
            .await
            .unwrap_err();
        assert_eq!(error.code, nsh_firebase_core::error::NETWORK);
    }
}
