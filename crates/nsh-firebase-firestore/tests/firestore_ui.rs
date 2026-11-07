//! Headless UI tests of the Firestore pure tier: documents CRUD and the
//! structured query, through a test-only component on a manual clock,
//! with a stubbed network (the v1 REST calls). The mirror of
//! crates/nsh-firebase-database/tests/database_ui.rs; native routing is
//! pinned by in-crate tests (the facade answers from a stub host there).

use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use nsh_core::{JsonValue, platform::HttpBody, prelude::*};
use nsh_engine::{Clock, EngineConfig, Network, Root};
use nsh_firebase_core::{FirebaseError, FirebaseOptions, initialize_app};
use nsh_firebase_firestore::{
    Direction, DocSnapshot, Firestore, FirestoreQuery, OrderBy, WhereFilter, WhereOp,
};
use nsh_testing::{Headless, StubNetwork, StubResponse};
use serde_json::json;

/// The manual clock's start: the same wall time every test uses.
fn clock_start() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_258_000)
}

fn start_with_network(root: Root, network: &Arc<StubNetwork>) -> Headless {
    let mut config = EngineConfig::new(root);
    config.clock = Clock::manual(clock_start());
    config.network = Some(Arc::clone(network) as Arc<dyn Network>);
    Headless::start(config).unwrap()
}

/// The documents root of the demo app's endpoint (the demo options name
/// demo-nsh).
fn root() -> String {
    "http://127.0.0.1:18080/v1/projects/demo-nsh/databases/(default)/documents".to_owned()
}

/// Formats a snapshot the way the screens show it.
fn show(snapshot: &DocSnapshot) -> String {
    let data = snapshot
        .data
        .as_ref()
        .map(JsonValue::to_string)
        .unwrap_or_else(|| "none".to_owned());
    format!("exists={} data={data}", snapshot.exists)
}

/// Formats a failed call the way the screens show it.
fn failed(error: FirebaseError) -> String {
    error.to_string()
}

/// The data the set button writes, in the key order its test asserts.
fn set_data() -> JsonValue {
    json!({
        "done": false,
        "title": "one",
        "count": 3,
        "price": 1.5,
        "tags": ["a", 2],
        "meta": { "deep": 1.5, "ok": true, "note": null },
    })
}

#[component]
fn FirestoreApp() -> impl IntoElement {
    let app = initialize_app(FirebaseOptions::demo(), "fs-ui").unwrap();
    let fs = Firestore::new(&app);
    let line = signal("ready".to_owned());
    view()
        .flex_1()
        .gap_1()
        .child(text(move || line.get()))
        .child(
            view()
                .test_id("set")
                .on_press({
                    let fs = fs.clone();
                    move || {
                        let fs = fs.clone();
                        spawn(async move {
                            match fs.set_doc("ui-todos/one", &set_data(), false).await {
                                Ok(()) => line.set("set: ok".to_owned()),
                                Err(error) => line.set(format!("set: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("set")),
        )
        .child(
            view()
                .test_id("merge")
                .on_press({
                    let fs = fs.clone();
                    move || {
                        let fs = fs.clone();
                        spawn(async move {
                            match fs
                                .set_doc(
                                    "ui-todos/one",
                                    &json!({ "title": "Merged", "meta": { "deep": 2 } }),
                                    true,
                                )
                                .await
                            {
                                Ok(()) => line.set("merge: ok".to_owned()),
                                Err(error) => line.set(format!("merge: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("merge")),
        )
        .child(
            view()
                .test_id("get")
                .on_press({
                    let fs = fs.clone();
                    move || {
                        let fs = fs.clone();
                        spawn(async move {
                            match fs.get_doc("ui-todos/one").await {
                                Ok(snapshot) => line.set(format!("get: {}", show(&snapshot))),
                                Err(error) => line.set(format!("get: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("get")),
        )
        .child(
            view()
                .test_id("missing")
                .on_press({
                    let fs = fs.clone();
                    move || {
                        let fs = fs.clone();
                        spawn(async move {
                            match fs.get_doc("ui-nowhere/none").await {
                                Ok(snapshot) => line.set(format!("missing: {}", show(&snapshot))),
                                Err(error) => line.set(format!("missing: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("missing")),
        )
        .child(
            view()
                .test_id("add")
                .on_press({
                    let fs = fs.clone();
                    move || {
                        let fs = fs.clone();
                        spawn(async move {
                            match fs.add_doc("ui-todos", &json!({ "title": "Added" })).await {
                                Ok(id) => line.set(format!("add: {id}")),
                                Err(error) => line.set(format!("add: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("add")),
        )
        .child(
            view()
                .test_id("delete")
                .on_press({
                    let fs = fs.clone();
                    move || {
                        let fs = fs.clone();
                        spawn(async move {
                            match fs.delete_doc("ui-todos/one").await {
                                Ok(()) => line.set("delete: ok".to_owned()),
                                Err(error) => line.set(format!("delete: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("delete")),
        )
        .child(
            view()
                .test_id("delete-missing")
                .on_press({
                    let fs = fs.clone();
                    move || {
                        let fs = fs.clone();
                        spawn(async move {
                            // An absent document deletes done: Firestore
                            // deletes are idempotent.
                            match fs.delete_doc("ui-gone/x").await {
                                Ok(()) => line.set("delete-missing: ok".to_owned()),
                                Err(error) => {
                                    line.set(format!("delete-missing: {}", failed(error)))
                                }
                            }
                        });
                    }
                })
                .child(text("delete missing")),
        )
        .child(
            view()
                .test_id("query")
                .on_press({
                    let fs = fs.clone();
                    move || {
                        let fs = fs.clone();
                        spawn(async move {
                            let query = FirestoreQuery {
                                collection_path: "ui-cities".to_owned(),
                                r#where: Some(WhereFilter {
                                    field: "state".to_owned(),
                                    op: WhereOp::Equal,
                                    value: json!("CA"),
                                }),
                                order_by: vec![OrderBy {
                                    field: "pop".to_owned(),
                                    direction: Direction::Desc,
                                }],
                                limit: Some(2),
                            };
                            match fs.run_query(&query).await {
                                Ok(docs) => {
                                    let ids = docs
                                        .iter()
                                        .map(|doc| doc.id.as_str())
                                        .collect::<Vec<_>>()
                                        .join(" ");
                                    line.set(format!("query: {ids}"));
                                }
                                Err(error) => line.set(format!("query: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("query")),
        )
        .child(
            view()
                .test_id("denied")
                .on_press({
                    let fs = fs.clone();
                    move || {
                        let fs = fs.clone();
                        spawn(async move {
                            match fs.get_doc("ui-denied/x").await {
                                Ok(snapshot) => line.set(format!("denied: {}", show(&snapshot))),
                                Err(error) => line.set(format!("denied: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("denied")),
        )
        .child(
            view()
                .test_id("bad-path")
                .on_press({
                    let fs = fs.clone();
                    move || {
                        let fs = fs.clone();
                        spawn(async move {
                            match fs.get_doc("ui-todos").await {
                                Ok(snapshot) => line.set(format!("bad-path: {}", show(&snapshot))),
                                Err(error) => line.set(format!("bad-path: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("bad path")),
        )
        .child(
            view()
                .test_id("typed")
                .on_press({
                    let fs = fs.clone();
                    move || {
                        let fs = fs.clone();
                        spawn(async move {
                            match fs.get_doc("ui-typed/x").await {
                                Ok(snapshot) => line.set(format!("typed: {}", show(&snapshot))),
                                Err(error) => line.set(format!("typed: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("typed")),
        )
        .child(
            view()
                .test_id("auth")
                .on_press({
                    let fs = fs.clone();
                    move || {
                        let fs = fs.clone();
                        spawn(async move {
                            fs.set_auth_token(Some("tok-1".to_owned()));
                            match fs.get_doc("ui-auth/a").await {
                                Ok(snapshot) => line.set(format!("auth: {}", show(&snapshot))),
                                Err(error) => line.set(format!("auth: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("auth")),
        )
        .child(
            view()
                .test_id("bad-json")
                .on_press({
                    let fs = fs.clone();
                    move || {
                        let fs = fs.clone();
                        spawn(async move {
                            match fs.get_doc("ui-badjson/x").await {
                                Ok(snapshot) => line.set(format!("bad-json: {}", show(&snapshot))),
                                Err(error) => line.set(format!("bad-json: {}", failed(error))),
                            }
                        });
                    }
                })
                .child(text("bad json")),
        )
}

fn route(method: &str, url: &str, status: u16, body: &str) -> (String, StubResponse) {
    let mut response = StubResponse::default();
    response.status = status;
    response.body = body.as_bytes().to_vec();
    (format!("{method} {url}"), response)
}

/// The body of a recorded request, which every write test asserts.
fn request_body(body: &HttpBody) -> String {
    match body {
        HttpBody::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        other => panic!("the request carried {other:?}"),
    }
}

/// The one line the app shows that starts with `prefix`.
fn line_of(app: &Headless, prefix: &str) -> String {
    let texts = app.screen().texts();
    let found = texts.iter().find(|text| text.starts_with(prefix));
    found.unwrap().to_owned().to_string()
}

#[test]
fn set_then_get_round_trip_through_the_typed_values() {
    let network = Arc::new(StubNetwork::new(vec![
        route("PATCH", &format!("{}/ui-todos/one", root()), 200, ""),
        route(
            "GET",
            &format!("{}/ui-todos/one", root()),
            200,
            r#"{"name": "projects/demo-nsh/databases/(default)/documents/ui-todos/one",
                "fields": {"done": {"booleanValue": false}, "title": {"stringValue": "one"},
                           "count": {"integerValue": "3"}}}"#,
        ),
    ]));
    let mut app = start_with_network(Root::of::<FirestoreApp>(), &network);
    app.wait_for("text:ready").unwrap();
    app.press("id:set").unwrap();
    app.wait_for("text:set: ok").unwrap();
    // The recorded request carries the whole typed-value document.
    let requests = network.requests();
    let patch = requests
        .iter()
        .find(|request| request.method == "PATCH")
        .unwrap();
    assert_eq!(patch.url, format!("{}/ui-todos/one", root()));
    let mut json_body = false;
    for (name, value) in &patch.headers {
        if name.eq_ignore_ascii_case("content-type") && value == "application/json" {
            json_body = true;
        }
    }
    assert!(json_body, "{:?}", patch.headers);
    assert_eq!(
        request_body(&patch.body),
        r#"{"fields":{"done":{"booleanValue":false},"title":{"stringValue":"one"},"count":{"integerValue":"3"},"price":{"doubleValue":1.5},"tags":{"arrayValue":{"values":[{"stringValue":"a"},{"integerValue":"2"}]}},"meta":{"mapValue":{"fields":{"deep":{"doubleValue":1.5},"ok":{"booleanValue":true},"note":{"nullValue":"NULL_VALUE"}}}}}}"#
    );
    app.press("id:get").unwrap();
    app.wait_for(r#"text:get: exists=true data={"done":false,"title":"one","count":3}"#)
        .unwrap();
}

#[test]
fn merge_writes_the_leaf_path_update_mask() {
    let network = Arc::new(StubNetwork::new(vec![route(
        "PATCH",
        &format!(
            "{}/ui-todos/one?updateMask.fieldPaths=title&updateMask.fieldPaths=meta.deep",
            root()
        ),
        200,
        "",
    )]));
    let mut app = start_with_network(Root::of::<FirestoreApp>(), &network);
    app.wait_for("text:ready").unwrap();
    app.press("id:merge").unwrap();
    app.wait_for("text:merge: ok").unwrap();
    let requests = network.requests();
    let patch = requests
        .iter()
        .find(|request| request.method == "PATCH")
        .unwrap();
    let mask = "updateMask.fieldPaths=title&updateMask.fieldPaths=meta.deep";
    assert_eq!(patch.url, format!("{}/ui-todos/one?{mask}", root()));
    assert_eq!(
        request_body(&patch.body),
        r#"{"fields":{"title":{"stringValue":"Merged"},"meta":{"mapValue":{"fields":{"deep":{"integerValue":"2"}}}}}}"#
    );
}

#[test]
fn a_missing_document_reads_as_exists_false_not_an_error() {
    let network = Arc::new(StubNetwork::new(vec![route(
        "GET",
        &format!("{}/ui-nowhere/none", root()),
        404,
        r#"{"error": {"code": 404, "message": "not there", "status": "NOT_FOUND"}}"#,
    )]));
    let mut app = start_with_network(Root::of::<FirestoreApp>(), &network);
    app.wait_for("text:ready").unwrap();
    app.press("id:missing").unwrap();
    app.wait_for("text:missing: exists=false data=none")
        .unwrap();
}

#[test]
fn add_doc_posts_to_the_collection_and_reads_the_id_from_the_name() {
    let network = Arc::new(StubNetwork::new(vec![route(
        "POST",
        &format!("{}/ui-todos", root()),
        200,
        r#"{"name": "projects/demo-nsh/databases/(default)/documents/ui-todos/SERVER1",
            "fields": {"title": {"stringValue": "Added"}}}"#,
    )]));
    let mut app = start_with_network(Root::of::<FirestoreApp>(), &network);
    app.wait_for("text:ready").unwrap();
    app.press("id:add").unwrap();
    app.wait_for("text:add: SERVER1").unwrap();
    let requests = network.requests();
    let post = requests
        .iter()
        .find(|request| request.method == "POST")
        .unwrap();
    assert_eq!(post.url, format!("{}/ui-todos", root()));
    assert_eq!(
        request_body(&post.body),
        r#"{"fields":{"title":{"stringValue":"Added"}}}"#
    );
}

#[test]
fn delete_doc_deletes_and_a_404_is_done() {
    let network = Arc::new(StubNetwork::new(vec![
        route("DELETE", &format!("{}/ui-todos/one", root()), 200, ""),
        route(
            "DELETE",
            &format!("{}/ui-gone/x", root()),
            404,
            r#"{"error": {"code": 404, "message": "gone already", "status": "NOT_FOUND"}}"#,
        ),
    ]));
    let mut app = start_with_network(Root::of::<FirestoreApp>(), &network);
    app.wait_for("text:ready").unwrap();
    app.press("id:delete").unwrap();
    app.wait_for("text:delete: ok").unwrap();
    app.press("id:delete-missing").unwrap();
    app.wait_for("text:delete-missing: ok").unwrap();
}

#[test]
fn a_queried_run_query_encodes_the_structured_query() {
    let network = Arc::new(StubNetwork::new(vec![route(
        "POST",
        &format!("{}:runQuery", root()),
        200,
        r#"[
            {"document": {"name": "projects/demo-nsh/databases/(default)/documents/ui-cities/SERVER1",
                          "fields": {"pop": {"integerValue": "500"}}},
             "readTime": "2026-01-01T00:00:00.000000001Z"},
            {"document": {"name": "projects/demo-nsh/databases/(default)/documents/ui-cities/SERVER2",
                          "fields": {"pop": {"integerValue": "300"}}},
             "readTime": "2026-01-01T00:00:00.000000001Z"},
            {"readTime": "2026-01-01T00:00:00.000000001Z"}
        ]"#,
    )]));
    let mut app = start_with_network(Root::of::<FirestoreApp>(), &network);
    app.wait_for("text:ready").unwrap();
    app.press("id:query").unwrap();
    app.wait_for("text:query: SERVER1 SERVER2").unwrap();
    let requests = network.requests();
    let post = requests
        .iter()
        .find(|request| request.method == "POST")
        .unwrap();
    assert_eq!(post.url, format!("{}:runQuery", root()));
    assert_eq!(
        request_body(&post.body),
        r#"{"parent":"projects/demo-nsh/databases/(default)/documents","structuredQuery":{"from":[{"collectionId":"ui-cities"}],"where":{"fieldFilter":{"field":{"fieldPath":"state"},"op":"EQUAL","value":{"stringValue":"CA"}}},"orderBy":[{"field":{"fieldPath":"pop"},"direction":"DESCENDING"}],"limit":2}}"#
    );
}

#[test]
fn a_denied_read_maps_to_permission_denied() {
    let network = Arc::new(StubNetwork::new(vec![route(
        "GET",
        &format!("{}/ui-denied/x", root()),
        403,
        r#"{"error": {"code": 403, "message": "Missing or insufficient permissions.", "status": "PERMISSION_DENIED"}}"#,
    )]));
    let mut app = start_with_network(Root::of::<FirestoreApp>(), &network);
    app.wait_for("text:ready").unwrap();
    app.press("id:denied").unwrap();
    let denied = line_of(&app, "denied: ");
    assert!(
        denied.contains("firestore/permission-denied: Missing or insufficient permissions."),
        "{denied}"
    );
}

#[test]
fn a_bad_path_is_refused_before_any_request() {
    let network = Arc::new(StubNetwork::new(Vec::<(String, StubResponse)>::new()));
    let mut app = start_with_network(Root::of::<FirestoreApp>(), &network);
    app.wait_for("text:ready").unwrap();
    app.press("id:bad-path").unwrap();
    let bad_path = line_of(&app, "bad-path: ");
    assert_eq!(
        bad_path.as_str(),
        "bad-path: core/invalid-argument: not a document path: `ui-todos` must name collection/document pairs (an even segment count)"
    );
    assert_eq!(network.requests().len(), 0);
}

#[test]
fn a_value_type_the_json_contract_cannot_carry_is_invalid_data() {
    let network = Arc::new(StubNetwork::new(vec![route(
        "GET",
        &format!("{}/ui-typed/x", root()),
        200,
        r#"{"name": "projects/demo-nsh/databases/(default)/documents/ui-typed/x",
            "fields": {"when": {"timestampValue": "2026-01-01T00:00:00Z"}}}"#,
    )]));
    let mut app = start_with_network(Root::of::<FirestoreApp>(), &network);
    app.wait_for("text:ready").unwrap();
    app.press("id:typed").unwrap();
    let typed = line_of(&app, "typed: ");
    assert!(typed.contains("timestampValue"), "{typed}");
    assert!(typed.contains("has no JSON form"), "{typed}");
}

#[test]
fn the_auth_token_rides_as_a_bearer_header() {
    let network = Arc::new(StubNetwork::new(vec![route(
        "GET",
        &format!("{}/ui-auth/a", root()),
        200,
        r#"{"name": "projects/demo-nsh/databases/(default)/documents/ui-auth/a",
            "fields": {"ok": {"booleanValue": true}}}"#,
    )]));
    let mut app = start_with_network(Root::of::<FirestoreApp>(), &network);
    app.wait_for("text:ready").unwrap();
    app.press("id:auth").unwrap();
    app.wait_for(r#"text:auth: exists=true data={"ok":true}"#)
        .unwrap();
    let requests = network.requests();
    let get = requests
        .iter()
        .find(|request| request.method == "GET")
        .unwrap();
    let mut bearer = false;
    for (name, value) in &get.headers {
        if name.eq_ignore_ascii_case("authorization") && value == "Bearer tok-1" {
            bearer = true;
        }
    }
    assert!(bearer, "{:?}", get.headers);
}

#[test]
fn a_non_json_body_is_invalid_response() {
    let network = Arc::new(StubNetwork::new(vec![route(
        "GET",
        &format!("{}/ui-badjson/x", root()),
        200,
        "not-json",
    )]));
    let mut app = start_with_network(Root::of::<FirestoreApp>(), &network);
    app.wait_for("text:ready").unwrap();
    app.press("id:bad-json").unwrap();
    let bad_json = line_of(&app, "bad-json: ");
    assert_eq!(
        bad_json.as_str(),
        "bad-json: firestore/invalid-response: the server sent a body that is not JSON"
    );
}
