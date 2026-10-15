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
//!    for the diff panes (`render_helpers::render_diff_list`). The panel seeds
//!    the count once and splices new receipts/retention cuts, preserving the
//!    measured rows and scrolled focus. Detail lines are free to wrap (no
//!    truncation) — a 200-char `error:` is readable instead of clipped. The
//!    summary uses two scaled lines, reserving named status and Copy first.
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
//! the list is built; activation handlers run after the entity borrow ends.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, rgb, App, ClickEvent, Context, Element as _, Entity, InteractiveElement, IntoElement,
    MouseButton, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::button::Button;
use gpui_component::{Disableable as _, Sizable as _};
use kagi_domain::oplog_reflog::Attribution;
use kagi_git::oplog::{Actor, OpLogEntry, OpOutcome};

use super::i18n::{self, Msg};
use super::render_helpers::with_vertical_scrollbar;
use super::theme::theme;
use super::{format_hms, oplog_panel, theme as theme_mod};
use kagi_ui_core::i18n::oplog_panel::OplogPanelMsg as P;

/// Two collapsed summary lines: required status/Copy, then quiet metadata.
const SUMMARY_ROW_H: f32 = 44.;

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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entry_count = self.len();
        #[cfg(feature = "gui-e2e")]
        oplog_panel::keyboard::clear();

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
        // Constructors and push() keep the count in sync through ListState::splice,
        // retaining measured row heights and a scrolled keyboard reading anchor.
        // W12-GCADOPT (§2.10): scrollbar overlay on the Operation Log list.
        // gpui-component implements `ScrollbarHandle` for `ListState` directly.
        let scrollbar_handle = scroll_handle.clone();
        let rows = std::rc::Rc::new(self.keyboard_rows(window, cx));
        let list_container = rows.list(rows_container());

        // Snapshot before the closure — capped at OP_ENTRIES_MAX (200), and the
        // previous `uniform_list` cloned the same vec on every range callback.
        let entries: Vec<OpLogEntry> = self.entries().iter().cloned().collect();
        let expanded = self.expanded();
        let reflog = self.reflog_detail().cloned();
        let entity = cx.entity();

        let oplog_list = gpui::list(scroll_handle, move |i, _window, _cx| match entries.get(i) {
            Some(entry) => {
                let detail = (expanded == Some(i)).then_some(reflog.as_ref());
                render_row(i, entry, detail, &entity, &rows)
            }
            None => div().into_any_element(),
        })
        .flex_1()
        .min_h(px(0.))
        .bg(rgb(theme().panel));

        super::list_a11y::plain_list(
            "oplog-list",
            list_container.child(with_vertical_scrollbar(
                "oplog-list-scroll",
                &scrollbar_handle,
                oplog_list,
                true,
            )),
            Msg::BottomTabOperationLog.t(),
        )
        .into_any_element()
    }
}

fn rows_container() -> gpui::Stateful<gpui::Div> {
    div()
        .id("oplog-list")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
}

/// One op-log row: a two-line summary plus, when expanded (`detail` is `Some`),
/// the selectable detail block underneath with the entry's reflog section.
fn render_row(
    i: usize,
    entry: &OpLogEntry,
    detail: Option<Option<&oplog_panel::ReflogDetail>>,
    entity: &Entity<oplog_panel::OpLogPanel>,
    rows: &std::rc::Rc<super::keyboard_nav::RowList>,
) -> gpui::AnyElement {
    let stable_key = SharedString::from(oplog_panel::keyboard::entry_key(entry));
    // `bg_row_alt` is the zebra token; this used to be `panel`, which only
    // looked striped in themes whose chrome happens to differ from the base.
    let row_bg = if i.is_multiple_of(2) {
        theme().bg_row_alt
    } else {
        theme().bg_base
    };

    div()
        .id(stable_key.clone())
        // Issue #468 E2E: the row's real laid-out height is read back through
        // `ListState::bounds_for_item`, which proves an expanded row GROWS
        // instead of overflowing onto the next one. So never pin a height here.
        .flex()
        .flex_col()
        .w_full()
        .bg(rgb(row_bg))
        // Keep selectable detail outside the disclosure's activation target.
        .child(render_summary(
            i,
            entry,
            detail.is_some(),
            entity,
            rows,
            stable_key,
        ))
        .when_some(detail, |row, reflog| {
            let row = row.child(render_detail(i, entry));
            let row = row.when_some(oplog_panel::recovery_lines(entry), |row, lines| {
                row.child(render_recovery(i, entry, lines))
            });
            let row = match &entry.ref_moves {
                Some(moves) => row.child(render_ref_moves(i, moves)),
                None => row.child(render_reflog(i, reflog)),
            };
            row.child(render_restore_actions(i, entry, entity))
        })
        .into_any_element()
}

fn render_summary(
    i: usize,
    entry: &OpLogEntry,
    expanded: bool,
    entity: &Entity<oplog_panel::OpLogPanel>,
    rows: &std::rc::Rc<super::keyboard_nav::RowList>,
    stable_key: SharedString,
) -> gpui::AnyElement {
    let outcome = SharedString::from(oplog_panel::outcome_summary(&entry.outcome));
    let operation = SharedString::from(kagi_domain::text_safety::sanitize_control_bytes(
        &kagi_ui_core::i18n::op::user_text::operation_label(&entry.op),
    ));
    let time = SharedString::from(format_hms(entry.timestamp));
    let identity = kagi_ui_core::i18n::oplog_panel::entry_disclosure_name(
        entry.id, &time, &operation, &outcome,
    );
    let copy_name = kagi_ui_core::i18n::oplog_panel::entry_copy_name(&identity);
    let key = oplog_panel::EntryKey::of(entry);
    let toggle_entity = entity.clone();
    let toggle_rows = rows.clone();
    rows.row(
        i,
        oplog_panel::keyboard::disclosure(
            div().id(stable_key.clone()),
            i,
            rows.len(),
            identity.into(),
            expanded,
        ),
    )
    .flex()
    .flex_col()
    .items_center()
    .px(super::keyboard_nav::inset(12.))
    .h(theme_mod::scaled_px(SUMMARY_ROW_H))
    .hover(|s| s.bg(rgb(theme().surface)).cursor_pointer())
    .on_click(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
        toggle_rows.focus_row(i, window, cx);
        toggle_entity.update(cx, |this, cx| {
            this.select_entry(&key, cx);
            cx.notify();
        });
    })
    .child(
        div()
            .flex()
            .items_center()
            .w_full()
            .min_w(px(0.))
            .h(theme_mod::scaled_px(22.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(theme_mod::scaled_px(theme_mod::FILE_TREE_FONT_PX))
                    .text_color(rgb(theme().text_sub))
                    .truncate()
                    .child(operation)
                    .when(cfg!(feature = "gui-e2e"), |label| {
                        label.child(super::e2e::measure_inside(format!("oplog-operation-{i}")))
                    }),
            )
            .child(render_status(i, &entry.outcome))
            .child(render_copy(
                i, entry, expanded, copy_name, entity, stable_key,
            )),
    )
    .child(
        div()
            .flex()
            .items_center()
            .w_full()
            .min_w(px(0.))
            .h(theme_mod::scaled_px(22.))
            .children(summary_fields(i, entry, time, outcome)),
    )
    .into_any_element()
}

fn render_status(i: usize, outcome: &OpOutcome) -> gpui::AnyElement {
    let (kind, message, color) = match outcome {
        OpOutcome::Success { .. } => ("Success", P::StatusSuccess, theme().color_success),
        OpOutcome::Failed { .. } => ("Failed", P::StatusFailed, theme().color_blocker),
        OpOutcome::Unknown { .. } => ("Unknown", P::StatusUnknown, theme().color_blocker),
        OpOutcome::Partial { .. } => ("Partial", P::StatusPartial, theme().color_warning),
        OpOutcome::Refused { .. } => ("Refused", P::StatusRefused, theme().color_warning),
    };
    let label = Msg::OplogPanel(message).t();
    div()
        .id(("oplog-status", i))
        .role(gpui::Role::Status)
        .aria_label(label)
        .flex_shrink_0()
        .ml(theme_mod::scaled_px(6.))
        .text_size(theme_mod::scaled_px(theme_mod::FILE_TREE_FONT_PX))
        .text_color(rgb(color))
        .child(label)
        .when(cfg!(feature = "gui-e2e"), |status| {
            status.child(super::e2e::measure_inside(format!(
                "oplog-status-{i}-{kind}"
            )))
        })
        .into_any_element()
}

fn summary_fields(
    i: usize,
    entry: &OpLogEntry,
    time: SharedString,
    outcome: SharedString,
) -> [gpui::AnyElement; 4] {
    let color = match &entry.outcome {
        OpOutcome::Success { .. } => theme().color_success,
        OpOutcome::Partial { .. } | OpOutcome::Refused { .. } => theme().color_warning,
        OpOutcome::Unknown { .. } | OpOutcome::Failed { .. } => theme().color_blocker,
    };
    [
        div()
            .w(theme_mod::scaled_px(60.))
            .flex_shrink_0()
            .text_size(theme_mod::scaled_px(theme_mod::FILE_TREE_FONT_PX))
            .text_color(rgb(theme().text_muted))
            .child(time)
            .when(cfg!(feature = "gui-e2e"), |label| {
                label.child(super::e2e::measure_inside(format!("oplog-time-{i}")))
            })
            .into_any_element(),
        actor_badge(i, entry.actor),
        worktree_badge(i, entry),
        div()
            .flex_1()
            .min_w(px(0.))
            .ml(theme_mod::scaled_px(6.))
            .text_size(theme_mod::scaled_px(theme_mod::FILE_TREE_FONT_PX))
            .text_color(rgb(color))
            .truncate()
            .child(outcome)
            .when(cfg!(feature = "gui-e2e"), |label| {
                label.child(super::e2e::measure_inside(format!("oplog-outcome-{i}")))
            })
            .into_any_element(),
    ]
}

fn render_copy(
    i: usize,
    entry: &OpLogEntry,
    expanded: bool,
    name: String,
    entity: &Entity<oplog_panel::OpLogPanel>,
    stable_key: SharedString,
) -> gpui::AnyElement {
    let copy_entity = entity.clone();
    let key = oplog_panel::EntryKey::of(entry);
    // Reuse the quiet-copy focus owner rather than the kit's low-alpha outer
    // ring; a second hover style on the pinned Button also conflicts with its own.
    let button = super::keyboard_nav::focusable(oplog_panel::keyboard::copy_button(
        div().id(stable_key),
        i,
        name,
    ))
    .tab_stop(expanded)
    .w(theme_mod::scaled_px(19.))
    .h(theme_mod::scaled_px(19.))
    .flex_shrink_0()
    .ml(theme_mod::scaled_px(6.))
    .p(super::keyboard_nav::inset(4.))
    .rounded_sm()
    .cursor_pointer()
    .opacity(0.55)
    .hover(|style| style.bg(rgb(theme().selected)).opacity(1.0))
    .focus_visible(|style| style.opacity(1.0).border_color(rgb(theme().ui_accent())))
    .tooltip(|w, cx| gpui_component::tooltip::Tooltip::new(Msg::OpLogCopyEntry.t()).build(w, cx))
    // Swallow the press as well, so it cannot start detail-text selection.
    .on_mouse_down(MouseButton::Left, |_e, _w, cx| cx.stop_propagation())
    .on_click(move |_: &ClickEvent, _window: &mut Window, cx: &mut App| {
        cx.stop_propagation();
        copy_entity.update(cx, |this, cx| this.copy_entry_key(&key, cx));
    })
    .child(
        gpui::svg()
            .path("icons/copy.svg")
            .w(theme_mod::scaled_px(11.))
            .h(theme_mod::scaled_px(11.))
            .text_color(rgb(theme().text_sub)),
    );
    super::e2e::measure_control(format!("oplog-copy-{i}"), button)
}

/// The expanded detail block: every line of the entry, soft-wrapped (never
/// truncated — the row is variable-height now) and drag-selectable. Leading /
/// aligned spaces collapse on screen; the row's copy button keeps them.
fn render_detail(i: usize, entry: &OpLogEntry) -> gpui::AnyElement {
    let text = oplog_panel::base_detail_lines(entry).join("\n");
    let selectable = text.len() <= SELECTABLE_DETAIL_MAX;
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
        .when(!selectable, |d| {
            d.child(
                div()
                    .pb_1()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::OpLogDetailSelectionOff.t())),
            )
        })
        .child(
            selectable_text(format!("oplog-detail-{}-{}", entry.id, i), &text)
                .selectable(selectable),
        )
        .into_any_element()
}

/// Text of the expanded block, selectable by drag and copied with ⌘C. The
/// selection is window-wide, so a drag across several of these copies them as
/// one text. `message_to_html` keeps each line its own paragraph (the pinned
/// `TextView` drops a `<br>` inside one, #908 / #946); the gap is 0 so the
/// lines sit together. The pinned `TextView` copies paragraphs with a blank
/// line between them; the row's copy button gives the exact entry.
fn selectable_text(id: String, text: &str) -> gpui_component::text::TextView {
    gpui_component::text::TextView::html(
        SharedString::from(id),
        SharedString::from(kagi_domain::message::message_to_html(text)),
    )
    .style(gpui_component::text::TextViewStyle {
        paragraph_gap: gpui::rems(0.),
        ..Default::default()
    })
    .selectable(true)
}

/// Separate plan guidance from execution facts in both the visual hierarchy
/// and the accessibility tree; its name follows the current language.
fn render_recovery(i: usize, entry: &OpLogEntry, lines: Vec<String>) -> gpui::AnyElement {
    let text = lines.join("\n");
    let selectable = text.len() <= SELECTABLE_DETAIL_MAX;
    let view =
        selectable_text(format!("oplog-recovery-{}-{i}", entry.id), &text).selectable(selectable);
    super::dialog_a11y::apply_group(
        "oplog-recovery",
        div()
            .id(("oplog-recovery", i))
            .flex()
            .flex_col()
            .w_full()
            .min_w(px(0.))
            .px_3()
            .pb_1()
            .bg(rgb(theme().selected))
            .text_xs()
            .text_color(rgb(theme().text_sub))
            .whitespace_normal()
            .child(super::e2e::measure_inside(format!("oplog-recovery-{i}"))),
        SharedString::from(Msg::OpLogRecovery.t()),
    )
    .aria_description(SharedString::from(text))
    .child(view)
    .into_any_element()
}

/// #334: who ran the operation — the person in the GUI, the MCP server or the
/// CLI (ADR-0149). An agent's write stands out from a person's at a glance.
fn actor_badge(i: usize, actor: Actor) -> gpui::AnyElement {
    let (label, color) = match actor {
        Actor::Human => (Msg::OplogPanel(P::ActorHuman).t(), theme().text_muted),
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
        .min_w(px(0.))
        .max_w(theme_mod::scaled_px(140.))
        .flex_shrink(1.)
        .overflow_hidden()
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
        .min_w(px(0.))
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
    let muted = |id: &str, text: String| {
        div()
            .text_color(rgb(theme().text_muted))
            .child(selectable_text(format!("oplog-reflog-{id}-{i}"), &text))
    };
    match reflog {
        None | Some(ReflogDetail::Loading) => section.child(muted(
            "loading",
            Msg::OplogPanel(P::ReflogLoading).t().into(),
        )),
        Some(ReflogDetail::Unavailable(error)) => section.child(muted(
            "unavailable",
            i18n::oplog_panel::reflog_unavailable(error),
        )),
        Some(ReflogDetail::Loaded { window, lines }) => {
            let heading = i18n::oplog_panel::reflog_heading(
                window.open_start,
                kagi_domain::oplog_reflog::UNBOUNDED_LOOKBACK_SECS,
            );
            let section = section.child(muted("heading", heading));
            if lines.is_empty() {
                return section
                    .child(muted("none", Msg::OplogPanel(P::ReflogNone).t().into()))
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
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(selectable_text(format!("oplog-reflog-line-{i}-{n}"), &text)),
                    )
                    .when(ambiguous, |row| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .text_color(rgb(theme().color_warning))
                                .child(Msg::OplogPanel(P::ReflogAmbiguous).t()),
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

/// #334 slice 2a: the refs the operation recorded moving, by OID — the exact
/// record (ADR-0214 §4), labelled apart from the time-window estimate.
fn render_ref_moves(i: usize, moves: &[kagi_domain::ref_moves::RefMove]) -> gpui::AnyElement {
    let section = div()
        .id(("oplog-row-refmoves", i))
        .relative()
        .flex()
        .flex_col()
        .w_full()
        .px_3()
        .pb_1()
        .bg(rgb(theme().selected))
        .text_xs()
        .text_color(rgb(theme().text_sub))
        .child(
            div()
                .text_color(rgb(theme().color_success))
                .child(selectable_text(
                    format!("oplog-recorded-heading-{i}"),
                    Msg::OplogPanel(P::RecordedHeading).t(),
                )),
        )
        .child(super::e2e::measure_inside(format!("oplog-recorded-{i}")));
    if moves.is_empty() {
        return section
            .child(
                div()
                    .text_color(rgb(theme().text_muted))
                    .child(selectable_text(
                        format!("oplog-recorded-none-{i}"),
                        Msg::OplogPanel(P::RecordedNone).t(),
                    )),
            )
            .into_any_element();
    }
    section
        .children(moves.iter().enumerate().map(|(n, m)| {
            let text =
                super::oplog_panel::ref_move_text(m, false, Msg::OplogPanel(P::RefAbsent).t());
            div()
                .relative()
                .child(selectable_text(
                    format!("oplog-refmove-text-{i}-{n}"),
                    &text,
                ))
                .child(super::e2e::measure_inside(format!(
                    "oplog-refmove-{i}-{n}-{}",
                    m.refname
                )))
        }))
        .into_any_element()
}

/// #334 slice 2b: the selected row's "revert this operation…" and "restore to
/// this point…" buttons. They only ask the app to plan (the card confirms);
/// an entry without recorded ref moves gets them disabled, with the reason.
/// Real buttons, so they read as buttons on the block's own background.
fn render_restore_actions(
    i: usize,
    entry: &OpLogEntry,
    entity: &Entity<oplog_panel::OpLogPanel>,
) -> gpui::AnyElement {
    let restore_enabled = oplog_panel::restorable(entry);
    let revert_enabled = oplog_panel::revertible(entry);
    let reason = if !restore_enabled {
        Some(Msg::OplogPanel(P::RestoreUnavailable).t())
    } else if !revert_enabled {
        Some(Msg::OplogPanel(P::RevertNoMoves).t())
    } else {
        None
    };
    let button = |kind: &'static str,
                  icon: &'static str,
                  label: &'static str,
                  enabled: bool,
                  op: kagi_git::Operation| {
        let entity = entity.clone();
        let state = if enabled { "enabled" } else { "disabled" };
        div()
            .relative()
            .child(super::button_style::named_button_with_description(
                Button::new(SharedString::from(format!("oplog-{kind}-{i}")))
                    .icon(gpui_component::Icon::empty().path(icon))
                    .label(label)
                    .small()
                    .disabled(!enabled)
                    .on_click(move |_: &ClickEvent, _w: &mut Window, cx: &mut App| {
                        if enabled {
                            let op = op.clone();
                            entity.update(cx, |_, cx| {
                                cx.emit(oplog_panel::OpLogPanelEvent::Restore(op))
                            });
                        }
                    }),
                label.into(),
                None,
                (!enabled).then(|| reason.unwrap_or_default().into()),
                !enabled,
            ))
            .child(super::e2e::measure_inside(format!(
                "oplog-{kind}-{i}-{state}"
            )))
    };
    let entry_id = entry.id;
    div()
        .id(("oplog-row-restore", i))
        .flex()
        .flex_col()
        .gap_1()
        .w_full()
        .px_3()
        .pb_2()
        .bg(rgb(theme().selected))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .flex_wrap()
                .child(button(
                    "revert",
                    "icons/undo-2.svg",
                    Msg::OplogPanel(P::RevertButton).t(),
                    revert_enabled,
                    kagi_git::Operation::OpRevert { entry_id },
                ))
                .child(button(
                    "restore",
                    "icons/history.svg",
                    Msg::OplogPanel(P::RestoreButton).t(),
                    restore_enabled,
                    kagi_git::Operation::RestoreToPoint { entry_id },
                )),
        )
        .when_some(reason, |d, reason| {
            d.child(
                div()
                    .id(("oplog-revert-reason", i))
                    .role(gpui::Role::Status)
                    .aria_label(reason)
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(reason)
                    .when(cfg!(feature = "gui-e2e"), |label| {
                        label.child(super::e2e::measure_inside(format!(
                            "oplog-revert-reason-{i}"
                        )))
                    }),
            )
        })
        .into_any_element()
}
