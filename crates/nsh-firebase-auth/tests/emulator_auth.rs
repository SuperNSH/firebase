//! SANDBOX: needs the emulator suite (scripts/test-emulator.sh starts it).
//! Signs up demo@demo-nsh.test on the demo-nsh auth emulator, signs in,
//! refreshes over Secure Token, and reads the user back through a fresh
//! Auth's restore. Live network to 127.0.0.1:19099 only; never a real
//! Firebase account. Run with `cargo test -p nsh-firebase-auth --test
//! emulator_auth -- --ignored`, or by scripts/test-emulator.sh.

use std::sync::Arc;
use std::time::Duration;

use nsh_core::element::IntoElement;
use nsh_core::prelude::*;
use nsh_engine::{EngineConfig, Root};
use nsh_firebase_auth::{Auth, AuthUser};
use nsh_firebase_core::{FirebaseOptions, initialize_app};
use nsh_testing::{Headless, UreqNetwork};

const EMAIL: &str = "demo@demo-nsh.test";
const PASSWORD: &str = "fake-password-1";

fn sandbox_user_line(user: Option<&AuthUser>) -> String {
    match user {
        Some(user) => format!(
            "user: {} email={} token_len={} refresh_len={}",
            user.uid,
            user.email.as_deref().unwrap_or("none"),
            user.id_token.len(),
            user.refresh_token.len(),
        ),
        None => "user: none".to_owned(),
    }
}

#[component]
fn SandboxApp() -> impl IntoElement {
    let app = initialize_app(FirebaseOptions::demo(), "auth-sandbox").unwrap();
    // The app's own storage, what a launch persists into: the restore
    // step at the end reads it back through a fresh Auth.
    let auth = Arc::new(Auth::new(app, Arc::new(nsh_firebase_auth::EngineStore)));
    let line = signal("idle".to_owned());
    let restored = signal("restored: never".to_owned());

    let sign_up = {
        let (auth, line) = (Arc::clone(&auth), line);
        move || {
            let (auth, line) = (Arc::clone(&auth), line);
            spawn(async move {
                match auth
                    .create_user_with_email_and_password(EMAIL, PASSWORD)
                    .await
                {
                    Ok(user) => line.set(format!("signup: ok {}", user.uid)),
                    Err(error) if error.code == "auth/email-already-in-use" => {
                        line.set("signup: exists".to_owned());
                    }
                    Err(error) => line.set(format!("signup: {error}")),
                }
            });
        }
    };
    let sign_in = {
        let (auth, line) = (Arc::clone(&auth), line);
        move || {
            let (auth, line) = (Arc::clone(&auth), line);
            spawn(async move {
                match auth.sign_in_with_email_and_password(EMAIL, PASSWORD).await {
                    Ok(user) => line.set(format!("signin: ok {}", user.uid)),
                    Err(error) => line.set(format!("signin: {error}")),
                }
            });
        }
    };
    let refresh = {
        let (auth, line) = (Arc::clone(&auth), line);
        move || {
            let (auth, line) = (Arc::clone(&auth), line);
            spawn(async move {
                match auth.refresh_now().await {
                    // The emulator hands the same id token back until it
                    // nears expiry; the honest check is the fresh expiry.
                    Ok(user) => line.set(format!(
                        "refresh: ok about_an_hour_out={}",
                        user.expires_at_ms > nsh_core::time::unix_ms().max(0.0) as u64 + 3_000_000,
                    )),
                    Err(error) => line.set(format!("refresh: {error}")),
                }
            });
        }
    };
    // A token no one minted: the emulator must refuse it with the wire
    // code the mapping turns into auth/invalid-custom-token.
    let custom = {
        let (auth, line) = (Arc::clone(&auth), line);
        move || {
            let (auth, line) = (Arc::clone(&auth), line);
            spawn(async move {
                match auth.sign_in_with_custom_token("custom-token-abc").await {
                    Ok(user) => line.set(format!("custom: ok {}", user.uid)),
                    Err(error) => line.set(format!("custom: {error}")),
                }
            });
        }
    };

    // Restore over the real store: a fresh Auth reads the app's storage,
    // what the next launch does.
    let reauth = move || {
        let app = initialize_app(FirebaseOptions::demo(), "auth-sandbox").unwrap();
        let auth = Auth::new(app, Arc::new(nsh_firebase_auth::EngineStore));
        restored.set(format!(
            "restored: {}",
            sandbox_user_line(auth.current_user().as_ref())
        ));
    };

    view()
        .flex_1()
        .gap_1()
        .child(text(move || {
            sandbox_user_line(auth.current_user().as_ref())
        }))
        .child(text(move || line.get()))
        .child(text(move || restored.get()))
        .child(
            view()
                .test_id("signup")
                .on_press(sign_up)
                .child(text("sign up")),
        )
        .child(
            view()
                .test_id("signin")
                .on_press(sign_in)
                .child(text("sign in")),
        )
        .child(
            view()
                .test_id("refresh")
                .on_press(refresh)
                .child(text("refresh")),
        )
        .child(
            view()
                .test_id("custom")
                .on_press(custom)
                .child(text("custom token")),
        )
        .child(
            view()
                .test_id("reauth")
                .on_press(reauth)
                .child(text("reauth")),
        )
}

#[ignore = "sandbox: needs emulator suite"]
#[test]
fn signs_up_signs_in_refreshes_and_restores_on_the_emulator() {
    let mut config = EngineConfig::new(Root::of::<SandboxApp>());
    config.network = Some(Arc::new(UreqNetwork::new()));
    let mut app = Headless::start(config).unwrap();
    app.first_render().unwrap();

    // A fresh suite signs up; a running one has the account, so the flow
    // falls through to sign-in. Either path ends with an account to use.
    app.press("id:signup").unwrap();
    app.wait_for_within("text:/signup: (ok|exists)/", Duration::from_secs(30))
        .unwrap();
    app.press("id:signin").unwrap();
    app.wait_for_within("text:/signin: ok .+/", Duration::from_secs(30))
        .unwrap();
    // The currentUser signal carries the emulator's session.
    app.wait_for_within(
        r#"text:/user: .+ email=demo@demo-nsh\.test token_len=[1-9]/"#,
        Duration::from_secs(5),
    )
    .unwrap();

    // Refresh now, ahead of the schedule: Secure Token answers a fresh
    // id token (rotated) with an expiry about an hour out.
    app.press("id:refresh").unwrap();
    app.wait_for_within(
        "text:refresh: ok about_an_hour_out=true",
        Duration::from_secs(30),
    )
    .unwrap();

    // A token no one minted: the refusal surfaces live with the stable
    // code, not an opaque HTTP error.
    app.press("id:custom").unwrap();
    app.wait_for_within(
        "text:/custom: auth\\/invalid-custom-token/",
        Duration::from_secs(30),
    )
    .unwrap();

    // Restore over the real store: a fresh Auth over the app's
    // nsh/storage reads the session the sign-in persisted.
    app.press("id:reauth").unwrap();
    app.wait_for_within(
        r#"text:/restored: user: .+ email=demo@demo-nsh\.test token_len=[1-9]/"#,
        Duration::from_secs(5),
    )
    .unwrap();
}
