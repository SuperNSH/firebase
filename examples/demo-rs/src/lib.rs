//! The Rust demo app (`examples/demo-rs`): one screen per nsh-firebase
//! product, the same routes as the TSX demo (`examples/demo`). The shell
//! wires the routes; the screens land in the second pass, when the product
//! crates do.

mod firebase;
mod nav;
mod screens;
mod ui;

use nsh_base::theme;
use nsh_core::{element::desktop::window as app_window, prelude::*};

/// The UI set of the demo (`nsh.json` `"rust"`): what a TSX entry could import.
#[native_module("demo/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

/// The app root: its one window, the theme behind it, the navigation stack.
/// Off the desktops `window()` shows its content in the app's one surface.
#[component]
pub fn App() -> impl IntoElement {
    app_window()
        .id("main")
        .title("Firebase Demo Rust")
        .content(|| {
            view()
                .flex_1()
                .bg(move || theme().background)
                .text_color(move || theme().foreground)
                .child(screens::stack())
        })
}
