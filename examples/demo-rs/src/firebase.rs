// The demo's Firebase clients over core's demo options (demo-nsh on
// 127.0.0.1, every value fake), one instance per product at the module
// level, so hot updates keep them.

use std::sync::Arc;

use nsh_core::{platform::HttpRequest, prelude::*};
use nsh_firebase_analytics::{Analytics, AnalyticsConfig};
use nsh_firebase_auth::{Auth, EngineStore};
use nsh_firebase_core::{FirebaseApp, FirebaseOptions, PureHost, initialize_app};
use nsh_firebase_database::{Database, StreamSink, StreamTask, StreamTransport};
use nsh_firebase_messaging::{Messaging, NEEDS_NATIVE_TIER};
use nsh_firebase_storage::FirebaseStorage;

const COLLECTOR: &str = "http://127.0.0.1:18742";

pub fn demo_app() -> Arc<FirebaseApp> {
    initialize_app(FirebaseOptions::demo(), "[DEFAULT]").expect("the demo options are valid")
}

pub fn demo_auth() -> Arc<Auth> {
    Arc::new(Auth::new(demo_app(), Arc::new(EngineStore)))
}

pub fn demo_database() -> Arc<Database> {
    Database::from_app(&demo_app(), Arc::new(WholeBodyStream))
        .expect("the demo options have a database")
}

pub fn demo_storage() -> FirebaseStorage {
    FirebaseStorage::new(&demo_app()).expect("the demo options have a bucket")
}

pub fn demo_messaging() -> Messaging {
    Messaging::new(Arc::new(PureHost))
}

pub fn demo_analytics() -> Analytics {
    Analytics::with_collector(
        &demo_app(),
        COLLECTOR,
        AnalyticsConfig {
            max_batch_size: 3,
            ..AnalyticsConfig::default()
        },
    )
    .expect("the demo analytics config is valid")
}

/// The messaging pure tier's refusal, word for word.
pub fn messaging_needs_native() -> &'static str {
    NEEDS_NATIVE_TIER
}

// nsh 0.4.0 gives a Rust app no incremental fetch (the whole body
// arrives at once), so the demo's stream transport reads the body whole
// and delivers it as one chunk: a stubbed or ended stream resolves, a
// live one that never ends would not. An embedding host replaces this
// with its own stream transport.
struct WholeBodyStream;

impl StreamTransport for WholeBodyStream {
    fn open(&self, request: HttpRequest, sink: Arc<dyn StreamSink>) -> Box<dyn StreamTask> {
        spawn(async move {
            match fetch(request).await {
                Ok(response) => {
                    sink.head(response.status());
                    let body = response.text();
                    if !body.is_empty() {
                        sink.data(body.into_bytes());
                    }
                    sink.end();
                }
                Err(error) => sink.failed(error.to_string()),
            }
        });
        Box::new(NoTask)
    }
}

struct NoTask;

impl StreamTask for NoTask {
    fn cancel(self: Box<Self>) {}
}
