//! The Firestore API of one app: documents CRUD and structured queries
//! over the v1 REST surface (the emulator by default), or native routing
//! through the coordinator-fixed handlers when a host is wired.
//! `packages/nsh-firebase/src/firestore/firestore.ts` mirrors this name
//! for name.
//!
//! Native handler contract (coordinator-fixed; the native-stack agent
//! implements exactly these on both hosts):
//! - firebase.firestore.getDoc({path}) -> {exists: bool, data: object|null}
//! - firebase.firestore.setDoc({path, data, merge?}) -> {}
//! - firebase.firestore.addDoc({collectionPath, data}) -> {id}
//! - firebase.firestore.deleteDoc({path}) -> {}
//! - firebase.firestore.runQuery({collectionPath, where?, orderBy?, limit?})
//!   -> {docs: [{id, data}]}
//!
//! The REST tier is the pure tier: document reads, writes, creates,
//! deletes and shallow structured queries, with values in the REST
//! typed-value encoding. There are no realtime listeners here: the REST
//! surface has none to speak of, and the contract above adds no watch
//! handler on purpose. The native tier's hosts return JSON maps, so its
//! replies pass through without the typed-value decode.

use std::sync::{Arc, Mutex};

use nsh_core::{element::HttpResponse, platform::HttpRequest};
use nsh_firebase_core::{FirebaseApp, FirebaseError, FirebaseHost, send};
use serde_json::Value as JsonValue;
use serde_json::json;

use crate::wire::{
    self, DocSnapshot, FirestoreQuery, QueryDoc, ensure_ok, parse_body, query_doc_from,
    snapshot_from,
};

/// The Firestore of one app. Cheap to clone; the auth token is shared
/// across the clones.
#[derive(Clone)]
pub struct Firestore {
    project_id: String,
    rest_base: String,
    host: Option<Arc<dyn FirebaseHost>>,
    auth_token: Arc<Mutex<Option<String>>>,
}

impl Firestore {
    /// The Firestore of `app`, pointed at the suite's Firestore emulator
    /// (127.0.0.1:18080). Wire [`Self::with_host`] to route through the
    /// native handlers instead, or [`Self::with_rest_base`] for another
    /// endpoint (production needs an id token via [`Self::set_auth_token`]).
    pub fn new(app: &Arc<FirebaseApp>) -> Self {
        Self {
            project_id: app.options.project_id.clone(),
            rest_base: wire::emulator_base(None),
            host: None,
            auth_token: Arc::new(Mutex::new(None)),
        }
    }

    /// Points the pure tier at `rest_base` (the endpoint root, without
    /// `/v1`). The native tier ignores it.
    pub fn with_rest_base(mut self, rest_base: impl Into<String>) -> Self {
        self.rest_base = rest_base.into();
        self
    }

    /// Routes every call through the host's `firebase.firestore.*`
    /// handlers instead of fetch, per the native contract.
    pub fn with_host(mut self, host: Arc<dyn FirebaseHost>) -> Self {
        self.host = Some(host);
        self
    }

    /// The id token `Authorization: Bearer` carries on the REST calls
    /// (the documented REST auth; the rules then see the user). None:
    /// the rules decide. The auth area's refreshed tokens land here. The
    /// native tier does its own auth.
    pub fn set_auth_token(&self, token: Option<String>) {
        *self.auth_token.lock().expect("auth token") = token;
    }

    fn auth(&self) -> Option<String> {
        self.auth_token.lock().expect("auth token").clone()
    }

    /// Reads one document. A 404 is the REST read of an absent document,
    /// not a failure: `exists: false`, `data: none`.
    pub async fn get_doc(&self, path: &str) -> Result<DocSnapshot, FirebaseError> {
        wire::split_doc_path(path)?;
        match &self.host {
            Some(host) => {
                let reply = self
                    .call(host, wire::native::GET_DOC, json!({ "path": path }))
                    .await?;
                let exists = reply
                    .get("exists")
                    .and_then(JsonValue::as_bool)
                    .ok_or_else(|| {
                        wire::invalid_response(
                            "`firebase.firestore.getDoc` answered without `exists`",
                        )
                    })?;
                let data = if !exists {
                    None
                } else {
                    let data = reply.get("data").filter(|data| data.is_object()).cloned();
                    Some(data.ok_or_else(|| {
                        wire::invalid_response(
                            "`firebase.firestore.getDoc` answered without `data`",
                        )
                    })?)
                };
                Ok(DocSnapshot { exists, data })
            }
            None => {
                let url = wire::document_url(&self.documents_base(), path)?;
                let response = self.send(HttpRequest::get(url)).await?;
                if response.status() == 404 {
                    return Ok(DocSnapshot {
                        exists: false,
                        data: None,
                    });
                }
                let document = parse_body(&ensure_ok(response)?.text())?;
                snapshot_from(&document)
            }
        }
    }

    /// Writes `data` (a JSON object of fields) at `path`, creating the
    /// document if absent. With `merge` only the leaf field paths of
    /// `data` are written (the REST patch's `updateMask`); without it the
    /// document is replaced whole.
    pub async fn set_doc(
        &self,
        path: &str,
        data: &JsonValue,
        merge: bool,
    ) -> Result<(), FirebaseError> {
        let fields = wire::encode_fields(data)?;
        // The mask is computed before routing, so a merge the REST patch
        // cannot express is refused in both tiers.
        let mask = merge.then(|| wire::leaf_paths(data)).transpose()?;
        match &self.host {
            Some(host) => {
                let mut args = json!({ "path": path, "data": data });
                if merge {
                    args["merge"] = JsonValue::Bool(true);
                }
                self.call(host, wire::native::SET_DOC, args).await?;
                Ok(())
            }
            None => {
                let url = wire::document_url(&self.documents_base(), path)?;
                let url = match &mask {
                    Some(paths) => format!("{url}?{}", wire::merge_mask_query(paths)),
                    None => url,
                };
                let body = json!({ "fields": fields }).to_string();
                let request = HttpRequest::new("PATCH", url)
                    .header("Content-Type", "application/json")
                    .body(body);
                ensure_ok(self.send(request).await?)?;
                Ok(())
            }
        }
    }

    /// Creates a document in `collection_path` with a server-assigned id
    /// (the REST createDocument POST) and answers it.
    pub async fn add_doc(
        &self,
        collection_path: &str,
        data: &JsonValue,
    ) -> Result<String, FirebaseError> {
        wire::split_collection_path(collection_path)?;
        let fields = wire::encode_fields(data)?;
        match &self.host {
            Some(host) => {
                let reply = self
                    .call(
                        host,
                        wire::native::ADD_DOC,
                        json!({ "collectionPath": collection_path, "data": data }),
                    )
                    .await?;
                reply
                    .get("id")
                    .and_then(JsonValue::as_str)
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        wire::invalid_response(
                            "`firebase.firestore.addDoc` answered without an `id`",
                        )
                    })
            }
            None => {
                let url = wire::collection_url(&self.documents_base(), collection_path)?;
                let body = json!({ "fields": fields }).to_string();
                let request = HttpRequest::post(url, body.into_bytes())
                    .header("Content-Type", "application/json");
                let response = ensure_ok(self.send(request).await?)?;
                Ok(query_doc_from(&parse_body(&response.text())?)?.id)
            }
        }
    }

    /// Deletes the document at `path`. A 404 (already absent) is done,
    /// as a Firestore delete is idempotent.
    pub async fn delete_doc(&self, path: &str) -> Result<(), FirebaseError> {
        wire::split_doc_path(path)?;
        match &self.host {
            Some(host) => {
                self.call(host, wire::native::DELETE_DOC, json!({ "path": path }))
                    .await?;
                Ok(())
            }
            None => {
                let url = wire::document_url(&self.documents_base(), path)?;
                let response = self.send(HttpRequest::new("DELETE", url)).await?;
                if response.status() == 404 {
                    return Ok(());
                }
                ensure_ok(response)?;
                Ok(())
            }
        }
    }

    /// Runs the structured query: the collection's documents that match
    /// the where filter, sorted, at most `limit` of them.
    pub async fn run_query(&self, query: &FirestoreQuery) -> Result<Vec<QueryDoc>, FirebaseError> {
        let plan = wire::run_query_plan(&self.rest_base, &self.project_id, query)?;
        match &self.host {
            Some(host) => {
                let reply = self
                    .call(host, wire::native::RUN_QUERY, plan.native_args)
                    .await?;
                docs_of(&reply)
            }
            None => {
                let request = HttpRequest::post(plan.url, plan.body.to_string().into_bytes())
                    .header("Content-Type", "application/json");
                let response = ensure_ok(self.send(request).await?)?;
                wire::docs_from_run_query(&response.text())
            }
        }
    }

    /// The documents root of the configured endpoint.
    fn documents_base(&self) -> String {
        wire::documents_base(&self.rest_base, &self.project_id)
    }

    /// One native handler call; a refusal keeps the platform's code and
    /// message.
    async fn call(
        &self,
        host: &Arc<dyn FirebaseHost>,
        name: &str,
        args: JsonValue,
    ) -> Result<JsonValue, FirebaseError> {
        host.call(name, args).await
    }

    /// Sends one REST call with the auth header of the moment. Must run
    /// on the engine thread: that is where fetch lives, and
    /// `capabilities.network` gates the host.
    async fn send(&self, mut request: HttpRequest) -> Result<HttpResponse, FirebaseError> {
        if let Some(token) = self.auth() {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        send(request).await
    }
}

/// The native runQuery reply's `{docs: [{id, data}]}`, entry by entry.
fn docs_of(reply: &JsonValue) -> Result<Vec<QueryDoc>, FirebaseError> {
    let Some(docs) = reply.get("docs").and_then(JsonValue::as_array) else {
        return Err(wire::invalid_response(
            "`firebase.firestore.runQuery` answered without `docs`",
        ));
    };
    let mut out = Vec::with_capacity(docs.len());
    for doc in docs {
        let id = doc
            .get("id")
            .and_then(JsonValue::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| wire::invalid_response("a `docs` entry answered without an `id`"))?;
        out.push(QueryDoc {
            id: id.to_owned(),
            data: doc.get("data").cloned().unwrap_or(JsonValue::Null),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{WhereFilter, WhereOp};
    use nsh_firebase_core::error;
    use serde_json::json;
    use std::sync::Mutex as StdMutex;

    fn app() -> Arc<FirebaseApp> {
        nsh_firebase_core::initialize_app(
            nsh_firebase_core::FirebaseOptions::demo(),
            "firestore-service-test",
        )
        .unwrap()
    }

    /// Records every call and answers from a canned table, the way the
    /// native areas' handlers do.
    struct RecordingHost {
        calls: StdMutex<Vec<(String, JsonValue)>>,
        answers: StdMutex<Vec<JsonValue>>,
    }

    impl FirebaseHost for RecordingHost {
        fn call(&self, name: &str, args: JsonValue) -> nsh_firebase_core::host::HostCallFuture {
            self.calls
                .lock()
                .expect("call log")
                .push((name.to_owned(), args));
            let answer = self
                .answers
                .lock()
                .expect("answers")
                .pop()
                .unwrap_or(JsonValue::Null);
            Box::pin(async move { Ok(answer) })
        }

        fn post(&self, _name: &str, _args: JsonValue) -> Result<(), FirebaseError> {
            unimplemented!("firestore never posts")
        }
    }

    fn host_with(replies: Vec<JsonValue>) -> (Firestore, Arc<RecordingHost>) {
        let channel = Arc::new(RecordingHost {
            calls: StdMutex::new(Vec::new()),
            answers: StdMutex::new(replies),
        });
        let host = Firestore::new(&app()).with_host(Arc::clone(&channel) as _);
        (host, channel)
    }

    fn calls_of(host: &RecordingHost) -> Vec<(String, JsonValue)> {
        host.calls.lock().expect("call log").clone()
    }

    #[tokio::test]
    async fn get_doc_routes_the_contract_args_and_reply() {
        let reply = json!({
            "exists": true,
            "data": { "done": false, "title": "one" },
        });
        let (firestore, host) = host_with(vec![reply]);
        let snapshot = firestore.get_doc("cities/LA").await.unwrap();
        assert!(snapshot.exists);
        assert_eq!(
            snapshot.data,
            Some(json!({ "done": false, "title": "one" }))
        );
        assert_eq!(
            calls_of(&host),
            vec![(
                "firebase.firestore.getDoc".to_owned(),
                json!({ "path": "cities/LA" })
            )]
        );
    }

    #[tokio::test]
    async fn an_absent_native_document_reads_exists_false() {
        let (firestore, host) = host_with(vec![json!({ "exists": false, "data": null })]);
        let snapshot = firestore.get_doc("cities/LA").await.unwrap();
        assert!(!snapshot.exists);
        assert_eq!(snapshot.data, None);
        assert_eq!(calls_of(&host).len(), 1);
    }

    #[tokio::test]
    async fn a_reply_without_the_contract_shape_is_invalid_response() {
        let (firestore, _host) = host_with(vec![json!({ "exists": true })]);
        let error = firestore.get_doc("cities/LA").await.unwrap_err();
        assert_eq!(error.code, wire::INVALID_RESPONSE);
        assert!(error.message.contains("`data`"), "{error}");
        let (firestore, _host) = host_with(vec![json!({})]);
        let error = firestore.get_doc("cities/LA").await.unwrap_err();
        assert_eq!(error.code, wire::INVALID_RESPONSE);
    }

    #[tokio::test]
    async fn set_doc_routes_path_data_and_merge_only_when_set() {
        let (firestore, host) = host_with(vec![json!({}), json!({})]);
        firestore
            .set_doc("cities/LA", &json!({ "pop": 500 }), false)
            .await
            .unwrap();
        firestore
            .set_doc("cities/LA", &json!({ "pop": 500 }), true)
            .await
            .unwrap();
        assert_eq!(
            calls_of(&host),
            vec![
                (
                    "firebase.firestore.setDoc".to_owned(),
                    json!({ "path": "cities/LA", "data": { "pop": 500 } })
                ),
                (
                    "firebase.firestore.setDoc".to_owned(),
                    json!({ "path": "cities/LA", "data": { "pop": 500 }, "merge": true })
                ),
            ]
        );
    }

    #[tokio::test]
    async fn add_doc_routes_the_collection_and_unwraps_the_id() {
        let (firestore, host) = host_with(vec![json!({ "id": "SERVER1" })]);
        let id = firestore
            .add_doc("cities", &json!({ "pop": 500 }))
            .await
            .unwrap();
        assert_eq!(id, "SERVER1");
        assert_eq!(
            calls_of(&host),
            vec![(
                "firebase.firestore.addDoc".to_owned(),
                json!({ "collectionPath": "cities", "data": { "pop": 500 } })
            )]
        );
        // A reply without an id is a broken shape.
        let (firestore, _host) = host_with(vec![json!({})]);
        let error = firestore.add_doc("cities", &json!({})).await.unwrap_err();
        assert_eq!(error.code, wire::INVALID_RESPONSE);
    }

    #[tokio::test]
    async fn delete_doc_routes_the_path_and_run_query_unwraps_docs() {
        let (firestore, host) = host_with(vec![
            json!({ "docs": [
                { "id": "N1", "data": { "n": 1 } },
                { "id": "N2", "data": { "n": 2 } },
            ] }),
            json!({}),
        ]);
        firestore.delete_doc("cities/LA").await.unwrap();
        let query = FirestoreQuery {
            collection_path: "cities".to_owned(),
            r#where: Some(WhereFilter {
                field: "state".to_owned(),
                op: WhereOp::Equal,
                value: json!("CA"),
            }),
            ..FirestoreQuery::default()
        };
        let docs = firestore.run_query(&query).await.unwrap();
        let want = vec![
            QueryDoc {
                id: "N1".to_owned(),
                data: json!({ "n": 1 }),
            },
            QueryDoc {
                id: "N2".to_owned(),
                data: json!({ "n": 2 }),
            },
        ];
        assert_eq!(docs, want);
        let calls = calls_of(&host);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "firebase.firestore.deleteDoc");
        assert_eq!(calls[0].1, json!({ "path": "cities/LA" }));
        assert_eq!(calls[1].0, "firebase.firestore.runQuery");
        assert_eq!(
            calls[1].1,
            json!({
                "collectionPath": "cities",
                "where": { "field": "state", "op": "==", "value": "CA" },
            })
        );
    }

    #[tokio::test]
    async fn a_run_query_reply_without_docs_is_invalid_response() {
        let reply = json!({ "docs": [{ "data": {} }] });
        let (firestore, _host) = host_with(vec![reply]);
        let query = FirestoreQuery {
            collection_path: "cities".to_owned(),
            ..FirestoreQuery::default()
        };
        let error = firestore.run_query(&query).await.unwrap_err();
        assert_eq!(error.code, wire::INVALID_RESPONSE);
    }

    #[tokio::test]
    async fn both_tiers_refuse_bad_shapes_before_their_transport() {
        // Native: the handler is never reached.
        let (firestore, host) = host_with(vec![json!({ "id": "x" })]);
        let error = firestore.get_doc("odd").await.unwrap_err();
        assert_eq!(error.code, error::INVALID_ARGUMENT);
        assert!(calls_of(&host).is_empty());
        let error = firestore
            .set_doc("cities/LA", &json!([1]), false)
            .await
            .unwrap_err();
        assert_eq!(error.code, error::INVALID_ARGUMENT);
        let error = firestore
            .set_doc("cities/LA", &json!({}), true)
            .await
            .unwrap_err();
        assert_eq!(
            error.message,
            "a merge write needs at least one field; the REST patch mask cannot be empty"
        );
        // Pure: no fetch can be reached in a unit test either; the plan
        // build fails first.
        let pure = Firestore::new(&app());
        let error = pure.get_doc("cities").await.unwrap_err();
        assert_eq!(error.code, error::INVALID_ARGUMENT);
    }
}
