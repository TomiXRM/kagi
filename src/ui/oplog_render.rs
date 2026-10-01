//! Operation Log panel rendering (`impl Render for OpLogPanel`), issue #468.
//!
//! Split out of `render_overlay.rs` (which keeps the toast stack) so the
//! variable-height list + selectable detail block live in a focused module.
//!
//! Two fixes over the previous `uniform_list` rendering:
//!
//! 1. **Variable row height.** `uniform_list` lays every row out at the FIRST
//!    row's height, so an expanded row's detail block — N lines, each free to
//!    soft-wrap — painted straight over the rows below. The list is now
//!    `gpui::list` + a `gpui::ListState`, the same swap T-DIFF-WRAP-001 made
//!    for the diff panes (`render_helpers::render_diff_list`); the item count
//!    is synced once per render, the lifecycle documented there. With a
//!    variable height the detail lines are free to wrap, so they do (no
//!    truncation) — a 200-char `error:` is readable instead of clipped. The
//!    summary row stays one fixed-height truncated line.
//! 2. **Selection + copy.** The detail block renders through a selectable
//!    `gpui_component::text::TextView` (escaped HTML, the same trick
//!    `inspector.rs` uses for commit messages — GPUI has no selection for a
//!    plain text run, and `TextView` parses only Markdown or HTML, of which
//!    Markdown would misread an error string). The summary row carries a
//!    hover-quiet copy button that writes the WHOLE entry — including the tail
//!    the summary truncates — to the clipboard via `OpLogPanel::copy_entry`.
//!
//! Never read this entity back through `cx` inside the list closure (it runs
//! while the entity is borrowed → panic). The row data is snapshotted before
//! the list is built; the click handlers run on mouse events, well after.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, rgb, App, ClickEvent, Context, Element as _, Entity, InteractiveElement, IntoElement,
    MouseButton, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
};
use kagi_domain::oplog_reflog::Attribution;
use kagi_git::oplog::{Actor, OpLogEntry, OpOutcome};

use super::i18n::{self, Msg};
use super::render_helpers::with_vertical_scrollbar;
use super::theme::theme;
use super::{format_hms, oplog_panel, theme as theme_mod};

/// Height of the collapsed summary line (issue #468's E2E asserts that an
/// expanded row grows past it).
const SUMMARY_ROW_H: f32 = 22.;

/// Issue #548: above this many bytes of detail text the block stops being
/// selectable.
///
/// gpui-component's `Inline::paint` recomputes a selection rectangle for EVERY
/// character on EVERY paint while `selectable(true)` — even with nothing
/// selected — and each one is a from-the-start scan of the wrapped line, so the
/// cost grows with the square of the text. A real 158 KB abort error measured
/// **9.7 s per draw**; the same payload not selectable, 4.6 ms. Under this
/// threshold the whole block costs well under a millisecond, so ordinary
/// entries keep issue #468's drag-select.
///
/// Nothing becomes unreachable: the row's copy button still writes the WHOLE
/// entry to the clipboard, and a note says so.
///
/// ponytail: a size switch, not a fix in the dependency. The real repair is
/// bounding `Inline::paint` to the visible glyphs upstream (#548 improvement 1);
/// raise or drop this threshold once that lands.
const SELECTABLE_DETAIL_MAX: usize = 4096;

impl gpui::Render for oplog_panel::OpLogPanel {
    /// Render the Operation Log tab body (T-BP-004).
    ///
    /// Each row shows `HH:MM:SS  op  outcome-summary` (outcome coloured
    /// green/red/yellow). Clicking a row toggles single-row expansion
    /// (before/after + error/blockers); the copy button copies the entry.
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entry_count = self.len();

        if entry_count == 0 {
            return div()
                .flex_1()
                .min_h(px(0.))
                .bg(rgb(theme().panel))
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::NoOperationsYet.t()))
                .into_any();
        }

        let scroll_handle = self.scroll_handle();
        // ListState lifecycle (see `render_helpers::render_diff_list`): sync the
        // item count here, once per render. Every mutation path (`push`, the
        // disk seed) calls `cx.notify()`, so the next render sees the new count.
        if scroll_handle.item_count() != entry_count {
            scroll_handle.reset(entry_count);
        }
        // W12-GCADOPT (§2.10): scrollbar overlay on the Operation Log list.
        // gpui-component implements `ScrollbarHandle` for `ListState` directly.
        let scrollbar_handle = scroll_handle.clone();

        // Snapshot before the closure — capped at OP_ENTRIES_MAX (200), and the
        // previous `uniform_list` cloned the same vec on every range callback.
        let entries: Vec<OpLogEntry> = self.entries().iter().cloned().collect();
        let expanded = self.expanded();
        let reflog = self.reflog_detail().cloned();
        let entity = cx.entity();

        let oplog_list = gpui::list(scroll_handle, move |i, _window, _cx| match entries.get(i) {
            Some(entry) => {
                let detail = (expanded == Some(i)).then_some(reflog.as_ref());
                render_row(i, entry, detail, &entity)
            }
            None => div().into_any_element(),
        })
        .flex_1()
        .min_h(px(0.))
        .bg(rgb(theme().panel));

        with_vertical_scrollbar("oplog-list-scroll", &scrollbar_handle, oplog_list, true)
            .into_any_element()
    }
}

/// One op-log row: the fixed-height truncated summary line plus, when
/// expanded (`detail` is `Some`), the selectable detail block underneath with
/// the entry's reflog section.
fn render_row(
    i: usize,
    entry: &OpLogEntry,
    detail: Option<Option<&oplog_panel::ReflogDetail>>,
    entity: &Entity<oplog_panel::OpLogPanel>,
) -> gpui::AnyElement {
    let outcome_color = match &entry.outcome {
        OpOutcome::Success { .. } => theme().color_success,
        OpOutcome::Partial { .. } | OpOutcome::Refused { .. } => theme().color_warning,
        OpOutcome::Unknown { .. } | OpOutcome::Failed { .. } => theme().color_blocker,
    };
    let outcome_label = SharedString::from(oplog_panel::outcome_summary(&entry.outcome));
    let time_label = SharedString::from(format_hms(entry.timestamp));
    let op_label = SharedString::from(entry.op.clone());

    let toggle_entity = entity.clone();
    let row_click = move |_: &ClickEvent, _window: &mut Window, cx: &mut App| {
        toggle_entity.update(cx, |this, cx| {
            this.select_row(i, cx);
            cx.notify();
        });
    };
    let copy_entity = entity.clone();
    let copy_click = move |_: &ClickEvent, _window: &mut Window, cx: &mut App| {
        copy_entity.update(cx, |this, cx| this.copy_entry(i, cx));
    };

    // `bg_row_alt` is the zebra token; this used to be `panel`, which only
    // looked striped in themes whose chrome happens to differ from the base.
    let row_bg = if i.is_multiple_of(2) {
        theme().bg_row_alt
    } else {
        theme().bg_base
    };

    div()
        .id(("oplog-row", i))
        // Issue #468 E2E: the row's real laid-out height is read back through
        // `ListState::bounds_for_item`, which proves an expanded row GROWS
        // instead of overflowing onto the next one. So never pin a height here.
        .flex()
        .flex_col()
        .w_full()
        .bg(rgb(row_bg))
        .hover(|s| s.bg(rgb(theme().surface)).cursor_pointer())
        .on_click(row_click)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .px_3()
                .h(theme_mod::scaled_px(SUMMARY_ROW_H))
                .child(
                    div()
                        .w(theme_mod::scaled_px(60.))
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(time_label),
                )
                .child(
                    div()
                        .w(theme_mod::scaled_px(100.))
                        .flex_shrink_0()
                        .ml(theme_mod::scaled_px(6.))
                        .text_xs()
                        .text_color(rgb(theme().text_sub))
                        .child(op_label),
                )
                .child(actor_badge(i, entry.actor))
                .child(worktree_badge(i, entry))
                .child(
                    div()
                        .flex_1()
                        .ml(theme_mod::scaled_px(6.))
                        .text_xs()
                        .text_color(rgb(outcome_color))
                        .truncate()
                        .child(outcome_label),
                )
                // Issue #468: copy the WHOLE entry — the summary row truncates,
                // so its tail is otherwise unreachable. Quiet until hovered,
                // same treatment as the PR-hunk copy button.
                .child(
                    div()
                        .id(("oplog-row-copy", i))
                        .flex_shrink_0()
                        .ml(theme_mod::scaled_px(6.))
                        .p_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .opacity(0.55)
                        .hover(|s| s.bg(rgb(theme().selected)).opacity(1.0))
                        .tooltip(|w, cx| {
                            gpui_component::tooltip::Tooltip::new(Msg::OpLogCopyEntry.t())
                                .build(w, cx)
                        })
                        // Swallow the mouse-DOWN too, not just the click:
                        // otherwise it reaches the selectable TextView below and
                        // starts a text selection that tracks the cursor after
                        // the button is released.
                        .on_mouse_down(MouseButton::Left, |_e, _w, cx| {
                            cx.stop_propagation();
                        })
                        .on_click(copy_click)
                        .child(
                            gpui::svg()
                                .path("icons/copy.svg")
                                .w(theme_mod::scaled_px(11.))
                                .h(theme_mod::scaled_px(11.))
                                .text_color(rgb(theme().text_sub)),
                        ),
                ),
        )
        .when_some(detail, |row, reflog| {
            row.child(render_detail(i, entry))
                .child(render_reflog(i, reflog))
        })
        .into_any_element()
}

/// The expanded detail block: every line of the entry, soft-wrapped (never
/// truncated — the row is variable-height now) and drag-selectable.
///
/// ponytail: the leading/aligned spaces of `detail_lines` may collapse in the
/// HTML text run; the clipboard copy keeps the exact alignment. Give the block
/// its own escaper only if the column alignment turns out to matter on screen.
fn render_detail(i: usize, entry: &OpLogEntry) -> gpui::AnyElement {
    let text = oplog_panel::detail_lines(entry).join("\n");
    let selectable = text.len() <= SELECTABLE_DETAIL_MAX;
    let html = SharedString::from(kagi_domain::message::message_to_html(&text));
    div()
        .id(("oplog-row-detail", i))
        .flex()
        .flex_col()
        .w_full()
        .min_w(px(0.))
        .px_3()
        .py_1()
        .bg(rgb(theme().selected))
        .text_xs()
        .text_color(rgb(theme().text_sub))
        .whitespace_normal()
        // Don't let a drag-select on the detail text toggle the row closed.
        .on_mouse_down(MouseButton::Left, |_e, _w, cx| {
            cx.stop_propagation();
        })
        .when(!selectable, |d| {
            d.child(
                div()
                    .pb_1()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::OpLogDetailSelectionOff.t())),
            )
        })
        .child(
            gpui_component::text::TextView::html(
                SharedString::from(format!("oplog-detail-{}-{}", entry.id, i)),
                html,
            )
            .selectable(selectable),
        )
        .into_any_element()
}

/// #334: who ran the operation — the person in the GUI, the MCP server or the
/// CLI (ADR-0149). An agent's write stands out from a person's at a glance.
fn actor_badge(i: usize, actor: Actor) -> gpui::AnyElement {
    let (label, color) = match actor {
        Actor::Human => (i18n::oplog_panel::actor_human(), theme().text_muted),
        Actor::Mcp => ("MCP", theme().color_warning),
        Actor::Cli => ("CLI", theme().color_branch),
    };
    badge(
        format!("oplog-actor-{i}-{}", actor.as_str()),
        label.into(),
        color,
    )
}

/// #334: the working tree the operation ran in, by its directory name (the
/// full path is in the tooltip). Linked worktrees log under their own path.
fn worktree_badge(i: usize, entry: &OpLogEntry) -> gpui::AnyElement {
    let path = oplog_panel::entry_worktree(entry).to_string();
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let name = kagi_domain::text_safety::sanitize_control_bytes(&name);
    div()
        .id(("oplog-worktree", i))
        .tooltip(move |w, cx| gpui_component::tooltip::Tooltip::new(path.clone()).build(w, cx))
        .child(badge(
            format!("oplog-worktree-{i}-{name}"),
            name.into(),
            theme().text_sub,
        ))
        .into_any_element()
}

fn badge(probe: String, label: SharedString, color: u32) -> gpui::AnyElement {
    div()
        .relative()
        .flex_shrink_0()
        .max_w(theme_mod::scaled_px(140.))
        .ml(theme_mod::scaled_px(6.))
        .px_1()
        .rounded_sm()
        .border_1()
        .border_color(rgb(theme().selected))
        .text_xs()
        .text_color(rgb(color))
        .truncate()
        .child(label)
        .child(super::e2e::measure_inside(probe))
        .into_any_element()
}

/// #334: the reflog lines in the selected entry's window (ADR-0214). A line in
/// a second another operation of the worktree shares says so — it is shown,
/// never attributed.
fn render_reflog(i: usize, reflog: Option<&oplog_panel::ReflogDetail>) -> gpui::AnyElement {
    use oplog_panel::ReflogDetail;
    let section = div()
        .id(("oplog-row-reflog", i))
        .flex()
        .flex_col()
        .w_full()
        .px_3()
        .pb_1()
        .bg(rgb(theme().selected))
        .text_xs()
        .text_color(rgb(theme().text_sub));
    let muted = |text: String| {
        div()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from(text))
    };
    match reflog {
        None | Some(ReflogDetail::Loading) => {
            section.child(muted(i18n::oplog_panel::reflog_loading().into()))
        }
        Some(ReflogDetail::Unavailable(error)) => {
            section.child(muted(i18n::oplog_panel::reflog_unavailable(error)))
        }
        Some(ReflogDetail::Loaded { window, lines }) => {
            let heading = i18n::oplog_panel::reflog_heading(
                window.open_start,
                kagi_domain::oplog_reflog::UNBOUNDED_LOOKBACK_SECS,
            );
            let section = section.child(muted(heading));
            if lines.is_empty() {
                return section
                    .child(muted(i18n::oplog_panel::reflog_none().into()))
                    .into_any_element();
            }
            section.children(lines.iter().enumerate().map(|(n, (line, attribution))| {
                let ambiguous = *attribution == Attribution::Ambiguous;
                let text = format!(
                    "{}  {}→{}  {}",
                    line.refname,
                    line.old.get(..8).unwrap_or(&line.old),
                    line.new.get(..8).unwrap_or(&line.new),
                    kagi_domain::text_safety::sanitize_control_bytes(&line.message)
                );
                div()
                    .relative()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(div().truncate().child(SharedString::from(text)))
                    .when(ambiguous, |row| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .text_color(rgb(theme().color_warning))
                                .child(i18n::oplog_panel::reflog_ambiguous()),
                        )
                    })
                    .child(super::e2e::measure_inside(format!(
                        "oplog-reflog-{i}-{n}-{}",
                        if ambiguous { "ambiguous" } else { "within" }
                    )))
            }))
        }
    }
    .into_any_element()
}
