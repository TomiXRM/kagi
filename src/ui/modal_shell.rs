//! Shared modal chrome: the card shell and its collapsible sections (#454).
//!
//! The audit behind #454 found that modal *chrome* had no owner — 39
//! `ActiveModal` variants and 45 render functions shared only
//! `modal_renderers::modal_overlay`, so each renderer hand-built its card, and
//! only 2 of 8 renderer files bothered to cap the card's height. This module is
//! that missing owner; renderers adopt it one at a time.
//!
//! **Safety rule for [`modal_section`]**: only *supporting* detail may sit
//! behind disclosure. The list of things an operation acts on stays visible, so
//! a destructive confirm can never hide its own targets (and a sticky user
//! override can never carry a collapsed state into the next confirm).

use super::theme::{self, theme as current_theme};
use super::KagiApp;
use gpui::{div, prelude::*, rgb, Context, SharedString};

/// The shared modal card shell: a height-capped column that does **not**
/// scroll itself.
///
/// Layout contract for adopters:
/// `modal_card(w).child(<title>).child(modal_scroll_body().child(..)).child(<buttons>)`
///
/// The card is capped (`max_h`) so a plan with many notes can no longer grow
/// past the window edge — the audit found only 2 of 8 renderer files capped
/// anything. Scrolling lives on [`modal_scroll_body`], never on the card: with
/// a scrolling card, a long body pushes the Cancel/Confirm row out of view, and
/// a destructive confirm must always show its own buttons.
pub(crate) fn modal_card(width: f32) -> gpui::Div {
    div()
        .w(theme::scaled_px(width))
        .max_h(gpui::relative(0.8))
        .overflow_hidden()
        .bg(rgb(current_theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
}

/// The scrollable middle of a [`modal_card`]: everything between the fixed
/// title row and the fixed button row goes in here.
///
/// `flex_1 + min_h(0)` lets it shrink inside the capped card instead of
/// pushing the buttons off the bottom (the T027 flex-compression bug class).
pub(crate) fn modal_scroll_body() -> gpui::Stateful<gpui::Div> {
    div()
        .id("modal-body")
        .flex_1()
        .min_h(gpui::px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_3()
}

/// Is section `id` expanded, given the renderer's default?
///
/// `overrides` is `KagiApp::modal_section_overrides`: it records only the
/// sections the user flipped, so each renderer keeps ownership of its own
/// default and no `clear_*` path has to reset disclosure state.
pub(crate) fn section_open(
    overrides: &std::collections::HashSet<&'static str>,
    id: &'static str,
    default_open: bool,
) -> bool {
    default_open != overrides.contains(id)
}

/// One collapsible section inside a [`modal_card`].
///
/// Header row = caret + title + count chip; clicking it flips the section via
/// `KagiApp::toggle_modal_section`. `body` is built by the caller and only
/// attached while open, so a closed section costs nothing to render. The count
/// chip stays visible when closed — the audit's finding was modals that printed
/// a total and then cut the rows, so the size of what is hidden is never lost.
pub(crate) fn modal_section(
    id: &'static str,
    title: impl Into<SharedString>,
    count: usize,
    open: bool,
    body: Option<gpui::AnyElement>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let caret = if open { "\u{25be}" } else { "\u{25b8}" };
    let header = div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |this, _ev, _window, cx| {
                this.toggle_modal_section(id);
                cx.notify();
            }),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(current_theme().text_muted))
                .child(SharedString::from(caret)),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().text_label))
                .child(title.into()),
        )
        .child(
            div()
                .ml_auto()
                .px_2()
                .rounded_full()
                .bg(rgb(current_theme().bg_row_alt))
                .text_xs()
                .text_color(rgb(current_theme().text_sub))
                .child(SharedString::from(count.to_string())),
        );

    let mut section = div().flex().flex_col().gap_1().child(header);
    if open {
        if let Some(body) = body {
            section = section.child(body);
        }
    }
    section.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::section_open;
    use std::collections::HashSet;

    /// The renderer owns the default; the set only records a flip. This is what
    /// lets a destructive modal keep its target list visible by construction
    /// while still remembering that the user collapsed a detail section.
    #[test]
    fn overrides_flip_the_renderer_default() {
        let mut o: HashSet<&'static str> = HashSet::new();
        assert!(section_open(&o, "files", true));
        assert!(!section_open(&o, "skipped", false));

        o.insert("skipped");
        assert!(section_open(&o, "skipped", false));
        // Flipping one section must not touch another.
        assert!(section_open(&o, "files", true));

        o.remove("skipped");
        assert!(!section_open(&o, "skipped", false));
    }
}
