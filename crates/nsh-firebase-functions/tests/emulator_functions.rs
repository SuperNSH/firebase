//! Labelled sandbox test against the emulator suite's Functions on
//! 127.0.0.1:15001 (`demo-nsh`, no account, no real Google endpoint; the
//! v1 callables `ping` and `add` of emulator/functions/index.js). The
//! repo's runner runs it (`bash scripts/test-emulator.sh` ->
//! `cargo test -p nsh-firebase-functions --test emulator_functions --
//! --ignored`); `cargo test` skips it by default.

use std::{
    sync::{Arc, OnceLock},
    time::{Duration, SystemTime},
};

use nsh_core::prelude::*;
use nsh_engine::{Clock, EngineConfig, Root};
use nsh_firebase_core::{FirebaseOptions, initialize_app};
use nsh_firebase_functions::{Functions, FunctionsError};
use nsh_testing::{Headless, UreqNetwork};

/// The summary the run leaves on screen; the test asserts it exactly, so
/// a wrong wire shape cannot pass.
const EXPECTED: &str =
    "sandbox ok ping-pong=true ping-sent-from=sandbox add-sum=5 missing=functions/not-found";

fn clock() -> Clock {
    // A manual clock: the run moves only as the test moves it.
    Clock::manual(SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_258_000))
}

fn summary_line() -> &'static OnceLock<String> {
    static LINE: OnceLock<String> = OnceLock::new();
    &LINE
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
                    let app = initialize_app(FirebaseOptions::demo(), "functions-sandbox").unwrap();
                    let functions = Functions::new(&app);
                    spawn(async move {
                        let summary = match round_trip(&functions).await {
                            Ok(summary) => summary,
                            Err(error) => format!("sandbox err {error}"),
                        };
                        let _ = summary_line().set(summary.clone());
                        line.set(summary);
                    });
                })
                .child(text("go")),
        )
}

/// Call `ping` with data, `add` with two numbers, and a function the
/// emulator does not serve.
async fn round_trip(functions: &Functions) -> Result<String, FunctionsError> {
    let ping = functions
        .call("ping", serde_json::json!({ "from": "sandbox" }))
        .await?;
    let pong = ping.get("pong").and_then(|pong| pong.as_bool());
    let sent_from = ping
        .get("sent")
        .and_then(|sent| sent.get("from"))
        .and_then(|from| from.as_str());
    if pong != Some(true) || sent_from != Some("sandbox") {
        return Err(FunctionsError::new(
            "functions/unknown",
            format!("ping echoed a wrong shape: {ping}"),
        ));
    }

    let add = functions
        .call("add", serde_json::json!({ "a": 2, "b": 3 }))
        .await?;
    let sum = add.get("sum").and_then(|sum| sum.as_f64());
    if sum != Some(5.0) {
        return Err(FunctionsError::new(
            "functions/unknown",
            format!("add returned a wrong shape: {add}"),
        ));
    }

    let missing = functions.call("missing", serde_json::Value::Null).await;
    let missing_code = missing.err().map(|error| error.code).unwrap_or_default();

    Ok(format!(
        "sandbox ok ping-pong={} ping-sent-from={} add-sum={} missing={}",
        pong.unwrap_or_default(),
        sent_from.unwrap_or_default(),
        sum.unwrap_or_default(),
        missing_code,
    ))
}

/// The whole round trip against the running emulator: an echo with data,
/// an add, and the missing function's not-found error.
#[test]
#[ignore = "sandbox: needs emulator suite"]
fn ping_add_and_missing_function_over_the_live_emulator() {
    let mut config = EngineConfig::new(Root::of::<SandboxApp>());
    config.clock = clock();
    // The engine's live network: this run may only reach the local
    // emulator (the URLs the app builds all name 127.0.0.1:15001).
    config.network = Some(Arc::new(UreqNetwork::new()));
    let mut app = Headless::start(config).unwrap();
    app.wait_for("text:sandbox running").unwrap();
    app.press("id:go").unwrap();
    // The functions runtime spawns a node process on the first call; the
    // default 5 s wait is not enough for a cold emulator.
    app.wait_for_within(&format!("text:{EXPECTED}"), Duration::from_secs(120))
        .unwrap();
    assert_eq!(summary_line().get().map(String::as_str), Some(EXPECTED));
}
