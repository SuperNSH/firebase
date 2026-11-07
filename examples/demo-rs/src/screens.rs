// The demo's pages: the index below, one screen per product route over
// the nsh-firebase crates. Each screen takes its product client, so the
// app passes the demo instances and a test passes stubs.

use std::sync::Arc;

use nsh_base::{input, nav_page, nav_stack, theme};
use nsh_core::{element::AnyElement, prelude::*};
use nsh_firebase_auth::{Auth, AuthUser};
use nsh_firebase_core::FirebaseError;
use nsh_firebase_database::ListenSignals;
use nsh_firebase_storage::{FirebaseStorage, UploadTask};
use nsh_form::{BindField, SubmitError, field, rules, use_form};
use serde_json::json;

use crate::firebase;
use crate::nav::{self, Route};
use crate::ui;

/// The navigation stack: the index below, one page per pushed route.
pub fn stack() -> AnyElement {
    nav_stack()
        .flex_1()
        .child(home())
        .child(pushed())
        .into_element()
}

/// The pages of the pushed routes, made when pushed, gone when popped.
fn pushed() -> AnyElement {
    let routes = nav::routes();
    each(
        move || routes.with(|routes| routes.0.clone()),
        |route: &Route| route.key(),
        |route, depth| {
            let route = route.get_untracked();
            page(route)
                // The user popped it natively: it and what is above it go.
                .on_dismiss(move || nav::truncate(depth.get_untracked()))
        },
    )
    .into_element()
}

/// One page of the stack: its title for the system header, its header for the desktops and the web.
fn page(route: Route) -> nsh_core::element::Screen {
    nav_page()
        .key(route.key())
        .test_id(route.test_id())
        .title(route.title())
        .header(page_header(route.title()))
        .child(content(route))
}

fn home() -> nsh_core::element::Screen {
    let home = Route::Home;
    nav_page()
        .key(home.key())
        .test_id(home.test_id())
        .title(home.title())
        .header(page_header(home.title()))
        .child(index())
}

fn index() -> AnyElement {
    scroll()
        .flex_1()
        .child(view().gap_2().p_4().children(
            nav::PRODUCTS.iter().map(|route| {
                ui::index_row(route.title(), route.detail(), move || nav::push(*route))
            }),
        ))
        .into_element()
}

fn content(route: Route) -> AnyElement {
    match route {
        Route::Home => index(),
        Route::Auth => auth_screen(firebase::demo_auth()),
        Route::Database => database_screen(firebase::demo_database()),
        Route::Storage => storage_screen(Arc::new(firebase::demo_storage())),
        Route::Messaging => messaging_screen(firebase::demo_messaging()),
        Route::Analytics => analytics_screen(Arc::new(firebase::demo_analytics())),
        // The monitor screen belongs to the perfmon area; the shell leaves it empty.
        Route::Monitor => view().into_element(),
    }
}

/// The page's header on the desktops and the web; iOS and Android show the system's,
/// from the page's title. The back button follows the stack's depth, so it is dynamic.
fn page_header(title: &'static str) -> AnyElement {
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .border_b_1()
        .border_b_color(move || theme().border)
        .px_4()
        .py_2()
        .child(dynamic(move || {
            if nav::depth() > 0 {
                ui::app_button("Back", ui::Variant::Ghost, || false, nav::pop).into_element()
            } else {
                view().into_element()
            }
        }))
        .child(
            text(title)
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || theme().foreground),
        )
        .into_element()
}

// --- Authentication ---------------------------------------------------------

#[derive(Clone, Default, PartialEq)]
struct SignIn {
    email: String,
    password: String,
}

/// The user line the screen shows: who the currentUser signal holds.
fn user_line(user: Option<AuthUser>) -> String {
    match user {
        None => "Signed out".to_owned(),
        Some(user) => match user.email {
            Some(email) => format!("Signed in as {} ({email})", user.uid),
            None => format!("Signed in as {}", user.uid),
        },
    }
}

/// A server reply lands on the field it names; everything else stays a
/// form-level message with the code.
fn submit_error(error: &FirebaseError) -> SubmitError {
    match error.code.as_str() {
        "auth/user-not-found" | "auth/email-already-in-use" => {
            SubmitError::field("email", error.message.clone())
        }
        "auth/wrong-password" | "auth/invalid-credential" | "auth/weak-password" => {
            SubmitError::field("password", error.message.clone())
        }
        _ => SubmitError::form(format!("{}: {}", error.code, error.message)),
    }
}

/// The press that submits the form through `next`'s flow: one closure
/// per button, each owning its client clone.
fn submit_with(
    auth: Arc<Auth>,
    form: nsh_form::Form<SignIn>,
    mode: Signal<&'static str>,
    next: &'static str,
) -> impl Fn() + 'static {
    move || {
        mode.set(next);
        let auth = Arc::clone(&auth);
        form.submit(move |values: SignIn| {
            let (auth, mode) = (Arc::clone(&auth), mode);
            async move {
                let outcome = if mode.get_untracked() == "signup" {
                    auth.create_user_with_email_and_password(&values.email, &values.password)
                        .await
                } else {
                    auth.sign_in_with_email_and_password(&values.email, &values.password)
                        .await
                };
                outcome.map(|_| ()).map_err(|error| submit_error(&error))
            }
        });
    }
}

pub fn auth_screen(auth: Arc<Auth>) -> AnyElement {
    let form = use_form(SignIn::default());
    let email = field!(form, email)
        .rule(rules::required("Enter your email."))
        .rule(rules::email("That is not an email address."));
    let password = field!(form, password)
        .rule(rules::required("Enter your password."))
        .rule(rules::min_length(8, "Use at least 8 characters."));
    // The button decides which flow the submit runs; read at the submit,
    // where the value is final.
    let mode = signal("signin");
    let user_text_auth = Arc::clone(&auth);
    let signed_in = computed({
        let auth = Arc::clone(&auth);
        move || auth.current_user().is_some()
    });
    let sign_out = {
        let auth = Arc::clone(&auth);
        move || auth.sign_out()
    };

    view()
        .flex_1()
        .p_4()
        .gap_4()
        .child(
            view()
                .gap_2()
                .rounded(move || theme().radius)
                .border_1()
                .border_color(move || theme().border)
                .p_4()
                .child(
                    text(move || user_line(user_text_auth.current_user()))
                        .test_id("auth-user")
                        .text_sm()
                        .text_color(move || theme().muted_foreground),
                )
                .child(
                    view()
                        .gap_3()
                        .pt_2()
                        // The form keeps its draft while signed in; it only hides.
                        .style_when(signed_in, |s| s.hidden())
                        .child(
                            view()
                                .gap_1()
                                .child(
                                    text("Email")
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(move || theme().foreground),
                                )
                                .child(
                                    email.bind(
                                        input()
                                            .test_id("auth-email")
                                            .placeholder("you@example.test"),
                                    ),
                                )
                                .child(
                                    text(move || email.error().unwrap_or_default())
                                        .test_id("auth-email-error")
                                        .text_sm()
                                        .text_color(move || theme().destructive),
                                ),
                        )
                        .child(
                            view()
                                .gap_1()
                                .child(
                                    text("Password")
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(move || theme().foreground),
                                )
                                .child(
                                    password.bind(input().test_id("auth-password").masked(true))
                                )
                                .child(
                                    text(move || password.error().unwrap_or_default())
                                        .test_id("auth-password-error")
                                        .text_sm()
                                        .text_color(move || theme().destructive),
                                ),
                        )
                        .child(
                            text(move || form.error().unwrap_or_default())
                                .test_id("auth-form-error")
                                .text_sm()
                                .text_color(move || theme().destructive),
                        )
                        .child(
                            view()
                                .flex_row()
                                .gap_2()
                                .child(ui::app_button(
                                    "Sign in",
                                    ui::Variant::Primary,
                                    || false,
                                    submit_with(Arc::clone(&auth), form, mode, "signin"),
                                ))
                                .child(ui::app_button(
                                    "Create account",
                                    ui::Variant::Outline,
                                    || false,
                                    submit_with(Arc::clone(&auth), form, mode, "signup"),
                                )),
                        ),
                ),
        )
        // The session note and the sign-out replace the form while signed
        // in; the form only hides, so its draft survives a sign-out.
        .child(
            view()
                .gap_2()
                .style_when(move || !signed_in.get(), |s| s.hidden())
                .child(
                    text("The session persists; reload keeps this user signed in.")
                        .text_sm()
                        .text_color(move || theme().muted_foreground),
                )
                .child(ui::app_button(
                    "Sign out",
                    ui::Variant::Ghost,
                    || false,
                    sign_out,
                )),
        )
        .child(
            text("Against the demo-nsh emulator on 127.0.0.1:19099; a fake project with fake credentials.")
                .text_xs()
                .text_color(move || theme().muted_foreground),
        )
        .into_element()
}

// --- Realtime Database ------------------------------------------------------

/// One row of the map the ref holds, newest first.
#[derive(Clone, PartialEq)]
struct ItemRow {
    id: String,
    title: String,
}

fn to_rows(value: Option<&serde_json::Value>) -> Vec<ItemRow> {
    let Some(map) = value.and_then(|value| value.as_object()) else {
        return Vec::new();
    };
    let mut rows: Vec<ItemRow> = map
        .iter()
        .map(|(id, entry)| {
            let title = entry
                .get("title")
                .and_then(|title| title.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| id.clone());
            ItemRow {
                id: id.clone(),
                title,
            }
        })
        .collect();
    rows.reverse();
    rows
}

/// The live list's status, worded as the screen shows it.
fn status_label(status: &nsh_firebase_database::ListenStatus) -> &'static str {
    use nsh_firebase_database::ListenStatus as Status;
    match status {
        Status::Connecting => "Connecting…",
        Status::Live => "Live",
        Status::Reconnecting { .. } => "Reconnecting…",
        Status::Revoked => "Stopped",
    }
}

pub fn database_screen(db: Arc<nsh_firebase_database::Database>) -> AnyElement {
    let r#ref = db.r#ref("demo/items");
    // The value and the connection state as signals: the list and the
    // status line update by themselves. The listener stops with this
    // page (on_value registers the cleanup).
    // on_value answers a Result of opening the stream; the demo's
    // transport always opens and reports failures through the sink.
    let ListenSignals { value, status, .. } = r#ref
        .on_value()
        .expect("the demo's stream transport always opens");
    let rows = computed(move || to_rows(value.get().as_ref()));
    let draft = signal(String::new());

    let add = {
        let (r#ref, draft) = (r#ref.clone(), draft);
        move || {
            let title = draft.get_untracked();
            if title.trim().is_empty() {
                return;
            }
            draft.set(String::new());
            let r#ref = r#ref.clone();
            spawn(async move {
                let _ = r#ref.push(&json!({ "title": title })).await;
            });
        }
    };
    let clear = {
        let r#ref = r#ref.clone();
        move || {
            let r#ref = r#ref.clone();
            spawn(async move {
                let _ = r#ref.set(&serde_json::Value::Null).await;
            });
        }
    };

    view()
        .flex_1()
        .bg(move || theme().background)
        .child(
            view()
                .gap_2()
                .border_b_1()
                .border_b_color(move || theme().border)
                .px_4()
                .py_3()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            text(move || {
                                format!(
                                    "{} items · {}",
                                    rows.with(Vec::len),
                                    status_label(&status.get()),
                                )
                            })
                            .test_id("db-status")
                            .text_sm()
                            .text_color(move || theme().muted_foreground),
                        )
                        .child(ui::app_button("Clear", ui::Variant::Ghost, || false, clear)),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            input()
                                .test_id("db-draft")
                                .placeholder("New item")
                                .value(move || draft.get())
                                .on_change(move |text: &str| draft.set(text.to_owned()))
                                .flex_1(),
                        )
                        .child(ui::app_button(
                            "Add",
                            ui::Variant::Primary,
                            move || draft.get().trim().is_empty(),
                            add,
                        )),
                ),
        )
        .child(
            virtual_list(
                move || rows.with(Vec::len),
                move |at| {
                    let row = computed(move || rows.with(|r| r[at.index()].clone()));
                    view()
                        .border_b_1()
                        .border_b_color(move || theme().border)
                        .px_4()
                        .py_3()
                        .child(
                            text(move || row.with(|r| r.title.clone()))
                                .text_base()
                                .text_color(move || theme().foreground),
                        )
                        .child(
                            text(move || row.with(|r| r.id.clone()))
                                .text_xs()
                                .text_color(move || theme().muted_foreground),
                        )
                },
            )
            .estimated_item_size(px(52.0))
            .flex_1(),
        )
        .into_element()
}

// --- Cloud Storage ----------------------------------------------------------

/// 600_000 bytes: two full 256 KiB slices and a rest, so the bar steps.
const PAYLOAD_LEN: usize = 600_000;
const OBJECT_PATH: &str = "demo/notes.bin";

pub fn storage_screen(storage: Arc<FirebaseStorage>) -> AnyElement {
    let task = signal(None::<UploadTask>);
    let line = signal(String::from("No object uploaded yet."));
    let uploaded = signal(false);

    let start = {
        let (storage, task, line, uploaded) = (Arc::clone(&storage), task, line, uploaded);
        move || {
            if task.get().is_some() {
                return;
            }
            let upload = storage
                .r#ref(OBJECT_PATH)
                .upload(vec![0x41; PAYLOAD_LEN], None);
            task.set(Some(upload.clone()));
            // The object exists from the first slice on; a failed upload
            // clears it through the delete path's own error line.
            uploaded.set(true);
            spawn(async move {
                match upload.finished().await {
                    Ok(metadata) => line.set(format!(
                        "Uploaded {} ({} bytes).",
                        metadata.name,
                        metadata.size.unwrap_or(PAYLOAD_LEN as u64),
                    )),
                    Err(error) => line.set(format!("Upload failed: {error}")),
                }
                task.set(None);
            });
        }
    };
    let download = {
        let (storage, line) = (Arc::clone(&storage), line);
        move || {
            let storage = Arc::clone(&storage);
            spawn(async move {
                match storage.r#ref(OBJECT_PATH).download().await {
                    Ok(bytes) => line.set(format!("Downloaded {} bytes.", bytes.len())),
                    Err(error) => line.set(format!("Download failed: {error}")),
                }
            });
        }
    };
    let remove = {
        let (storage, line, uploaded) = (Arc::clone(&storage), line, uploaded);
        move || {
            let storage = Arc::clone(&storage);
            spawn(async move {
                match storage.r#ref(OBJECT_PATH).delete().await {
                    Ok(()) => {
                        line.set("Deleted.".to_owned());
                        uploaded.set(false);
                    }
                    Err(error) => line.set(format!("Delete failed: {error}")),
                }
            });
        }
    };

    view()
        .flex_1()
        .p_4()
        .gap_4()
        .child(
            text(move || line.get())
                .test_id("storage-line")
                .text_sm()
                .text_color(move || theme().foreground),
        )
        .child(
            view()
                .gap_2()
                // The task's progress signal drives the bar; the closures
                // read both the task slot and the progress signal.
                .style_when(move || task.get().is_none(), |s| s.hidden())
                .child(
                    view()
                        .gap_1()
                        .test_id("storage-progress")
                        .child(ui::app_progress(move || {
                            task.get()
                                .map(|task| {
                                    let snapshot = task.progress().get();
                                    (snapshot.bytes_transferred as f32
                                        / snapshot.total_bytes as f32)
                                        * 100.0
                                })
                                .unwrap_or(0.0)
                        }))
                        .child(
                            text(move || {
                                task.get()
                                    .map(|task| {
                                        let snapshot = task.progress().get();
                                        format!(
                                            "{} of {} bytes · {}",
                                            snapshot.bytes_transferred,
                                            snapshot.total_bytes,
                                            snapshot.state.as_str(),
                                        )
                                    })
                                    .unwrap_or_default()
                            })
                            .text_xs()
                            .text_color(move || theme().muted_foreground),
                        ),
                ),
        )
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(ui::app_button(
                    "Upload 600 kB",
                    ui::Variant::Primary,
                    move || task.get().is_some(),
                    start,
                ))
                .child(ui::app_button(
                    "Download",
                    ui::Variant::Outline,
                    move || !uploaded.get(),
                    download,
                ))
                .child(ui::app_button(
                    "Delete",
                    ui::Variant::Outline,
                    move || !uploaded.get(),
                    remove,
                )),
        )
        .child(
            text(
                "Resumable upload to the demo-nsh emulator on 127.0.0.1:19199, in 256 KiB slices.",
            )
            .text_xs()
            .text_color(move || theme().muted_foreground),
        )
        .into_element()
}

// --- Cloud Messaging --------------------------------------------------------

pub fn messaging_screen(messaging: nsh_firebase_messaging::Messaging) -> AnyElement {
    let permission = signal(String::from("Not checked."));
    let token_line = signal(String::from("Not requested."));

    let check = {
        let (messaging, permission) = (messaging.clone(), permission);
        move || {
            let (messaging, permission) = (messaging.clone(), permission);
            spawn(async move {
                match messaging.permission().await {
                    Ok(granted) => permission.set(granted.as_str().to_owned()),
                    Err(error) => permission.set(error.to_string()),
                }
            });
        }
    };
    let request = {
        let (messaging, token_line) = (messaging.clone(), token_line);
        move || {
            let (messaging, token_line) = (messaging.clone(), token_line);
            spawn(async move {
                match messaging.token().await {
                    Ok(token) => token_line.set(format!("Token: {token}")),
                    Err(error) => token_line.set(error.to_string()),
                }
            });
        }
    };

    view()
        .flex_1()
        .p_4()
        .gap_4()
        .child(
            view()
                .gap_2()
                .rounded(move || theme().radius)
                .border_1()
                .border_color(move || theme().border)
                .p_4()
                .child(
                    text("Permission")
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || theme().foreground),
                )
                .child(
                    text(move || permission.get())
                        .test_id("messaging-permission")
                        .text_sm()
                        .text_color(move || theme().muted_foreground),
                )
                .child(ui::app_button(
                    "Check permission",
                    ui::Variant::Ghost,
                    || false,
                    check,
                )),
        )
        .child(
            view()
                .gap_2()
                .rounded(move || theme().radius)
                .border_1()
                .border_color(move || theme().border)
                .p_4()
                .child(
                    text("Registration token")
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || theme().foreground),
                )
                .child(
                    text(move || token_line.get())
                        .test_id("messaging-token")
                        .text_sm()
                        .text_color(move || theme().muted_foreground),
                )
                .child(ui::app_button(
                    "Request token",
                    ui::Variant::Ghost,
                    || false,
                    request,
                )),
        )
        .child(
            text(firebase::messaging_needs_native())
                .text_xs()
                .text_color(move || theme().muted_foreground),
        )
        .child(
            text("This Mac is the pure tier: the requests above fail on purpose, and the screen says so instead of pretending. On the iOS and Android hosts the same calls reach the FCM SDK.")
                .text_xs()
                .text_color(move || theme().muted_foreground),
        )
        .into_element()
}

// --- Analytics --------------------------------------------------------------

pub fn analytics_screen(analytics: Arc<nsh_firebase_analytics::Analytics>) -> AnyElement {
    // The library's counters are plain reads, not signals: every handler
    // bumps this version, and the status binding that reads it re-reads
    // them.
    let version = signal(0_u32);

    let log = {
        let (analytics, version) = (Arc::clone(&analytics), version);
        move || {
            let _ = analytics.log_event("demo_tap", Some(json!({ "screen": "analytics" })));
            version.update(|n| *n += 1);
        }
    };
    let flush = {
        let (analytics, version) = (Arc::clone(&analytics), version);
        move || {
            let (analytics, version) = (Arc::clone(&analytics), version);
            spawn(async move {
                let _ = analytics.flush().await;
                version.update(|n| *n += 1);
            });
        }
    };

    view()
        .flex_1()
        .p_4()
        .gap_4()
        .child(
            view()
                .gap_1()
                .rounded(move || theme().radius)
                .border_1()
                .border_color(move || theme().border)
                .p_4()
                .child({
                    let analytics = Arc::clone(&analytics);
                    text(move || {
                        version.get();
                        format!("Pending: {}", analytics.pending())
                    })
                    .test_id("analytics-pending")
                    .text_sm()
                    .text_color(move || theme().foreground)
                })
                .child({
                    let analytics = Arc::clone(&analytics);
                    text(move || {
                        version.get();
                        format!("Dropped: {}", analytics.dropped())
                    })
                    .test_id("analytics-dropped")
                    .text_sm()
                    .text_color(move || theme().muted_foreground)
                }),
        )
        .child(
            view()
                .flex_row()
                .flex_wrap()
                .gap_2()
                .child(ui::app_button("Log event", ui::Variant::Primary, || false, log))
                .child(ui::app_button(
                    "Flush now",
                    ui::Variant::Ghost,
                    || false,
                    flush,
                )),
        )
        .child(
            text("Events batch locally (a batch every 3 events or 60 s) and go to the collector sink on 127.0.0.1:18742; the send retries with backoff while it is down.")
                .text_xs()
                .text_color(move || theme().muted_foreground),
        )
        .into_element()
}
