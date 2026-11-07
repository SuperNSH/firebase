//! Labelled sandbox test against the emulator suite's Storage on
//! 127.0.0.1:19199 (`demo-nsh`, no account, no real Google endpoint).
//! The repo's runner runs it (`bash scripts/test-emulator.sh` ->
//! `cargo test -p nsh-firebase-storage --test emulator_storage -- --ignored`);
//! `cargo test` skips it by default.

use std::{
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, SystemTime},
};

use nsh_core::prelude::*;
use nsh_engine::{Clock, EngineConfig, Root};
use nsh_firebase_core::{FirebaseError, FirebaseOptions, initialize_app};
use nsh_firebase_storage::{FirebaseStorage, SettableMetadata, UploadState};
use nsh_testing::{Headless, UreqNetwork};

/// The object path this run owns, so re-runs start clean.
const PATH: &str = "sandbox/storage-crate/fixture.txt";
const FIXTURE: &[u8] =
    b"nsh-firebase storage sandbox fixture: the bytes a real emulator round-trips.";

/// The progress transitions, appended by the component's poll loop.
fn progress_log() -> &'static Mutex<Vec<String>> {
    static LOG: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    LOG.get_or_init(|| Mutex::new(Vec::new()))
}

fn clock() -> Clock {
    // A manual clock: the run moves only as the test moves it.
    Clock::manual(SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_258_000))
}

#[component]
fn SandboxApp() -> impl IntoElement {
    let line = signal(String::from("sandbox running"));
    let line_for_run = line;
    view()
        .flex_1()
        .gap_1()
        .child(text(move || line.get()))
        .child(
            view()
                .test_id("go")
                .on_press(move || {
                    let line = line_for_run;
                    let app = initialize_app(FirebaseOptions::demo(), "storage-sandbox").unwrap();
                    let storage = FirebaseStorage::new(&app).unwrap();
                    spawn(async move {
                        match round_trip(&storage).await {
                            Ok(summary) => line.set(summary),
                            Err(error) => line.set(format!("sandbox err {error}")),
                        }
                    });
                })
                .child(text("go")),
        )
}

/// Upload the fixture (with progress), read its download URL, download
/// the bytes back, list, delete.
async fn round_trip(storage: &FirebaseStorage) -> Result<String, FirebaseError> {
    let reference = storage.r#ref(PATH);
    let task = {
        let metadata = SettableMetadata {
            content_type: Some("text/plain".to_owned()),
            ..SettableMetadata::default()
        };
        reference.upload(FIXTURE.to_vec(), Some(metadata))
    };
    loop {
        let snapshot = task.progress().get();
        progress_log().lock().unwrap().push(format!(
            "{}/{} {}",
            snapshot.bytes_transferred,
            snapshot.total_bytes,
            snapshot.state.as_str()
        ));
        if snapshot.state == UploadState::Done || snapshot.state == UploadState::Error {
            break;
        }
        timer(Duration::from_millis(16)).await;
    }
    let uploaded = task.finished().await?;
    let url = reference.download_url().await?;
    let bytes = reference.download().await?;
    if bytes != FIXTURE {
        return Err(FirebaseError::new(
            "storage/unknown",
            "the downloaded bytes differ from the fixture",
        ));
    }
    let listed = storage
        .r#ref("sandbox/storage-crate")
        .list(Some(10), None)
        .await?;
    let was_listed = listed.items.iter().any(|item| item == PATH);
    reference.delete().await?;
    // The object is gone: the download now fails as object-not-found.
    let gone = reference.download().await;
    Ok(format!(
        "sandbox ok size={} url-token={} listed={} deleted-gone={}",
        uploaded.size.unwrap_or(0),
        url.contains("token="),
        was_listed,
        gone.err().map(|error| error.code).unwrap_or_default(),
    ))
}

/// The whole round trip against the running emulator: upload with
/// progress events, download URL, the bytes back, list, delete.
#[test]
#[ignore = "sandbox: needs emulator suite"]
fn upload_download_url_round_trip_and_delete() {
    progress_log().lock().unwrap().clear();
    let mut config = EngineConfig::new(Root::of::<SandboxApp>());
    config.clock = clock();
    // The engine's live network: this run may only reach the local
    // emulator (the URLs the app builds all name 127.0.0.1:19199).
    config.network = Some(Arc::new(UreqNetwork::new()));
    let mut app = Headless::start(config).unwrap();
    app.wait_for("text:sandbox running").unwrap();
    app.press("id:go").unwrap();
    // The responses come in real time from the emulator: the wait moves
    // the manual clock and pumps for what comes from outside the app.
    app.wait_until("the round trip ends", Duration::from_secs(30), |screen| {
        screen
            .texts()
            .iter()
            .any(|text| text.starts_with("sandbox ok") || text.starts_with("sandbox err"))
    })
    .unwrap();

    let summary = app
        .screen()
        .texts()
        .into_iter()
        .find(|text| text.starts_with("sandbox ok") || text.starts_with("sandbox err"))
        .unwrap()
        .to_owned();
    assert!(
        summary.starts_with("sandbox ok"),
        "the round trip failed: {summary}"
    );
    assert!(summary.contains("url-token=true"), "{summary}");
    assert!(summary.contains("listed=true"), "{summary}");
    assert!(
        summary.contains("deleted-gone=storage/object-not-found"),
        "{summary}"
    );
    let progress = progress_log().lock().unwrap();
    assert!(
        progress.iter().any(|entry| entry.ends_with(" running")),
        "progress events were expected, got {progress:?}"
    );
    assert!(
        progress.contains(&format!("{}/{} done", FIXTURE.len(), FIXTURE.len())),
        "the last progress event names every byte, got {progress:?}"
    );
}
