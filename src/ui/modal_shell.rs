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
use kagi_domain::status::ChangeKind;

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

/// Fraction of the window height a prose panel (recovery text) may occupy.
const MODAL_PROSE_VIEWPORT_FRAC: f32 = 0.25;

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
/// Standard card widths (#454). One of these, never a bare literal, so the
/// cards stay in step and a width change is one edit.
///
/// They are the pre-#454 widths **+20%**: at 420/480/540 a plan title wrapped
/// to four lines and a deep path clipped mid-segment, which read as a layout
/// bug rather than a long string (user report 2026-09-06). Growing the card is
/// the cheap half of that fix; the structural half is the sections and caps
/// above.
pub(crate) const MODAL_W_SM: f32 = 504.;
pub(crate) const MODAL_W_MD: f32 = 576.;
pub(crate) const MODAL_W_LG: f32 = 648.;

pub(crate) fn modal_card(width: f32) -> gpui::Div {
    modal_card_sized()
        .w(theme::scaled_px(width))
        // A card must never be wider than the window it sits in: at zoom 1.5
        // the LG card is 972px, past a 900px-wide window. `max_w` wins over
        // `w` in taffy, so the card shrinks instead of running off-screen.
        .max_w(gpui::relative(0.9))
}

/// [`modal_card`] without the width, for the one card whose width is already
/// computed in real pixels (the update modal sizes itself off the viewport).
/// Passing such a width through `modal_card` would apply `scaled_px` on top of
/// it and multiply the UI zoom in twice.
pub(crate) fn modal_card_sized() -> gpui::Div {
    div()
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
    div()
        .flex_1()
        .min_h(gpui::px(0.))
        // Clip, so a block that outgrows its share can never paint over the
        // pinned button row (#454: with the sections' padding and the tally
        // row added, the recovery prose did exactly that on a 700px window).
        .overflow_hidden()
        .flex()
        .flex_col()
        .gap_3()
}

/// Height cap for a **prose** panel (recovery text, notes) in a card whose
/// body does not scroll: a quarter of the window, then it scrolls inside its
/// own panel.
///
/// This is a sibling scroll region, not a nested one — the rule
/// ([`modal_body`]) is that no scroller sits *inside* another, and a card may
/// have one per panel. Without a cap the recovery text pushes past the card
/// and gets clipped, which is worse: the recovery instructions are the reason
/// a destructive operation is allowed at all.
pub(crate) fn modal_prose_max_h() -> gpui::Pixels {
    match theme::viewport_h() {
        Some(h) => gpui::px(h * MODAL_PROSE_VIEWPORT_FRAC),
        None => theme::scaled_px(MODAL_LIST_FALLBACK_ROWS * MODAL_LIST_ROW_H),
    }
}

/// A prose panel body (recovery text, notes): capped by [`modal_prose_max_h`],
/// scrollable in place, and floored at two lines.
///
/// Both bounds are load-bearing. Without the cap the text pushed past the card
/// and painted over the button row; without the floor the list panel's own
/// floor squeezed the prose to zero height and the recovery instructions
/// vanished entirely — measured on a 700px window, both directions.
pub(crate) fn modal_prose_box(
    id: &'static str,
    body: gpui::AnyElement,
) -> gpui::Stateful<gpui::Div> {
    /// Two lines of `text_xs` plus its line gap.
    const PROSE_FLOOR_H: f32 = 34.;
    div()
        .id(id)
        .min_h(theme::scaled_px(PROSE_FLOOR_H))
        .max_h(modal_prose_max_h())
        .overflow_y_scroll()
        .child(body)
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

/// A count/label chip: the small rounded pill the #454 mock puts at the right
/// of a section header (`487`) and next to a card title (`Cannot be undone`).
///
/// `color` tints text and background off the same token, via the shared
/// `badge_style` used by the graph/commit badges, so a chip in a modal matches
/// a chip everywhere else.
pub(crate) fn modal_chip(text: impl Into<SharedString>, color: u32) -> gpui::Div {
    let (bg, border, _) = theme::badge_style(color);
    div()
        .flex_shrink_0()
        .px_2()
        .rounded_full()
        .bg(gpui::rgba(bg))
        .border_1()
        .border_color(gpui::rgba(border))
        .text_xs()
        .text_color(rgb(color))
        .child(text.into())
}

/// One section inside a [`modal_card`], rendered as the mock's inset panel:
/// a rounded surface with a header row (caret + title + right-aligned count
/// chip) and, while open, its body.
///
/// Clicking the header flips the section via `KagiApp::toggle_modal_section`.
/// `body` is built by the caller and only attached while open, so a closed
/// section costs nothing to render. The count chip stays visible when closed —
/// the audit's finding was modals that printed a total and then cut the rows,
/// so the size of what is hidden is never lost.
///
/// `count == 0` renders no chip (the recovery section has instructions, not a
/// count); pass a chip label through `chip` instead when the section wants a
/// word (`oplog`) rather than a number.
pub(crate) fn modal_section(
    id: &'static str,
    title: impl Into<SharedString>,
    count: usize,
    open: bool,
    body: Option<gpui::AnyElement>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    modal_section_chipped(id, title, Some(count.to_string().into()), open, body, cx)
}

/// [`modal_section`] with an explicit chip label instead of a count.
pub(crate) fn modal_section_chipped(
    id: &'static str,
    title: impl Into<SharedString>,
    chip: Option<SharedString>,
    open: bool,
    body: Option<gpui::AnyElement>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let (panel_bg, panel_border) = theme::panel_style();
    let caret = if open { "\u{25be}" } else { "\u{25b8}" };
    let mut header = div()
        .id(id)
        .flex_shrink_0()
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
        );
    if let Some(chip) = chip {
        header = header.child(
            div()
                .ml_auto()
                .child(modal_chip(chip, current_theme().text_sub)),
        );
    }

    // The panel: a rounded surface with a hairline border, so sections read as
    // separate cards instead of one text column (#454 mock, right half).
    let mut section = div()
        .flex()
        .flex_col()
        .gap_2()
        .min_h(gpui::px(0.))
        .p_2()
        .rounded_md()
        .bg(gpui::rgba(panel_bg))
        .border_1()
        .border_color(gpui::rgba(panel_border))
        .child(header);
    if open {
        if let Some(body) = body {
            section = section.child(body);
        }
    }
    section.into_any_element()
}

/// A non-collapsible list panel: the mock's `対象ファイル` box — same surface,
/// border and header (title + count chip) as [`modal_section`], but with no
/// caret, because the list of things an operation *acts on* must never be
/// hideable behind disclosure.
///
/// `body` supplies the scroll region (a capped `overflow_y_scroll` column or a
/// `uniform_list`); the panel itself only clips and yields height, so the card
/// keeps exactly one scroller per panel and never nests them.
pub(crate) fn modal_list_panel(
    title: impl Into<SharedString>,
    count: usize,
    body: gpui::AnyElement,
) -> gpui::Div {
    // Theme-independent panel tint: `surface == modal` in several themes, so a
    // `surface` fill would be invisible on the card (see `theme::panel_style`).
    let (panel_bg, panel_border) = theme::panel_style();
    // Floor, so the panel is not the block that gives everything up: on a
    // 700px window the prose below it (recovery text, notes) kept its three
    // lines while the list collapsed to five rows — the wrong priority, since
    // the list is the operation's data and the prose is advice. The prose
    // panels stay `min_h(0)` and yield first; this floor is the header/padding
    // chrome plus up to `PANEL_FLOOR_ROWS` rows.
    const PANEL_FLOOR_ROWS: f32 = 8.;
    const PANEL_CHROME_H: f32 = 44.;
    let floor_rows = (count as f32).min(PANEL_FLOOR_ROWS);
    div()
        .min_h(theme::scaled_px(
            floor_rows * MODAL_LIST_ROW_H + PANEL_CHROME_H,
        ))
        .overflow_hidden()
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .rounded_md()
        .bg(gpui::rgba(panel_bg))
        .border_1()
        .border_color(gpui::rgba(panel_border))
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_label))
                        .child(title.into()),
                )
                .child(
                    div()
                        .ml_auto()
                        .child(modal_chip(count.to_string(), current_theme().text_sub)),
                ),
        )
        .child(body)
}

/// One row of a modal file list: the change-kind letter badge the mock shows
/// (`M` / `A` / `D` / `R` / `T`) plus the path.
///
/// Fixed row height so the list-height math in [`modal_list_max_h`] stays
/// exact, and `overflow_hidden` clips a path that does not fit rather than
/// truncating its head (cutting the head keeps the least identifying part).
pub(crate) fn modal_file_row(path: impl Into<SharedString>, change: &ChangeKind) -> gpui::Div {
    let (letter, color) = change_badge(change);
    div()
        .flex_shrink_0()
        .h(theme::scaled_px(MODAL_LIST_ROW_H))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_shrink_0()
                .w(theme::scaled_px(10.))
                .text_xs()
                .text_color(rgb(color))
                .child(SharedString::from(letter.to_string())),
        )
        .child(
            // One line, always: a wrapped path grew past this row's fixed
            // height and painted over the next one (`uniform_list` does not
            // clip its items), which is what "long paths collide with the line
            // below" was (user report 2026-09-06). The ellipsis goes at the
            // **start**: a deep path's tail — the file name — is the part worth
            // keeping, the same reason these paths were never `take(80)`-ed.
            div()
                .flex_1()
                .min_w(gpui::px(0.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_start()
                .text_xs()
                .text_color(rgb(current_theme().text_sub))
                .child(path.into()),
        )
}

/// Letter + theme colour for a change kind. The letters match
/// `kagi_domain::message_gen`'s `change_letter` (A/M/D/R/T) and the colours the
/// `change_*` theme tokens the file tree already uses, so a discarded `D` row
/// is the same red as everywhere else.
fn change_badge(change: &ChangeKind) -> (char, u32) {
    let t = current_theme();
    match change {
        ChangeKind::Added => ('A', t.change_added),
        ChangeKind::Modified => ('M', t.change_modified),
        ChangeKind::Deleted => ('D', t.change_deleted),
        ChangeKind::Renamed { .. } => ('R', t.change_renamed),
        ChangeKind::TypeChange => ('T', t.change_typechange),
    }
}

/// #454 Phase 2 item 6: the per-kind tally the mock puts above a long list
/// (`M 115  D 5`), so a card that acts on hundreds of files says *what kind*
/// of change is at stake without the user scrolling the list.
///
/// `None` below `SUMMARY_MIN_ROWS`: with a handful of rows the list itself is
/// already the summary, and a second count line would just be noise.
pub(crate) fn modal_change_summary(files: &[kagi_domain::status::FileStatus]) -> Option<gpui::Div> {
    /// Row count from which the tally earns its line.
    const SUMMARY_MIN_ROWS: usize = 10;
    if files.len() < SUMMARY_MIN_ROWS {
        return None;
    }
    // Counted in the order the badges are defined, so the row reads the same
    // way every time regardless of which kinds happen to be present.
    let kinds = [
        ChangeKind::Modified,
        ChangeKind::Added,
        ChangeKind::Deleted,
        ChangeKind::TypeChange,
    ];
    let mut row = div()
        .flex_shrink_0()
        .flex()
        .flex_row()
        .items_center()
        .gap_3();
    let mut any = false;
    for kind in kinds {
        let n = files.iter().filter(|f| f.change == kind).count();
        if n == 0 {
            continue;
        }
        any = true;
        let (letter, color) = change_badge(&kind);
        row = row.child(
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(color))
                .child(SharedString::from(format!("{} {}", letter, n))),
        );
    }
    // Renames carry a `from` path, so they cannot be counted by equality.
    let renamed = files
        .iter()
        .filter(|f| matches!(f.change, ChangeKind::Renamed { .. }))
        .count();
    if renamed > 0 {
        any = true;
        row = row.child(
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(current_theme().change_renamed))
                .child(SharedString::from(format!("R {}", renamed))),
        );
    }
    any.then_some(row)
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

/// #454: the path list under a note summary. The plan card's body does not
/// scroll any more, so this list carries its own capped scroll region — one
/// scroller per panel, never one inside another (see [`modal_body`]).
pub(crate) fn note_path_list_element(files: &[String]) -> gpui::AnyElement {
    let mut col = div()
        .id("note-path-list")
        .flex()
        .flex_col()
        .gap_px()
        .pl_4()
        .min_h(gpui::px(0.))
        .max_h(modal_list_max_h(files.len()))
        .overflow_y_scroll();
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
                .whitespace_nowrap()
                .text_ellipsis_start()
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
