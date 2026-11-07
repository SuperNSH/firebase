//! The database and its references: the REST verbs of
//! <https://firebase.google.com/docs/database/rest/save-data> and the
//! reads of <https://firebase.google.com/docs/database/rest/retrieve-data>
//! — `get`, `set`, multi-path `update`, `remove`, client-generated
//! `push`, conditional-write transactions (a GET with
//! `X-Firebase-ETag: true`, a PUT with `if-match`; a `412` carries the
//! current value and a fresh ETag, and the client rebuilds — the docs
//! do not retry for you) — plus the live listeners of
//! [`crate::listener`].

use std::sync::{Arc, Mutex};

use nsh_core::element::HttpResponse;
use nsh_core::platform::HttpRequest;
use nsh_core::prelude::{cleanup, signal};
use nsh_core::time::unix_ms;
use nsh_firebase_core::{
    FirebaseApp, FirebaseError,
    error::{ACCESS_DENIED, INVALID_ARGUMENT, NETWORK},
    transport::send,
};
use serde_json::Value as JsonValue;

use crate::listener::{
    EngineReconnect, ListenHandle, ListenSignals, Listener, ReconnectClock, ReconnectPolicy,
    StreamTransport, start_listen,
};
use crate::push::push_id;
use crate::query::{Order, QueryParams, sort_children};

/// A transaction's outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct TransactionResult {
    /// Whether the write happened. False when `apply` aborted (returning
    /// none) or the attempts ran out.
    pub committed: bool,
    /// The value at the ref when it ended.
    pub snapshot: Option<JsonValue>,
}

/// Conditional writes give up after this many `412` rebuilds (the SDK's
/// own cap); the last snapshot stands, `committed` false.
const MAX_TRANSACTION_ATTEMPTS: u32 = 25;

/// One app's Realtime Database: the base URL (the emulator switch wins
/// over `databaseUrl`) and the auth token `?auth=` carries.
pub struct Database {
    base_url: String,
    auth_token: Mutex<Option<String>>,
    transport: Arc<dyn StreamTransport>,
}

impl Database {
    /// The database of `app`, live listeners running on `transport`.
    /// The app needs a database URL or a database emulator switch. Hold
    /// the returned Arc for the app's lifetime (as `getDatabase` caches
    /// per app in TypeScript): a second `from_app` makes a second
    /// database, and an auth token set on one reaches only the refs of
    /// its own Arc.
    pub fn from_app(
        app: &FirebaseApp,
        transport: Arc<dyn StreamTransport>,
    ) -> Result<Arc<Self>, FirebaseError> {
        let base_url = app.options.database_base_url().ok_or_else(|| {
            FirebaseError::new(
                INVALID_ARGUMENT,
                "the app has no databaseUrl and no database emulator",
            )
        })?;
        Ok(Arc::new(Self {
            base_url,
            auth_token: Mutex::new(None),
            transport,
        }))
    }

    /// The id token `?auth=` carries on every request (the documented
    /// query-parameter form, REST page "Auth"); none: the rules decide.
    /// The auth area's refreshed tokens land here.
    pub fn set_auth_token(&self, token: Option<String>) {
        *self.auth_token.lock().expect("auth token") = token;
    }

    /// The token now set, as a request builder reads it.
    fn auth(&self) -> Option<String> {
        self.auth_token.lock().expect("auth token").clone()
    }

    /// A reference at `path` (relative, `/`-separated; `""` is the root).
    pub fn r#ref(self: &Arc<Self>, path: &str) -> DatabaseReference {
        DatabaseReference {
            db: Arc::clone(self),
            path: path.trim_matches('/').to_owned(),
            query: QueryParams::default(),
        }
    }
}

/// A location in the database, with the query its reads carry.
#[derive(Clone)]
pub struct DatabaseReference {
    db: Arc<Database>,
    path: String,
    query: QueryParams,
}

impl DatabaseReference {
    /// A reference under this one (`"a/b"` descends two levels).
    #[must_use = "a reference does nothing until a call runs"]
    pub fn child(&self, path: &str) -> Self {
        let path = path.trim_matches('/');
        let full = if self.path.is_empty() {
            path.to_owned()
        } else if path.is_empty() {
            self.path.clone()
        } else {
            format!("{}/{path}", self.path)
        };
        Self {
            db: Arc::clone(&self.db),
            path: full,
            query: self.query.clone(),
        }
    }

    /// The request URL: `<base>/<path>.json`, the query's parameters,
    /// then `auth`.
    fn url(&self, params: &[(String, String)]) -> String {
        // Each segment is percent-encoded (a space or a `?` is a legal
        // RTDB key and must not break the URL); the slashes that separate
        // segments stay.
        let path = self
            .path
            .split('/')
            .map(crate::query::encode_component)
            .collect::<Vec<_>>()
            .join("/");
        let mut url = nsh_firebase_core::join_url(&self.db.base_url, &format!("{path}.json"));
        let mut query = params.to_vec();
        if let Some(token) = self.db.auth() {
            query.push(("auth".to_owned(), token));
        }
        if !query.is_empty() {
            let pairs: Vec<String> = query
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect();
            url.push('?');
            url.push_str(&pairs.join("&"));
        }
        url
    }

    /// The plain read-write URL (transactions ignore a query: they
    /// compare whole values).
    fn plain_url(&self) -> String {
        self.url(&[])
    }

    /// Reads the value, none when the location holds null. A queried
    /// read sorts its children (the REST answers unsorted).
    pub async fn get(&self) -> Result<Option<JsonValue>, FirebaseError> {
        let params = if self.query.is_empty() {
            Vec::new()
        } else {
            self.query.encode()?
        };
        let response = send(HttpRequest::get(self.url(&params))).await?;
        let response = map_status(response)?;
        let text = response.text();
        let Some(mut value) = parse_optional(&text)? else {
            return Ok(None);
        };
        if !self.query.is_empty() {
            sort_children(&mut value, &self.order());
        }
        Ok(Some(value))
    }

    fn order(&self) -> Order {
        self.query
            .order_by
            .as_deref()
            .map(Order::parse)
            .expect("a query with filters has an orderBy")
    }

    /// Writes `value` (a `null` is a remove).
    pub async fn set(&self, value: &JsonValue) -> Result<(), FirebaseError> {
        let body = serde_json::to_string(value).expect("a JSON value serializes");
        let response = send(HttpRequest::new("PUT", self.plain_url()).body(body)).await?;
        map_status(response)?;
        Ok(())
    }

    /// Merges `value`'s keys (relative paths from this ref, `"a/b"`
    /// descending) into what is there.
    pub async fn update(&self, value: &JsonValue) -> Result<(), FirebaseError> {
        if !value.is_object() {
            return Err(FirebaseError::new(
                INVALID_ARGUMENT,
                "update takes an object of relative paths to values",
            ));
        }
        let body = serde_json::to_string(value).expect("a JSON value serializes");
        let response = send(HttpRequest::new("PATCH", self.plain_url()).body(body)).await?;
        map_status(response)?;
        Ok(())
    }

    /// Deletes the location.
    pub async fn remove(&self) -> Result<(), FirebaseError> {
        let response = send(HttpRequest::new("DELETE", self.plain_url())).await?;
        map_status(response)?;
        Ok(())
    }

    /// Generates a push id (the timestamp of the app's clock, plus
    /// random bits; two within one millisecond keep their order) and
    /// writes `value` there. The id is known before the write lands,
    /// so lists can hold it at once.
    pub async fn push(&self, value: &JsonValue) -> Result<String, FirebaseError> {
        let id = push_id(unix_ms() as i64);
        self.child(&id).set(value).await?;
        Ok(id)
    }

    /// Runs a transaction: reads with an ETag, applies `apply` to the
    /// current value (returning none aborts, committed false), and puts
    /// with `if-match`; on the server's `412` it rebuilds from the
    /// carried value, at most [`MAX_TRANSACTION_ATTEMPTS`] times.
    pub async fn transaction(
        &self,
        apply: impl Fn(Option<&JsonValue>) -> Option<JsonValue> + Send + Sync,
    ) -> Result<TransactionResult, FirebaseError> {
        let url = self.plain_url();
        for _ in 0..MAX_TRANSACTION_ATTEMPTS {
            let response =
                send(HttpRequest::get(url.clone()).header("X-Firebase-ETag", "true")).await?;
            let response = map_status(response)?;
            let Some(etag) = response.header("ETag").map(str::to_owned) else {
                return Err(FirebaseError::new(
                    NETWORK,
                    "the database sent no ETag for the transaction read",
                ));
            };
            let current = parse_optional(&response.text())?;
            let Some(next) = apply(current.as_ref()) else {
                return Ok(TransactionResult {
                    committed: false,
                    snapshot: current,
                });
            };
            let body = serde_json::to_string(&next).expect("a JSON value serializes");
            let response = send(
                HttpRequest::new("PUT", url.clone())
                    .body(body)
                    .header("if-match", etag),
            )
            .await?;
            if (200..300).contains(&response.status()) {
                return Ok(TransactionResult {
                    committed: true,
                    snapshot: parse_optional(&response.text())?,
                });
            }
            if response.status() == 412 {
                // Changed elsewhere: the next round's tagged read
                // rebuilds from what the server holds now.
                continue;
            }
            return Err(map_status(response).unwrap_err());
        }
        let snapshot = self.get().await?;
        Ok(TransactionResult {
            committed: false,
            snapshot,
        })
    }

    /// The query builders: each returns a reference with one more
    /// filter, as React Native Firebase's chain does.
    #[must_use]
    pub fn order_by_child(&self, key: &str) -> Self {
        self.with_query(|query| query.order_by = Some(key.to_owned()))
    }

    #[must_use]
    pub fn order_by_key(&self) -> Self {
        self.order_by_child(crate::query::ORDER_KEY)
    }

    #[must_use]
    pub fn order_by_value(&self) -> Self {
        self.order_by_child(crate::query::ORDER_VALUE)
    }

    /// An inclusive lower bound (a number, string, boolean or null).
    #[must_use]
    pub fn start_at(&self, value: impl Into<JsonValue>) -> Self {
        self.with_query(|query| query.start_at = Some(value.into()))
    }

    /// An inclusive upper bound.
    #[must_use]
    pub fn end_at(&self, value: impl Into<JsonValue>) -> Self {
        self.with_query(|query| query.end_at = Some(value.into()))
    }

    /// Exactly this value.
    #[must_use]
    pub fn equal_to(&self, value: impl Into<JsonValue>) -> Self {
        self.with_query(|query| query.equal_to = Some(value.into()))
    }

    #[must_use]
    pub fn limit_to_first(&self, count: u32) -> Self {
        self.with_query(|query| query.limit_to_first = Some(count))
    }

    #[must_use]
    pub fn limit_to_last(&self, count: u32) -> Self {
        self.with_query(|query| query.limit_to_last = Some(count))
    }

    fn with_query(&self, change: impl FnOnce(&mut QueryParams)) -> Self {
        let mut query = self.query.clone();
        change(&mut query);
        Self {
            db: Arc::clone(&self.db),
            path: self.path.clone(),
            query,
        }
    }

    /// Subscribes a listener: the value and the connection state arrive
    /// through `listener` (on the engine thread when the transport
    /// delivers there). Off with the handle. `reconnect` schedules the
    /// waits after a loss — [`EngineReconnect`] on an engine thread,
    /// [`crate::listener::ThreadReconnect`] where none runs. An invalid
    /// query is an error here, as `get` on the same ref is: subscribe
    /// time fails loudly, no silent reconnect loop.
    pub fn listen(
        &self,
        reconnect: Arc<dyn ReconnectClock>,
        listener: Listener,
    ) -> Result<ListenHandle, FirebaseError> {
        // The query encodes once here; the factory carries the encoded
        // parameters, so no attempt can trip over it later.
        let params = if self.query.is_empty() {
            Vec::new()
        } else {
            self.query.encode()?
        };
        Ok(start_listen(
            {
                let r#ref = self.clone();
                move || r#ref.stream_request(&params)
            },
            Arc::clone(&self.db.transport),
            reconnect,
            ReconnectPolicy::default(),
            listener,
        ))
    }

    /// The value and the status as signals, so UI bindings update by
    /// themselves: `value` holds none until the first `put`, `status`
    /// the connection's state. On the engine thread; reconnects wait on
    /// the app's clock (a manual test clock moves them). The listener
    /// stops in the cleanup of the scope it was made in; [`ListenSignals::off`]
    /// stops it before that. An invalid query is an error, as `listen`
    /// is.
    pub fn on_value(&self) -> Result<ListenSignals, FirebaseError> {
        let value = signal::<Option<JsonValue>>(None);
        let status = signal(crate::listener::ListenStatus::Connecting);
        let (listener_value, listener_status) = (value, status);
        let listener: Listener = Arc::new(move |snapshot, state| {
            listener_value.try_set(snapshot.cloned());
            listener_status.try_set(state);
        });
        let handle = self.listen(Arc::new(EngineReconnect), listener)?;
        cleanup({
            let handle = handle.clone();
            move || handle.off()
        });
        Ok(ListenSignals {
            value,
            status,
            handle,
        })
    }

    /// The GET a live listener holds: the read URL (its parameters
    /// encoded at subscribe time) plus the `text/event-stream` accept
    /// header the streaming section asks for.
    fn stream_request(&self, params: &[(String, String)]) -> HttpRequest {
        HttpRequest::get(self.url(params)).header("Accept", "text/event-stream")
    }
}

/// Maps a failed REST response to the stable codes: a `permission_denied`
/// body is an access denial, a wrong request an invalid argument, and
/// everything else a network failure.
fn map_status(response: HttpResponse) -> Result<HttpResponse, FirebaseError> {
    let status = response.status();
    if (200..300).contains(&status) {
        return Ok(response);
    }
    let text = response.text();
    let message = serde_json::from_str::<JsonValue>(&text)
        .ok()
        .and_then(|body| {
            body.get("error")
                .and_then(JsonValue::as_str)
                .map(str::to_owned)
        })
        .unwrap_or(text);
    let code = match status {
        401 | 403 => ACCESS_DENIED,
        400 | 404 => INVALID_ARGUMENT,
        _ => NETWORK,
    };
    Err(FirebaseError::new(code, format!("{message} ({status})")))
}

/// A body of `null` (an absent location) is none.
fn parse_optional(text: &str) -> Result<Option<JsonValue>, FirebaseError> {
    if text.is_empty() || text == "null" {
        return Ok(None);
    }
    serde_json::from_str(text).map(Some).map_err(|error| {
        FirebaseError::new(NETWORK, format!("the database sent invalid JSON: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nsh_firebase_core::FirebaseOptions;

    /// A transport whose opening is a failure: nothing here listens.
    struct Never;
    impl crate::listener::StreamTransport for Never {
        fn open(
            &self,
            _request: HttpRequest,
            _sink: Arc<dyn crate::listener::StreamSink>,
        ) -> Box<dyn crate::listener::StreamTask> {
            unreachable!("no listener in these tests")
        }
    }

    fn db_with_token(token: Option<&str>) -> Arc<Database> {
        let app = nsh_firebase_core::initialize_app(FirebaseOptions::demo(), "db-unit").unwrap();
        let db = Database::from_app(&app, Arc::new(Never)).unwrap();
        db.set_auth_token(token.map(str::to_owned));
        db
    }

    #[test]
    fn the_url_carries_the_path_params_and_auth_in_order() {
        let db = db_with_token(Some("tok"));
        let r#ref = db.r#ref("scores");
        let query = QueryParams {
            order_by: Some("score".to_owned()),
            limit_to_first: Some(2),
            ..QueryParams::default()
        };
        let r#ref = r#ref.order_by_child("score").limit_to_first(2);
        assert_eq!(
            r#ref.url(&query.encode().unwrap()),
            "http://127.0.0.1:19000/scores.json?orderBy=%22score%22&limitToFirst=2&auth=tok"
        );
        // The transaction's URL is the plain one, no query.
        assert_eq!(
            r#ref.plain_url(),
            "http://127.0.0.1:19000/scores.json?auth=tok"
        );
    }

    /// A space or a `?` is a legal RTDB key and must not break the URL;
    /// the slashes that separate segments stay.
    #[test]
    fn a_path_with_a_space_or_a_question_mark_is_percent_encoded() {
        let db = db_with_token(None);
        assert_eq!(
            db.r#ref("a b").plain_url(),
            "http://127.0.0.1:19000/a%20b.json"
        );
        assert_eq!(
            db.r#ref("a?b").plain_url(),
            "http://127.0.0.1:19000/a%3Fb.json"
        );
        assert_eq!(
            db.r#ref("a b/c?d").plain_url(),
            "http://127.0.0.1:19000/a%20b/c%3Fd.json"
        );
    }

    #[test]
    fn without_a_token_the_url_has_no_auth() {
        let db = db_with_token(None);
        assert_eq!(db.r#ref("").plain_url(), "http://127.0.0.1:19000/.json");
    }

    #[test]
    fn child_joins_paths_without_double_slashes() {
        let db = db_with_token(None);
        let root = db.r#ref("");
        assert_eq!(root.child("a/b").child("/c/").path, "a/b/c");
        assert_eq!(root.child("a").child("").path, "a");
    }

    #[test]
    fn the_root_ref_has_an_empty_path() {
        let db = db_with_token(None);
        assert_eq!(db.r#ref("/todos/").path, "todos");
    }

    fn response_of(status: u16, body: &str) -> HttpResponse {
        let mut response = HttpResponse::default();
        response.head.status = status;
        response.body = body.as_bytes().to_vec();
        response
    }

    #[test]
    fn permission_denied_maps_to_access_denied() {
        let error = map_status(response_of(403, r#"{"error":"permission_denied"}"#)).unwrap_err();
        assert_eq!(error.code, ACCESS_DENIED);
        assert_eq!(error.message, "permission_denied (403)");
    }

    #[test]
    fn a_wrong_request_is_invalid_argument_and_other_statuses_are_network() {
        assert_eq!(
            map_status(response_of(400, r#"{"error":"bad"}"#))
                .unwrap_err()
                .code,
            INVALID_ARGUMENT
        );
        assert_eq!(
            map_status(response_of(500, "boom")).unwrap_err().code,
            NETWORK
        );
        assert!(map_status(response_of(200, "ok")).is_ok());
    }

    #[test]
    fn a_null_body_reads_as_none_and_garbage_is_a_network_error() {
        assert_eq!(parse_optional("null").unwrap(), None);
        assert_eq!(parse_optional("").unwrap(), None);
        assert_eq!(parse_optional("5").unwrap(), Some(json_value(5)));
        assert_eq!(parse_optional("{bad").unwrap_err().code, NETWORK);
    }

    fn json_value(at: i64) -> JsonValue {
        serde_json::from_str(&at.to_string()).unwrap()
    }

    #[test]
    fn an_update_of_a_non_object_is_invalid_argument() {
        // The shape check returns before any request goes out, so one
        // poll off the engine drives it; anything Pending is a bug here.
        let error = poll_once(async {
            db_with_token(None)
                .r#ref("x")
                .update(&JsonValue::Number(1.into()))
                .await
                .unwrap_err()
        });
        assert_eq!(error.code, INVALID_ARGUMENT);
    }

    /// One poll with a no-op waker, for the checks that answer at once.
    fn poll_once<F: Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match future.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(output) => output,
            std::task::Poll::Pending => panic!("the future must not wait here"),
        }
    }
}
