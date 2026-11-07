//! The Rust demo app (`examples/demo-rs`): one screen per nsh-firebase
//! product, the same routes as the TSX demo (`examples/demo`). The shell
//! wires the routes; the screens land in the second pass, when the product
//! crates do.

mod firebase;
mod nav;
mod screens;
mod ui;

use nsh_base::theme;
use nsh_core::prelude::*;

/// The UI set of the demo (`nsh.json` `"rust"`): what a TSX entry could import.
#[native_module("demo/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

/// The app root: the whole surface, the theme behind it, the navigation stack.
#[component]
pub fn App() -> impl IntoElement {
    view()
        .flex_1()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .child(screens::stack())
}
