// The demo's styled layer: one wrapper per base component shape, theme tokens only.

use nsh_base::{button, progress, theme};
use nsh_core::{element::FontWeight, prelude::*};

/// How loud a control is.
#[derive(Clone, Copy, PartialEq)]
pub enum Variant {
    /// The one emphasized commitment of a decision area.
    Primary,
    /// A clear boundary with less emphasis.
    Outline,
    /// A quiet row or toolbar action.
    Ghost,
}

/// The app's button: wrap once, use everywhere; the label color pairs
/// with the variant. Tests find it by `role:button[label]`: a component
/// element takes no test id from its caller.
pub fn app_button(
    label: &'static str,
    variant: Variant,
    disabled: impl Fn() -> bool + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .disabled(disabled)
        .on_press(on_press)
        .min_h_9()
        .px_4()
        .py_2()
        .flex_row()
        .items_center()
        .justify_center()
        .gap_2()
        .rounded(move || theme().radius)
        .when(variant == Variant::Primary, |b| {
            b.bg(move || theme().primary).active(|s| s.opacity(0.9))
        })
        .when(variant == Variant::Outline, |b| {
            b.border_1()
                .border_color(move || theme().border)
                .active(|s| s.bg(move || theme().muted))
        })
        .when(variant == Variant::Ghost, |b| {
            b.active(|s| s.bg(move || theme().muted))
        })
        .focus(|s| s.outline(px(2.0)).outline_color(move || theme().ring))
        .disabled_style(|s| s.opacity(0.5))
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || match variant {
                    Variant::Primary => theme().primary_foreground,
                    Variant::Outline | Variant::Ghost => theme().foreground,
                }),
        )
}

/// A quiet full-width row of the index: title, one line of detail, a chevron.
pub fn index_row(
    title: &'static str,
    detail: &'static str,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    button()
        .accessibility_label(title)
        .on_press(on_press)
        .min_h_11()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_2()
        .rounded(move || theme().radius)
        .active(|s| s.bg(move || theme().muted))
        .child(
            view()
                .flex_1()
                .items_start()
                .gap(px(2.0))
                .child(
                    text(title)
                        .text_base()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(move || theme().foreground),
                )
                .child(
                    text(detail)
                        .text_xs()
                        .text_color(move || theme().muted_foreground),
                ),
        )
        .child(text("›").text_color(move || theme().muted_foreground))
}

/// The app's progress bar: track and indicator styled once.
pub fn app_progress(value: impl IntoReactive<f32> + 'static) -> impl IntoElement {
    progress()
        .value(value)
        .track_style(|s| s.h(px(8.0)).rounded_full().bg(move || theme().muted))
        .indicator_style(|s| s.h(px(8.0)).rounded_full().bg(move || theme().primary))
}
