//! Headless UI tests of storage: the resumable upload's wire traffic and
//! progress (an interrupted-then-resumed run, pause, cancel), download
//! with the Firebase auth header, download URLs, metadata, list paging
//! and delete — all through a test-only component on a manual clock with
//! the network stubbed, no real time and no real server.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use nsh_core::{
    platform::{HttpBody, HttpRequest},
    prelude::*,
};
use nsh_engine::{Clock, EngineConfig, Root};
use nsh_firebase_core::{FirebaseOptions, initialize_app};
use nsh_firebase_storage::{FirebaseStorage, SettableMetadata, UploadTask};
use nsh_testing::{Headless, StubHandler, StubNetwork, StubReply, StubResponse, StubRoute, WAIT};

/// `StubResponse` is `#[non_exhaustive]`: fields are set on the default.
fn stub(status: u16, headers: Vec<(&str, String)>, body: Vec<u8>, delay: Duration) -> StubResponse {
    let mut response = StubResponse::default();
    response.status = status;
    response.headers = headers
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
    response.body = body;
    response.delay = delay;
    response
}

/// A failed request: no response at all (offline, a reset connection).
fn stub_error(message: &str) -> StubResponse {
    let mut response = StubResponse::default();
    response.error = Some(message.to_owned());
    response
}

/// The session URI the start stub hands out; also what `resume` saves.
const SESSION: &str = "http://127.0.0.1:19199/upload/session-1";
/// The start URL the client builds for the upload path.
const START_URL: &str =
    "http://127.0.0.1:19199/v0/b/demo-nsh.firebasestorage.app/o?name=demo%2Fbig.bin";
const PATH: &str = "demo/big.bin";
/// 600_000 bytes: two full 256 KiB slices and a final 75_712 one.
const PAYLOAD_LEN: usize = 600_000;
const OBJECT_BASE: &str = "http://127.0.0.1:19199/v0/b/demo-nsh.firebasestorage.app";

/// What the fake server recorded of the session so far: the bytes it
/// holds, each accepted slice, the fault injection and slice delay.
#[derive(Default)]
struct UploadProbe {
    received: Mutex<u64>,
    uploads: Mutex<Vec<(String, usize)>>,
    fail_offset: Mutex<Option<u64>>,
    delay: Mutex<Duration>,
    deny_finalize: Mutex<bool>,
}

fn header_of(request: &HttpRequest, name: &str) -> Option<String> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn body_len(request: &HttpRequest) -> usize {
    match &request.body {
        HttpBody::Bytes(bytes) => bytes.len(),
        _ => 0,
    }
}

/// The session route: a `query` reports what the server holds, slices
/// record themselves and answer active (final with metadata on the last).
fn session_route(probe: Arc<UploadProbe>) -> StubRoute {
    let handler: StubHandler = Arc::new(move |request: HttpRequest, reply: StubReply| {
        let command = header_of(&request, "X-Goog-Upload-Command").unwrap_or_default();
        match command.as_str() {
            "query" => reply.send(stub(
                200,
                vec![
                    ("X-Goog-Upload-Status", "active".to_owned()),
                    (
                        "X-Goog-Upload-Size-Received",
                        probe.received.lock().unwrap().to_string(),
                    ),
                ],
                Vec::new(),
                Duration::ZERO,
            )),
            "upload" | "upload, finalize" => {
                let offset: u64 = header_of(&request, "X-Goog-Upload-Offset")
                    .and_then(|offset| offset.parse().ok())
                    .unwrap_or(0);
                let fails = {
                    let mut fail_offset = probe.fail_offset.lock().unwrap();
                    if *fail_offset == Some(offset) {
                        *fail_offset = None;
                        true
                    } else {
                        false
                    }
                };
                if fails {
                    reply.send(stub_error("connection reset"));
                    return;
                }
                // A denied finalize answers 403 with an
                // `x-goog-upload-status: final` header: read the status first.
                if command == "upload, finalize" && *probe.deny_finalize.lock().unwrap() {
                    reply.send(stub(
                        403,
                        vec![("X-Goog-Upload-Status", "final".to_owned())],
                        br#"{"error":{"code":403,"message":"Permission denied. No WRITE permission."}}"#
                            .to_vec(),
                        Duration::ZERO,
                    ));
                    return;
                }
                *probe.received.lock().unwrap() += body_len(&request) as u64;
                probe
                    .uploads
                    .lock()
                    .unwrap()
                    .push((command.clone(), body_len(&request)));
                let finalized = command == "upload, finalize";
                let delay = if finalized {
                    Duration::ZERO
                } else {
                    *probe.delay.lock().unwrap()
                };
                reply.send(stub(
                    200,
                    vec![(
                        "X-Goog-Upload-Status",
                        if finalized { "final" } else { "active" }.to_owned(),
                    )],
                    if finalized {
                        metadata_document().into_bytes()
                    } else {
                        Default::default()
                    },
                    delay,
                ));
            }
            _ => reply.send(stub(400, Vec::new(), Vec::new(), Duration::ZERO)),
        }
    });
    StubRoute::Handler(handler)
}

/// The object metadata a finalize hands back.
fn metadata_document() -> String {
    r#"{"name":"demo/big.bin","bucket":"demo-nsh.firebasestorage.app","generation":"1","metageneration":"1","size":"600000","contentType":"application/octet-stream","downloadTokens":"tok-1"}"#.to_owned()
}

fn upload_routes(probe: Arc<UploadProbe>) -> Vec<(String, StubRoute)> {
    let start = stub(
        200,
        vec![
            ("X-Goog-Upload-Status", "active".to_owned()),
            ("X-Goog-Upload-URL", SESSION.to_owned()),
        ],
        Vec::new(),
        Duration::ZERO,
    );
    vec![
        (format!("POST {START_URL}"), StubRoute::from(start)),
        (format!("POST {SESSION}"), session_route(probe)),
    ]
}

fn demo_clock() -> Clock {
    Clock::manual(SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_258_000))
}

fn start_app(routes: Vec<(String, StubRoute)>) -> (Headless, Arc<StubNetwork>) {
    let clock = demo_clock();
    let network = Arc::new(StubNetwork::new(routes).with_clock(clock.clone()));
    let mut config = EngineConfig::new(Root::of::<StorageApp>());
    config.clock = clock;
    config.network = Some(network.clone());
    let mut app = Headless::start(config).unwrap();
    app.wait_for("text:storage ready").unwrap();
    (app, network)
}

/// Waits until a text on screen ends with `suffix` (the log line grows).
fn wait_text(app: &mut Headless, suffix: &str) {
    app.wait_until(&format!("text ending `{suffix}`"), WAIT, |screen| {
        screen.texts().iter().any(|text| text.ends_with(suffix))
    })
    .unwrap();
}

#[component]
fn StorageApp() -> impl IntoElement {
    let line = signal(String::from("storage ready"));
    let progress_log = signal(String::from("idle"));
    let task = signal(None::<UploadTask>);

    // Re-runs when the task is replaced or its progress changes; appends
    // every transition it sees.
    effect(move || match task.get() {
        None => progress_log.set(String::from("idle")),
        Some(task) => {
            let snapshot = task.progress().get();
            progress_log.update(|text| {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(&format!(
                    "{}/{} {}",
                    snapshot.bytes_transferred,
                    snapshot.total_bytes,
                    snapshot.state.as_str()
                ));
            });
        }
    });

    let (task_for_upload, line_for_upload) = (task, line);
    let (task_for_resume, line_for_resume) = (task, line);
    let (task_for_pause, task_for_cancel) = (task, task);
    let saved_session: Stored<Option<String>> = stored(None);
    let (task_for_save, line_for_save, session_for_save) = (task, line, saved_session);
    let session_for_resume = saved_session;
    view()
        .flex_1()
        .gap_1()
        .child(text(move || progress_log.get()))
        .child(text(move || line.get()))
        .child(
            view()
                .test_id("upload")
                .on_press(move || {
                    upload_now(&task_for_upload, line_for_upload, None);
                })
                .child(text("upload")),
        )
        .child(
            view()
                .test_id("save")
                .on_press(move || {
                    // What resume_upload needs saved across a process
                    // death: the session URI the server issued.
                    let session = task_for_save.get().and_then(|task| task.session_url());
                    line_for_save.set(format!(
                        "st: session {}",
                        session.clone().unwrap_or_default()
                    ));
                    session_for_save.replace(session);
                })
                .child(text("save session")),
        )
        .child(
            view()
                .test_id("resume")
                .on_press(move || {
                    upload_now(
                        &task_for_resume,
                        line_for_resume,
                        session_for_resume.with(|s| s.clone()),
                    );
                })
                .child(text("resume id")),
        )
        .child(
            view()
                .test_id("pause")
                .on_press(move || {
                    if let Some(task) = task_for_pause.get() {
                        task.pause();
                    }
                })
                .child(text("pause")),
        )
        .child(
            view()
                .test_id("cancel")
                .on_press(move || {
                    if let Some(task) = task_for_cancel.get() {
                        task.cancel();
                    }
                })
                .child(text("cancel")),
        )
}

fn upload_now(task: &Signal<Option<UploadTask>>, line: Signal<String>, session: Option<String>) {
    let app = initialize_app(FirebaseOptions::demo(), "storage-ui").unwrap();
    let storage = FirebaseStorage::new(&app).unwrap();
    let reference = storage.r#ref(PATH);
    let new_task = match session {
        Some(session) => reference.resume_upload(vec![0xAB; PAYLOAD_LEN], session),
        None => reference.upload(vec![0xAB; PAYLOAD_LEN], None),
    };
    task.set(Some(new_task.clone()));
    spawn(async move {
        match new_task.finished().await {
            Ok(metadata) => line.set(format!(
                "finished ok {} size={}",
                metadata.full_path,
                metadata.size.unwrap_or(0)
            )),
            Err(error) => line.set(format!("finished err {}", error.code)),
        }
    });
}

/// The recorded start request must speak the exact protocol.
#[test]
fn upload_slices_and_reports_progress() {
    let probe = Arc::new(UploadProbe::default());
    let (mut app, network) = start_app(upload_routes(Arc::clone(&probe)));
    app.press("id:upload").unwrap();
    wait_text(&mut app, "600000/600000 done");
    wait_text(&mut app, "finished ok demo/big.bin size=600000");

    let requests = network.requests();
    assert_eq!(requests.len(), 5, "start, query, three slices");
    let start = &requests[0];
    assert_eq!(start.method, "POST");
    assert_eq!(start.url, START_URL);
    assert_eq!(
        header_of(start, "X-Goog-Upload-Protocol").as_deref(),
        Some("resumable")
    );
    assert_eq!(
        header_of(start, "X-Goog-Upload-Command").as_deref(),
        Some("start")
    );
    assert_eq!(
        header_of(start, "X-Goog-Upload-Header-Content-Length").as_deref(),
        Some("600000")
    );
    assert_eq!(
        header_of(start, "X-Goog-Upload-Header-Content-Type").as_deref(),
        Some("application/octet-stream")
    );
    assert_eq!(
        header_of(start, "Content-Type").as_deref(),
        Some("application/json; charset=utf-8")
    );
    let start_body: serde_json::Value = match &start.body {
        HttpBody::Bytes(bytes) => serde_json::from_slice(bytes).unwrap(),
        _ => serde_json::Value::Null,
    };
    assert_eq!(
        start_body,
        serde_json::json!({
            "name": "demo/big.bin",
            "contentType": "application/octet-stream",
        })
    );
    assert_eq!(
        header_of(&requests[1], "X-Goog-Upload-Command").as_deref(),
        Some("query")
    );
    let expected = [
        ("upload", 256 * 1024),
        ("upload", 256 * 1024),
        ("upload, finalize", PAYLOAD_LEN - 2 * 256 * 1024),
    ];
    let uploads = probe.uploads.lock().unwrap();
    for ((command, length), (want_command, want_length)) in uploads.iter().zip(expected.iter()) {
        assert_eq!(command, want_command);
        assert_eq!(length, want_length);
    }
    assert_eq!(uploads.len(), 3);
    let recorded_offsets: Vec<u64> = requests[2..]
        .iter()
        .map(|request| {
            header_of(request, "X-Goog-Upload-Offset")
                .and_then(|offset| offset.parse().ok())
                .unwrap()
        })
        .collect();
    assert_eq!(
        recorded_offsets,
        vec![0, 256 * 1024, 2 * 256 * 1024],
        "slices go out in order from the query's offset"
    );
    assert_eq!(*probe.received.lock().unwrap(), PAYLOAD_LEN as u64);
}

/// A slice that dies mid-run leaves the session on the server; resuming
/// by the saved id queries the offset and uploads only the rest.
#[test]
fn an_interrupted_upload_resumes_from_the_saved_session() {
    let probe = Arc::new(UploadProbe {
        fail_offset: Mutex::new(Some(256 * 1024)),
        ..UploadProbe::default()
    });
    let (mut app, network) = start_app(upload_routes(Arc::clone(&probe)));
    app.press("id:upload").unwrap();
    wait_text(&mut app, "262144/600000 error");
    wait_text(&mut app, "finished err core/network");
    assert_eq!(*probe.received.lock().unwrap(), 256 * 1024);

    // The slice landed server-side even though its answer was lost:
    // save the session URI, then resume by it, as a restart would.
    *probe.fail_offset.lock().unwrap() = None;
    app.press("id:save").unwrap();
    app.wait_for("text:st: session http://127.0.0.1:19199/upload/session-1")
        .unwrap();
    app.press("id:resume").unwrap();
    wait_text(&mut app, "600000/600000 done");
    wait_text(&mut app, "finished ok demo/big.bin size=600000");

    let requests = network.requests();
    // One start only; the resume opens with a query, not a session.
    assert_eq!(requests[0].url, START_URL);
    assert!(
        requests[1..].iter().all(|request| request.url != START_URL),
        "resume must not start a new session"
    );
    assert_eq!(
        header_of(&requests[4], "X-Goog-Upload-Command").as_deref(),
        Some("query"),
        "the resume's first request queries the session"
    );
    let resumed_offsets: Vec<u64> = requests[5..]
        .iter()
        .map(|request| {
            header_of(request, "X-Goog-Upload-Offset")
                .and_then(|offset| offset.parse().ok())
                .unwrap()
        })
        .collect();
    assert_eq!(
        resumed_offsets,
        vec![256 * 1024, 2 * 256 * 1024],
        "the retry slices continue from what the server holds"
    );
    let uploads = probe.uploads.lock().unwrap();
    assert_eq!(uploads.len(), 3);
    assert_eq!(uploads[2].0, "upload, finalize");
    assert_eq!(uploads[2].1, PAYLOAD_LEN - 2 * 256 * 1024);
}

/// Pause stops after the slice in flight; resume queries and finishes.
#[test]
fn pause_holds_and_resume_continues() {
    let probe = Arc::new(UploadProbe {
        delay: Mutex::new(Duration::from_secs(1)),
        ..UploadProbe::default()
    });
    let (mut app, _network) = start_app(upload_routes(Arc::clone(&probe)));
    app.press("id:upload").unwrap();
    // The first slice is in flight; pausing stops the loop at the next
    // boundary without it ever sending a second slice.
    app.press("id:pause").unwrap();
    wait_text(&mut app, "262144/600000 paused");
    // Nothing is in flight any more: the clock moving alone must not
    // advance the upload.
    app.run_for(Duration::from_secs(10)).unwrap();
    app.wait_until("the paused state holds", WAIT, |screen| {
        screen
            .texts()
            .iter()
            .any(|text| text.ends_with("262144/600000 paused"))
    })
    .unwrap();
    assert_eq!(*probe.received.lock().unwrap(), 256 * 1024);

    app.press("id:resume").unwrap();
    wait_text(&mut app, "600000/600000 done");
    wait_text(&mut app, "finished ok demo/big.bin size=600000");
    let uploads = probe.uploads.lock().unwrap();
    assert_eq!(
        uploads
            .iter()
            .map(|(command, _)| command.as_str())
            .collect::<Vec<_>>(),
        vec!["upload", "upload", "upload, finalize"]
    );
    assert_eq!(*probe.received.lock().unwrap(), PAYLOAD_LEN as u64);
}

/// Cancel ends the task in `error` with the canceled code.
#[test]
fn cancel_ends_in_error_with_the_canceled_code() {
    let probe = Arc::new(UploadProbe {
        delay: Mutex::new(Duration::from_secs(1)),
        ..UploadProbe::default()
    });
    let (mut app, _network) = start_app(upload_routes(Arc::clone(&probe)));
    app.press("id:upload").unwrap();
    wait_text(&mut app, "0/600000 running");
    app.press("id:cancel").unwrap();
    wait_text(&mut app, "error");
    wait_text(&mut app, "finished err storage/canceled");
    // The in-flight slice still landed, and nothing more was sent.
    app.run_for(Duration::from_secs(10)).unwrap();
    assert_eq!(*probe.received.lock().unwrap(), 256 * 1024);
    assert_eq!(probe.uploads.lock().unwrap().len(), 1);
}

#[component]
fn AuthStorageApp() -> impl IntoElement {
    let line = signal(String::from("auth ready"));
    let app = initialize_app(FirebaseOptions::demo(), "storage-auth").unwrap();
    let storage = FirebaseStorage::new(&app)
        .unwrap()
        .with_auth(|| Some("test-id-token".to_owned()));
    let download = {
        let storage = storage.clone();
        move || {
            let storage = storage.clone();
            spawn(async move {
                match storage.r#ref("demo/secret.txt").download().await {
                    Ok(bytes) => {
                        if bytes == b"secret-bytes" {
                            line.set(format!("auth download ok {}", bytes.len()));
                        } else {
                            line.set(String::from("auth download MISMATCH"));
                        }
                    }
                    Err(error) => line.set(format!("auth err {}", error.code)),
                }
            });
        }
    };
    view()
        .flex_1()
        .gap_1()
        .child(text(move || line.get()))
        .child(
            view()
                .test_id("auth-download")
                .on_press(download)
                .child(text("download secret")),
        )
}

/// A wired token provider puts `Authorization: Firebase <idToken>` on the
/// request, and the bytes come back whole.
#[test]
fn download_carries_the_firebase_auth_header() {
    let route = (
        format!("GET {OBJECT_BASE}/o/demo%2Fsecret.txt?alt=media"),
        StubRoute::from(stub(
            200,
            Vec::new(),
            b"secret-bytes".to_vec(),
            Duration::ZERO,
        )),
    );
    let network = Arc::new(StubNetwork::new([route]));
    let mut config = EngineConfig::new(Root::of::<AuthStorageApp>());
    config.clock = demo_clock();
    config.network = Some(network.clone());
    let mut app = Headless::start(config).unwrap();
    app.wait_for("text:auth ready").unwrap();
    app.press("id:auth-download").unwrap();
    wait_text(&mut app, "auth download ok 12");
    let requests = network.requests();
    assert_eq!(
        header_of(&requests[0], "Authorization").as_deref(),
        Some("Firebase test-id-token")
    );
}

#[component]
fn ReadsApp() -> impl IntoElement {
    let line = signal(String::from("reads ready"));
    let app = initialize_app(FirebaseOptions::demo(), "storage-reads").unwrap();
    let storage = FirebaseStorage::new(&app).unwrap();

    // Each read shows what came back, or the stable code of why not.
    let (url_line, url_storage) = (line, storage.clone());
    let (meta_line, meta_storage) = (line, storage.clone());
    let (patch_line, patch_storage) = (line, storage.clone());
    let (list_line, list_storage) = (line, storage.clone());
    let (delete_line, delete_storage) = (line, storage.clone());
    let (missing_line, missing_storage) = (line, storage.clone());
    view()
        .flex_1()
        .gap_1()
        .child(text(move || line.get()))
        .child(
            view()
                .test_id("url")
                .on_press(move || {
                    let (storage, line) = (url_storage.clone(), url_line);
                    spawn(async move {
                        match storage.r#ref("demo/file.txt").download_url().await {
                            Ok(url) => line.set(format!("url {url}")),
                            Err(error) => line.set(format!("err {}", error.code)),
                        }
                    });
                })
                .child(text("url")),
        )
        .child(
            view()
                .test_id("metadata")
                .on_press(move || {
                    let (storage, line) = (meta_storage.clone(), meta_line);
                    spawn(async move {
                        match storage.r#ref("demo/file.txt").get_metadata().await {
                            Ok(metadata) => line.set(format!(
                                "metadata ct={} size={}",
                                metadata.content_type.unwrap_or_default(),
                                metadata.size.unwrap_or(0)
                            )),
                            Err(error) => line.set(format!("err {}", error.code)),
                        }
                    });
                })
                .child(text("metadata")),
        )
        .child(
            view()
                .test_id("patch")
                .on_press(move || {
                    let (storage, line) = (patch_storage.clone(), patch_line);
                    let change = SettableMetadata {
                        content_type: Some("text/updated".to_owned()),
                        ..SettableMetadata::default()
                    };
                    spawn(async move {
                        match storage.r#ref("demo/file.txt").update_metadata(change).await {
                            Ok(metadata) => line.set(format!(
                                "patched ct={} gen={}",
                                metadata.content_type.unwrap_or_default(),
                                metadata.metageneration.unwrap_or_default()
                            )),
                            Err(error) => line.set(format!("err {}", error.code)),
                        }
                    });
                })
                .child(text("patch")),
        )
        .child(
            view()
                .test_id("list")
                .on_press(move || {
                    let (storage, line) = (list_storage.clone(), list_line);
                    spawn(async move {
                        let page = storage.r#ref("demo").list(Some(2), None).await;
                        match page {
                            Ok(first) => {
                                let second = storage
                                    .r#ref("demo")
                                    .list(Some(2), first.next_page_token.as_deref())
                                    .await;
                                match second {
                                    Ok(second) => line.set(format!(
                                        "page1={} next={} page2={}",
                                        first.items.len(),
                                        first.next_page_token.unwrap_or_default(),
                                        second.items.len()
                                    )),
                                    Err(error) => line.set(format!("err {}", error.code)),
                                }
                            }
                            Err(error) => line.set(format!("err {}", error.code)),
                        }
                    });
                })
                .child(text("list")),
        )
        .child(
            view()
                .test_id("delete")
                .on_press(move || {
                    let (storage, line) = (delete_storage.clone(), delete_line);
                    spawn(async move {
                        match storage.r#ref("demo/file.txt").delete().await {
                            Ok(()) => line.set(String::from("deleted ok")),
                            Err(error) => line.set(format!("err {}", error.code)),
                        }
                    });
                })
                .child(text("delete")),
        )
        .child(
            view()
                .test_id("missing")
                .on_press(move || {
                    let (storage, line) = (missing_storage.clone(), missing_line);
                    spawn(async move {
                        if let Err(error) = storage.r#ref("demo/gone.txt").get_metadata().await {
                            line.set(format!("err {}", error.code));
                        }
                    });
                })
                .child(text("missing")),
        )
}

fn start_reads(routes: Vec<(String, StubResponse)>) -> (Headless, Arc<StubNetwork>) {
    let network = Arc::new(StubNetwork::new(
        routes
            .into_iter()
            .map(|(url, response)| (url, StubRoute::from(response))),
    ));
    let mut config = EngineConfig::new(Root::of::<ReadsApp>());
    config.clock = demo_clock();
    config.network = Some(network.clone());
    let mut app = Headless::start(config).unwrap();
    app.wait_for("text:reads ready").unwrap();
    (app, network)
}

fn response(status: u16, body: &str) -> StubResponse {
    stub(status, Vec::new(), body.as_bytes().to_vec(), Duration::ZERO)
}

/// The download URL comes from the metadata's first token.
#[test]
fn download_url_builds_from_the_metadata_tokens() {
    let (mut app, _network) = start_reads(vec![(
        format!("GET {OBJECT_BASE}/o/demo%2Ffile.txt"),
        response(
            200,
            r#"{"name":"demo/file.txt","bucket":"demo-nsh.firebasestorage.app","downloadTokens":"tok-abc,tok-2","contentType":"text/plain"}"#,
        ),
    )]);
    app.press("id:url").unwrap();
    app.wait_for(
        r#"text:url http://127.0.0.1:19199/v0/b/demo-nsh.firebasestorage.app/o/demo%2Ffile.txt?alt=media&token=tok-abc"#,
    )
    .unwrap();
}

/// get_metadata parses the document; update_metadata PATCHes the
/// writable fields only and parses what comes back.
#[test]
fn metadata_round_trips() {
    let (mut app, network) = start_reads(vec![
        (
            format!("GET {OBJECT_BASE}/o/demo%2Ffile.txt"),
            response(
                200,
                r#"{"name":"demo/file.txt","bucket":"demo-nsh.firebasestorage.app","size":"11","contentType":"text/plain","metageneration":"1"}"#,
            ),
        ),
        (
            format!("PATCH {OBJECT_BASE}/o/demo%2Ffile.txt"),
            response(
                200,
                r#"{"name":"demo/file.txt","contentType":"text/updated","metageneration":"2"}"#,
            ),
        ),
    ]);
    app.press("id:metadata").unwrap();
    app.wait_for("text:metadata ct=text/plain size=11").unwrap();
    app.press("id:patch").unwrap();
    app.wait_for("text:patched ct=text/updated gen=2").unwrap();
    let requests = network.requests();
    let patch = requests
        .iter()
        .find(|request| request.method == "PATCH")
        .unwrap();
    let body = match &patch.body {
        HttpBody::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        _ => String::new(),
    };
    assert_eq!(body, r#"{"contentType":"text/updated"}"#);
}

/// list() walks pages by `nextPageToken`.
#[test]
fn list_pages_through_the_tokens() {
    let (mut app, _network) = start_reads(vec![
        (
            format!("GET {OBJECT_BASE}/o?prefix=demo%2F&delimiter=/&maxResults=2"),
            response(
                200,
                r#"{"prefixes":[],"items":[{"name":"demo/a.txt"},{"name":"demo/b.txt"}],"nextPageToken":"t2"}"#,
            ),
        ),
        (
            format!("GET {OBJECT_BASE}/o?prefix=demo%2F&delimiter=/&pageToken=t2&maxResults=2"),
            response(200, r#"{"prefixes":[],"items":[{"name":"demo/c.txt"}]}"#),
        ),
    ]);
    app.press("id:list").unwrap();
    app.wait_for("text:page1=2 next=t2 page2=1").unwrap();
}

/// delete() succeeds on 204; a missing object maps to its stable code.
#[test]
fn delete_succeeds_and_errors_map_to_stable_codes() {
    let (mut app, network) = start_reads(vec![
        (
            format!("DELETE {OBJECT_BASE}/o/demo%2Ffile.txt"),
            response(204, ""),
        ),
        (
            format!("GET {OBJECT_BASE}/o/demo%2Fgone.txt"),
            response(404, r#"{"error":{"code":404,"message":"No such object"}}"#),
        ),
    ]);
    app.press("id:delete").unwrap();
    app.wait_for("text:deleted ok").unwrap();
    app.press("id:missing").unwrap();
    app.wait_for("text:err storage/object-not-found").unwrap();
    let requests = network.requests();
    assert_eq!(requests[0].method, "DELETE");
    assert_eq!(requests[0].url, format!("{OBJECT_BASE}/o/demo%2Ffile.txt"));
}

/// A denied finalize (the emulator's 403, `x-goog-upload-status: final`
/// and all) is an error, never a completion with made-up metadata.
#[test]
fn a_denied_finalize_is_an_error_despite_its_final_header() {
    let probe = Arc::new(UploadProbe {
        deny_finalize: Mutex::new(true),
        ..UploadProbe::default()
    });
    let (mut app, _network) = start_app(upload_routes(Arc::clone(&probe)));
    app.press("id:upload").unwrap();
    wait_text(&mut app, "524288/600000 error");
    wait_text(&mut app, "finished err storage/unauthorized");
}
