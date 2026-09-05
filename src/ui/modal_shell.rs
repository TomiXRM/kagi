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

/// Modal list geometry, shared by every scrollable list in a card (#454). The
/// row height must match the `uniform_list` item height exactly
/// (virtualization assumes uniform rows), and the plain lists give their rows
/// this height too so the height math below is exact.
pub(crate) const MODAL_LIST_ROW_H: f32 = 18.;

/// Fraction of the window height a single modal list may occupy.
///
/// The card itself is capped at 80% of the window; above the list sit the
/// title and the current→predicted block, below it blockers, recovery text
/// and the pinned action row. 40% leaves those visible.
const MODAL_LIST_VIEWPORT_FRAC: f32 = 0.4;

/// Row ceiling used only before the first frame publishes a window height
/// (headless snapshot tests), to keep list heights deterministic there.
const MODAL_LIST_FALLBACK_ROWS: f32 = 20.;

/// A list is never squeezed below this many rows: on a short window the fixed
/// prose would otherwise take everything and leave a 0-height list — the
/// operation's own targets, invisible (observed at 700px, #454).
const MODAL_LIST_MIN_ROWS: f32 = 3.;

/// Height for a scrolling modal list of `rows` rows: it hugs its content while
/// short, then stops at [`MODAL_LIST_VIEWPORT_FRAC`] of the **window height**,
/// never below [`MODAL_LIST_MIN_ROWS`] (a list that shrinks to nothing hides
/// what the operation acts on).
///
/// #454: the height used to be a flat `min(rows, 20) * 18px` — the same box on
/// a 700px window as on a 1400px one. Only the cards that scroll their list
/// (not their body — see [`modal_body`]) use this.
pub(crate) fn modal_list_max_h(rows: usize) -> gpui::Pixels {
    let rows = rows.max(1) as f32;
    let content = theme::scaled_px(rows * MODAL_LIST_ROW_H);
    // The window height is already in real pixels; only rows are zoom-scaled.
    let ceiling = match theme::viewport_h() {
        Some(h) => gpui::px(h * MODAL_LIST_VIEWPORT_FRAC),
        None => theme::scaled_px(MODAL_LIST_FALLBACK_ROWS * MODAL_LIST_ROW_H),
    };
    let floor = theme::scaled_px(rows.min(MODAL_LIST_MIN_ROWS) * MODAL_LIST_ROW_H);
    content.min(ceiling).max(floor)
}

/// The shared modal card shell: a height-capped column that does **not**
/// scroll itself.
///
/// Layout contract for adopters:
/// `modal_card(w).child(<title>).child(modal_body().child(..)).child(<buttons>)`
///
/// The card is capped (`max_h`) so a plan with many notes can no longer grow
/// past the window edge — the audit found only 2 of 8 renderer files capped
/// anything. Nothing here scrolls: with a scrolling card, a long body pushes
/// the Cancel/Confirm row out of view, and a destructive confirm must always
/// show its own buttons. The lists inside [`modal_body`] are the scrollers.
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

/// The middle of a [`modal_card`] for cards whose **lists** are the scroll
/// region (amend, discard): it does not scroll itself.
///
/// #454 layout rule — *one scroll region per card, never nested*. A scrolling
/// body wrapped around scrolling lists gave a scroller inside a scroller: the
/// outer one drifted and clipped the card's own labels (`CURRENT` slid off the
/// top on a 700px window). So a card picks exactly one:
///
/// * huge, virtualized list + short prose -> `modal_body` here, and the list
///   carries the scroll ([`modal_list_max_h`] for its height);
/// * long prose + plain bounded lists -> [`modal_scroll_body`], and the lists
///   render at full height inside it.
///
/// `flex_1 + min_h(0)` lets the body shrink inside the capped card instead of
/// pushing the buttons off the bottom (the T027 flex-compression bug class).
pub(crate) fn modal_body() -> gpui::Div {
    div().flex_1().min_h(gpui::px(0.)).flex().flex_col().gap_3()
}

/// The middle of a [`modal_card`] that scrolls, for cards whose lists are
/// plain and bounded (the shared plan card: preview commits are capped at 100
/// by the producer, note paths by the plan). Everything inside renders at full
/// height and this is the card's single scroll region — see [`modal_body`] for
/// the rule.
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

/// #454: does this note carry a path **list** the card should render as a
/// list instead of a comma wall inside the sentence?
///
/// Returns the summary line (no paths) plus the paths. Only the checkout
/// overlap blocker qualifies today — it is the one note whose producer builds
/// a `Vec<String>` of paths (`predict_checkout_conflict`), and with 40
/// overlapping files the joined form filled the whole confirmation card.
pub(crate) fn note_path_list(
    note: &kagi_domain::plan_note::PlanNote,
) -> Option<(String, Vec<String>)> {
    use kagi_domain::plan_note::{checkout::CheckoutNote, PlanNote};
    match note {
        PlanNote::Checkout(CheckoutNote::CheckoutOverlap { count, files }) => Some((
            super::i18n::Msg::PlanOverlapSummary
                .t()
                .replace("{}", &count.to_string()),
            files.clone(),
        )),
        _ => None,
    }
}

/// #454: the path list under a note summary. It lives in the shared plan card,
/// whose body is the scroll region ([`modal_scroll_body`]), so the list renders
/// at full height — every path is reachable by scrolling the card once, with no
/// scroller inside a scroller.
pub(crate) fn note_path_list_element(files: &[String]) -> gpui::AnyElement {
    let mut col = div().flex().flex_col().gap_px().pl_4();
    for f in files {
        col = col.child(
            div()
                .flex_shrink_0()
                .h(theme::scaled_px(MODAL_LIST_ROW_H))
                .w_full()
                .flex()
                .items_center()
                .text_xs()
                .text_color(rgb(current_theme().text_sub))
                .overflow_hidden()
                .child(SharedString::from(f.clone())),
        );
    }
    col.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{modal_list_max_h, section_open, MODAL_LIST_ROW_H};
    use std::collections::HashSet;

    /// #454: the list ceiling follows the **window height**, not a fixed row
    /// count — the same 100-row list must get a taller box on a taller window,
    /// while a 3-row list stays 3 rows on both.
    #[test]
    fn list_ceiling_tracks_window_height() {
        let rows_px = |n: f32| crate::ui::theme::scaled_px(n * MODAL_LIST_ROW_H);

        crate::ui::theme::set_viewport_h(700.);
        let short_window = modal_list_max_h(100);
        assert_eq!(short_window, gpui::px(700. * 0.4));
        assert_eq!(modal_list_max_h(3), rows_px(3.), "short list hugs content");

        crate::ui::theme::set_viewport_h(1400.);
        let tall_window = modal_list_max_h(100);
        assert_eq!(tall_window, gpui::px(1400. * 0.4));
        assert!(
            tall_window > short_window,
            "a taller window must show more rows ({tall_window:?} vs {short_window:?})"
        );
        assert_eq!(modal_list_max_h(3), rows_px(3.), "short list still 3 rows");

        // Before the first frame publishes a height, fall back to 20 rows so
        // headless renders stay deterministic.
        crate::ui::theme::set_viewport_h(0.);
        assert_eq!(modal_list_max_h(100), rows_px(20.));
    }

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
