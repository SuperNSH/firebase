//! The labelled sandbox test against the Firestore emulator on
//! 127.0.0.1:18080 (project `demo-nsh`, started by
//! `bash scripts/start-emulator.sh`, run by
//! `bash scripts/test-emulator.sh`): set a doc, get it back, addDoc,
//! runQuery with a where filter, deleteDoc, and getDoc answering
//! exists=false. This file talks to the network for real; nothing in it
//! runs under the headless unit gates.

use nsh_core::{JsonValue, prelude::*, time::unix_ms};
use nsh_engine::{EngineConfig, Root, net::network};
use nsh_firebase_core::{FirebaseOptions, initialize_app};
use nsh_firebase_firestore::{DocSnapshot, Firestore, FirestoreQuery, WhereFilter, WhereOp};
use nsh_testing::Headless;
use serde_json::json;

fn show(snapshot: &DocSnapshot) -> String {
    let data = snapshot
        .data
        .as_ref()
        .map(JsonValue::to_string)
        .unwrap_or_else(|| "none".to_owned());
    format!("exists={} data={data}", snapshot.exists)
}

#[component]
fn SandboxApp() -> impl IntoElement {
    let app = initialize_app(FirebaseOptions::demo(), "sandbox-fs").unwrap();
    let fs = Firestore::new(&app);
    // "set: ok" only lands after the write's read back exists, so the
    // lines below are one step each.
    let set_line = signal("set: idle".to_owned());
    let add_line = signal("add: idle".to_owned());
    let query_line = signal("query: idle".to_owned());
    let delete_line = signal("delete: idle".to_owned());
    let reget_line = signal("reget: idle".to_owned());
    view()
        .flex_1()
        .gap_1()
        .child(text(move || set_line.get()))
        .child(text(move || add_line.get()))
        .child(text(move || query_line.get()))
        .child(text(move || delete_line.get()))
        .child(text(move || reget_line.get()))
        .child(
            view()
                .test_id("run")
                .on_press(move || {
                    let fs = fs.clone();
                    spawn(async move {
                        // A marker unique to this run: the query's where
                        // filter matches exactly this run's two docs,
                        // whatever earlier runs left behind.
                        let marker = format!("rust-{}", unix_ms() as i64);
                        let write = async {
                            fs.set_doc("sandbox-fs/one", &json!({ "kind": marker, "n": 1 }), false)
                                .await?;
                            fs.get_doc("sandbox-fs/one").await
                        };
                        match write.await {
                            Ok(snapshot) if snapshot.exists => set_line.set("set: ok".to_owned()),
                            Ok(snapshot) => {
                                set_line.set(format!("set: read back {}", show(&snapshot)))
                            }
                            Err(error) => set_line.set(format!("set: {error}")),
                        }
                        match fs
                            .add_doc("sandbox-fs", &json!({ "kind": marker, "n": 2 }))
                            .await
                        {
                            Ok(_) => add_line.set("add: ok".to_owned()),
                            Err(error) => add_line.set(format!("add: {error}")),
                        }
                        match fs
                            .run_query(&FirestoreQuery {
                                collection_path: "sandbox-fs".to_owned(),
                                r#where: Some(WhereFilter {
                                    field: "kind".to_owned(),
                                    op: WhereOp::Equal,
                                    value: json!(marker),
                                }),
                                limit: Some(5),
                                ..FirestoreQuery::default()
                            })
                            .await
                        {
                            Ok(docs) => query_line.set(format!("query: {} docs", docs.len())),
                            Err(error) => query_line.set(format!("query: {error}")),
                        }
                        match fs.delete_doc("sandbox-fs/one").await {
                            Ok(()) => delete_line.set("delete: ok".to_owned()),
                            Err(error) => delete_line.set(format!("delete: {error}")),
                        }
                        match fs.get_doc("sandbox-fs/one").await {
                            Ok(snapshot) if !snapshot.exists => {
                                reget_line.set("reget: exists=false".to_owned())
                            }
                            Ok(snapshot) => {
                                reget_line.set(format!("reget: still {}", show(&snapshot)))
                            }
                            Err(error) => reget_line.set(format!("reget: {error}")),
                        }
                    });
                })
                .child(text("run")),
        )
}

/// The app's engine: the real network stack behind `fetch` (the REST
/// calls) and the system's clock (a sandbox run keeps real time: the
/// marker needs it).
fn config_of(root: Root) -> EngineConfig {
    let mut config = EngineConfig::new(root);
    config.network = Some(network());
    config
}

#[ignore = "sandbox: needs emulator suite"]
#[test]
fn writes_reads_queries_and_deletes_against_the_emulator() {
    let mut app = Headless::start(config_of(Root::of::<SandboxApp>())).unwrap();
    app.wait_for("text:set: idle").unwrap();
    app.press("id:run").unwrap();
    app.wait_for("text:set: ok").unwrap();
    app.wait_for("text:add: ok").unwrap();
    // The marker's uniqueness makes the count exact.
    app.wait_for("text:query: 2 docs").unwrap();
    app.wait_for("text:delete: ok").unwrap();
    app.wait_for("text:reget: exists=false").unwrap();
}
