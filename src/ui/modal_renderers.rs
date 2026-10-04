//! Modal renderer functions extracted from modals.rs (ADR-0114 / Phase D).
//!
//! These are the per-modal `render_*` functions that build GPUI elements from
//! modal state structs. Extracted from modals.rs to bring it under the 800-LOC
//! target (AGENTS.md). modals.rs retains the modal state structs + ActiveModal enum.
//!
//! T-SPLIT-MODALS-001 / ADR-0116 Wave 3: the per-modal renderers were further
//! split into focused sibling modules by feature series. This file now retains
//! only the shared modal chrome/card builders (`modal_overlay`,
//! `render_plan_modal_card`, `render_input_plan_modal`) and re-exports the moved
//! renderers so the existing `use crate::ui::modal_renderers::*;` call sites
//! (render_overlay.rs) keep resolving without any caller change.

#![allow(clippy::too_many_arguments)]

use super::dialog_a11y::{apply_dialog, dialog_a11y, ConfirmStage, DialogHandler};
use super::i18n::Msg;
use super::modal_command::render_equivalent_command;
use super::modal_copy::{modal_copy_button, plan_clipboard_text};
use super::modal_shell::{
    modal_body, modal_card, modal_compact, modal_list_max_h, modal_list_panel, modal_prose_box,
    modal_recovery_section, note_path_list, note_path_list_element, section_open, MODAL_LIST_ROW_H,
    MODAL_W_MD,
};
use super::plan_card_rows::{render_commit_row, render_note_row};
use super::theme::{self, theme as current_theme};
use super::KagiApp;
use gpui::{div, prelude::*, rgb, Context, SharedString};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Icon, IconName, Sizable as _};
use kagi_git::{CommitId, OperationPlan};
use kagi_ui_core::i18n::{plan_note_text, plan_recovery_text, plan_title_text};

/// #462 section id for the shared plan card's recovery prose — the one block
/// on this card that is *supporting* detail. Warnings, blockers, the commit
/// preview and the error line are not collapsible: a confirmation may never
/// hide what it acts on or why it is refused. `KagiApp` owns the user's flip
/// (`modal_section_overrides`); this renderer owns the default.
const SECTION_PLAN_RECOVERY: &str = "plan-recovery";

/// Richer plan-card header (ADR pending: "richer popup cards", started with
/// Pull/Push per user request 2026-07-22, extended to every plan-confirmation
/// modal per user request 2026-07-23): an icon-badge circle in the op's
/// accent colour, shown left of the title. `None` (only a handful of fully
/// bespoke modals left today) keeps the plain text-only header pixel-identical
/// to before — this is additive, not a redesign of the shared card.
pub(crate) type PlanCardAccent = (ModalIcon, u32);

/// A modal-badge icon: either a real `gpui_component::IconName` (has a
/// matching SVG in gpui-component's own vendored icon set, so `Icon::new`
/// works), or a raw asset path for icons kagi bundles itself
/// (`assets/icons/*.svg`, e.g. `trash-2`/`square-pen`/`waypoints`) that have
/// no upstream `IconName` variant — same `Icon::default().path(...)` idiom
/// already used for the toolbar's Editor/Graph glyphs (`render_header.rs`).
#[derive(Clone)]
pub(crate) enum ModalIcon {
    Named(IconName),
    Path(&'static str),
}

impl From<IconName> for ModalIcon {
    fn from(name: IconName) -> Self {
        ModalIcon::Named(name)
    }
}

fn modal_icon_element(icon: ModalIcon) -> Icon {
    match icon {
        ModalIcon::Named(name) => Icon::new(name),
        ModalIcon::Path(path) => Icon::default().path(path),
    }
}

// T-SPLIT-MODALS-001 / ADR-0116 Wave 3: re-export the per-series renderers moved
// to focused sibling modules so the existing `modal_renderers::*` call sites keep
// resolving without touching the render_overlay callers (public paths preserved).
pub(crate) use super::modal_renderers_commit::*;
pub(crate) use super::modal_renderers_create::*;
pub(crate) use super::modal_renderers_destructive::*;
pub(crate) use super::modal_renderers_editor_fs::*;
pub(crate) use super::modal_renderers_misc::*;
pub(crate) use super::modal_renderers_plan::*;
pub(crate) use super::modal_renderers_stash::*;

/// Shared full-screen modal overlay chrome (T-SPLIT-HELPERS-001 / ADR-0116
/// Wave 3). Every modal renderer wrapped its card in the same two-layer
/// structure: a semi-transparent, occluding backdrop + a centred flex column
/// holding the card. This factors that DOM into one place so each renderer
/// only builds its card and calls `modal_overlay(card)`.
///
/// Produces exactly the tree the renderers built inline:
/// ```text
/// div.size_full.absolute.top_0.left_0
///   ├─ div.size_full.absolute.top_0.left_0.occlude.bg(modal_overlay).opacity(0.65)   // backdrop
///   └─ div.size_full.absolute.top_0.left_0.flex.flex_col.justify_center.items_center  // centring
///        └─ {card}
/// ```
/// Returns a `Div` (not `impl IntoElement`) so callers that additionally
/// attached a root-level `.on_key_down(..)` (the discard modal's ESC handler)
/// can keep chaining it — event handlers are stored independently of children,
/// so chaining order does not change the rendered tree. Callers that occluded
/// the card itself pass `card.occlude()` in the `card` slot.
pub(crate) fn modal_overlay(card: impl IntoElement) -> gpui::Div {
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        // Backdrop (dark, semi-transparent). `.occlude()` blocks mouse events
        // from reaching the UI beneath the modal (click-through bug).
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .occlude()
                .bg(rgb(current_theme().modal_overlay))
                .opacity(0.65),
        )
        // Card centred on top of the backdrop.
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(card),
        )
}

/// Shared plan comparison helpers. Summary strings can also carry operation
/// prose, so turn only known status forms into labeled chips and preserve all
/// other descriptions verbatim in fallback chips.
pub(crate) fn plan_state_chip(text: &str, icon: &'static str, color: u32) -> gpui::AnyElement {
    let (bg, border, foreground) = theme::badge_style(color);
    div()
        .flex_shrink_0()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .whitespace_nowrap()
        .px_1()
        .rounded_sm()
        .border_1()
        .border_color(gpui::rgba(border))
        .bg(gpui::rgba(bg))
        .text_color(rgb(foreground))
        .when(icon.is_empty(), |chip| {
            chip.child(SharedString::from("\u{2713}"))
        })
        .when(!icon.is_empty(), |chip| {
            chip.child(
                gpui::svg()
                    .path(icon)
                    .flex_shrink_0()
                    .w(theme::scaled_px(11.))
                    .h(theme::scaled_px(11.))
                    .text_color(rgb(foreground)),
            )
        })
        .child(SharedString::from(text.to_owned()))
        .into_any_element()
}

fn plan_head_chips(head: &str) -> Vec<gpui::AnyElement> {
    let t = current_theme();
    if let Some(rest) = head.strip_prefix("branch: ") {
        // A branch name contains no whitespace. Everything after it is plan
        // detail (tip, tracking, merge summary, etc.) and must remain visible.
        let name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        if name_end > 0 {
            let mut chips = vec![plan_state_chip(
                &rest[..name_end],
                "icons/laptop.svg",
                t.color_head,
            )];
            if name_end < rest.len() {
                chips.push(plan_state_chip(
                    &rest[name_end..],
                    "icons/circle-dot.svg",
                    t.color_head,
                ));
            }
            return chips;
        }
    }
    let (icon, color) = if head.starts_with("detached: ") {
        ("icons/git-compare.svg", t.color_head)
    } else if head.starts_with("unborn (") || head == "unborn" {
        ("icons/git-branch.svg", t.color_branch)
    } else if head == "HEAD" || head.starts_with("HEAD ") {
        ("icons/git-branch.svg", t.color_head)
    } else if head.starts_with("stash@{") || head.starts_with("stash: ") {
        ("icons/inbox.svg", t.color_tag)
    } else if head.starts_with("remote: ") {
        ("icons/cloud.svg", t.color_remote)
    } else {
        ("icons/circle-dot.svg", t.color_head)
    };
    vec![plan_state_chip(head, icon, color)]
}

pub(crate) fn plan_status_chips(dirty: &str) -> Vec<gpui::AnyElement> {
    let t = current_theme();
    if dirty == "clean" {
        return vec![plan_state_chip("", "", t.color_success)];
    }
    // Count-only forms are emitted by status_summary_display. In particular,
    // "3 conflicted file(s) (resolve in Conflict Mode)" is NOT just a count:
    // preserve the entire predicted description instead of losing the caveat.
    let mut parts = Vec::new();
    for part in dirty.split(", ") {
        let Some((count, kind)) = part.split_once(' ') else {
            return vec![plan_state_chip(dirty, "icons/circle-dot.svg", t.text_sub)];
        };
        if count.parse::<usize>().is_err() {
            return vec![plan_state_chip(dirty, "icons/circle-dot.svg", t.text_sub)];
        }
        let (icon, color) = match kind {
            "staged" => ("icons/plus.svg", t.color_success),
            "modified" => ("icons/square-pen.svg", t.color_warning),
            "untracked" => ("icons/file-text.svg", t.color_tag),
            "conflicted" => ("icons/git-merge.svg", t.color_blocker),
            _ => return vec![plan_state_chip(dirty, "icons/circle-dot.svg", t.text_sub)],
        };
        parts.push(plan_state_chip(part, icon, color));
    }
    parts
}

fn plan_state(head: &str, dirty: &str, id: &'static str, label: &str) -> gpui::AnyElement {
    let full = SharedString::from(format!("{label}: {head} [{dirty}]"));
    let (label_probe, chips_probe) = if label == "CURRENT" {
        ("plan-state-current-label", "plan-state-current-chips")
    } else {
        ("plan-state-predicted-label", "plan-state-predicted-chips")
    };
    div()
        .id(id)
        .role(gpui::Role::Group)
        .aria_label(full.clone())
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(full.clone()).build(window, cx)
        })
        .relative()
        .flex_1()
        .min_w(gpui::px(0.))
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .relative()
                .text_xs()
                .text_color(rgb(current_theme().text_label))
                .child(SharedString::from(label))
                .child(super::e2e::measure_inside(label_probe)),
        )
        .child(
            div()
                .id(chips_probe)
                .relative()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .overflow_x_scroll()
                .children(plan_head_chips(head))
                .children(plan_status_chips(dirty))
                .child(super::e2e::measure_inside(chips_probe)),
        )
        .when(cfg!(feature = "gui-e2e"), |state| {
            state.child(super::e2e::measure_inside(if label == "CURRENT" {
                "plan-state-current"
            } else {
                "plan-state-predicted"
            }))
        })
        .into_any_element()
}
pub(crate) fn render_current_predicted(
    plan: &OperationPlan,
    accent: Option<PlanCardAccent>,
) -> gpui::AnyElement {
    let color = accent
        .map(|(_, color)| color)
        .unwrap_or(current_theme().color_branch);
    div()
        .min_w(gpui::px(0.))
        .id("plan-state-comparison")
        .w_full()
        .rounded_md()
        .bg(gpui::rgba(theme::panel_style().0))
        .text_sm()
        .when(modal_compact(), |row| row.text_xs())
        .px_3()
        .py_2()
        .when(modal_compact(), |row| row.px_2().py_1())
        // Scroll the comparison as a unit on narrow windows; never let one
        // state's chip text steal width from its equal-width neighbor.
        .overflow_x_scroll()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .when(modal_compact(), |row| row.gap_1())
                .min_w(theme::scaled_px(480.))
                .w_full()
                .child(plan_state(
                    &plan.current.head,
                    &plan.current.dirty,
                    "plan-current-state",
                    "CURRENT",
                ))
                .child(
                    div()
                        .relative()
                        .flex_shrink_0()
                        .w(theme::scaled_px(24.))
                        .text_center()
                        .text_color(rgb(color))
                        .child(SharedString::from("\u{2192}"))
                        .when(cfg!(feature = "gui-e2e"), |arrow| {
                            arrow.child(super::e2e::measure_inside("plan-state-arrow"))
                        }),
                )
                .child(plan_state(
                    &plan.predicted.head,
                    &plan.predicted.dirty,
                    "plan-predicted-state",
                    "PREDICTED",
                )),
        )
        .into_any_element()
}

/// Shared scaffold for the plan-confirmation modals. Builds the cancel/confirm
/// `cx.listener` pair — each runs its action, then restores root focus and
/// notifies (the identical boilerplate that was hand-repeated in ~14 per-modal
/// renderers) — and delegates to [`render_plan_modal_card_styled`]. A
/// per-modal renderer supplies what differs: the plan/error/label, the
/// optional create-branch target, the icon-badge `accent` (every
/// plan-confirmation modal has one as of the user's 2026-07-23 "do the same
/// everywhere" request), and the two actions.
pub(crate) fn render_plan_modal_wrapper_styled(
    plan: std::sync::Arc<OperationPlan>,
    error: Option<SharedString>,
    confirm_label: impl Into<SharedString>,
    create_branch_target: Option<CommitId>,
    accent: Option<PlanCardAccent>,
    cancel_action: impl Fn(&mut KagiApp, &mut Context<KagiApp>) + 'static,
    confirm_action: impl Fn(&mut KagiApp, &mut Context<KagiApp>) + 'static,
    // #462: the user's section open/closed choices, borrowed from `KagiApp`
    // for this frame. Borrowed rather than read back through `cx`: the app is
    // already mutably borrowed while it renders.
    overrides: &std::collections::HashMap<&'static str, bool>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let stage = ConfirmStage::Single;
    render_plan_modal_wrapper_staged(
        plan,
        error,
        confirm_label,
        create_branch_target,
        accent,
        stage,
        cancel_action,
        confirm_action,
        overrides,
        cx,
    )
}

/// [`render_plan_modal_wrapper_styled`] for a two-stage card: `stage` tells
/// assistive technology whether the next Confirm runs the operation (#354).
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_plan_modal_wrapper_staged(
    plan: std::sync::Arc<OperationPlan>,
    error: Option<SharedString>,
    confirm_label: impl Into<SharedString>,
    create_branch_target: Option<CommitId>,
    accent: Option<PlanCardAccent>,
    stage: ConfirmStage,
    cancel_action: impl Fn(&mut KagiApp, &mut Context<KagiApp>) + 'static,
    confirm_action: impl Fn(&mut KagiApp, &mut Context<KagiApp>) + 'static,
    overrides: &std::collections::HashMap<&'static str, bool>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    render_plan_modal_wrapper_extra(
        plan,
        error,
        confirm_label,
        create_branch_target,
        accent,
        stage,
        None,
        cancel_action,
        confirm_action,
        overrides,
        cx,
    )
}

/// [`render_plan_modal_wrapper_staged`] with one card-specific section drawn
/// after the warnings (#334: the restore preview graph). Display only.
/// `Copy all` copies its `clipboard` text too (#883 review).
pub(crate) struct PlanCardExtra {
    pub element: gpui::AnyElement,
    pub clipboard: String,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_plan_modal_wrapper_extra(
    plan: std::sync::Arc<OperationPlan>,
    error: Option<SharedString>,
    confirm_label: impl Into<SharedString>,
    create_branch_target: Option<CommitId>,
    accent: Option<PlanCardAccent>,
    stage: ConfirmStage,
    extra: Option<PlanCardExtra>,
    cancel_action: impl Fn(&mut KagiApp, &mut Context<KagiApp>) + 'static,
    confirm_action: impl Fn(&mut KagiApp, &mut Context<KagiApp>) + 'static,
    overrides: &std::collections::HashMap<&'static str, bool>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel = cx.listener(move |this, _: &(), window, cx| {
        cancel_action(this, cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let confirm = cx.listener(move |this, _: &(), window, cx| {
        confirm_action(this, cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let cancel: DialogHandler = std::rc::Rc::new(move |w, a| cancel(&(), w, a));
    let confirm: DialogHandler = std::rc::Rc::new(move |w, a| confirm(&(), w, a));
    render_plan_modal_card_styled(
        plan,
        error,
        confirm_label,
        cancel,
        confirm,
        create_branch_target,
        accent,
        stage,
        extra,
        overrides,
        cx,
    )
    .into_any_element()
}

/// Icon-badge title row, shared by every plan-confirmation card and — as of
/// the Create Branch richer-card pass (user request 2026-07-23) — the
/// bespoke create-branch/create-worktree cards too. `None` renders the
/// original plain `text_xl` title, unconstrained; every modal that hasn't
/// opted into `accent` keeps this pixel-identical. `Some((icon, color))`
/// renders the 40x40 icon badge + `text_lg` semibold title, width-constrained
/// (`flex_1`/`min_w(0)`/`overflow_hidden`) so a long title wraps inside the
/// card instead of overflowing it (user report 2026-07-23).
pub(crate) fn render_modal_title_row(
    title: SharedString,
    accent: Option<PlanCardAccent>,
) -> gpui::AnyElement {
    match accent {
        Some((icon, color)) => {
            let (badge_bg, badge_border, _) = theme::badge_style(color);
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .when(modal_compact(), |row| row.gap_2())
                .child(
                    div()
                        .flex_shrink_0()
                        .w(theme::scaled_px(40.))
                        .h(theme::scaled_px(40.))
                        .when(modal_compact(), |badge| badge.size(theme::scaled_px(32.)))
                        .rounded_full()
                        .bg(gpui::rgba(badge_bg))
                        .border_1()
                        .border_color(gpui::rgba(badge_border))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            modal_icon_element(icon)
                                .with_size(gpui_component::Size::Size(theme::scaled_px(18.)))
                                .text_color(rgb(color)),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(gpui::px(0.))
                        .text_color(rgb(current_theme().text_main))
                        .text_lg()
                        .when(modal_compact(), |title| title.text_base())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .overflow_hidden()
                        .child(title),
                )
                .into_any_element()
        }
        None => div()
            .text_color(rgb(current_theme().text_main))
            .text_xl()
            .when(modal_compact(), |title| title.text_base())
            .child(title)
            .into_any_element(),
    }
}

/// Builds the plan-confirmation card. `accent` is `None` for the plain,
/// unchanged card (~14 modals); Pull/Push (user request 2026-07-22) pass an
/// icon-badge header via `Some(...)` for the richer treatment.
fn render_plan_modal_card_styled(
    plan: std::sync::Arc<OperationPlan>,
    error: Option<SharedString>,
    confirm_label: impl Into<SharedString>,
    cancel_handler: DialogHandler,
    confirm_handler: DialogHandler,
    create_branch_target: Option<CommitId>,
    accent: Option<PlanCardAccent>,
    stage: ConfirmStage,
    // #334: a card-specific element after the warnings (display only).
    extra: Option<PlanCardExtra>,
    // #462: see [`render_plan_modal_wrapper_styled`].
    overrides: &std::collections::HashMap<&'static str, bool>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // Accept either a `&'static str` (most modals) or a dynamic `String`/
    // `SharedString` (merge: `Merge <source> into <target>`, T-DNDMERGE-001).
    let confirm_label: SharedString = confirm_label.into();
    let has_blockers = !plan.blockers.is_empty();

    // ── Title (plain, or icon-badge header when `accent` is set) ──────
    let title_row = render_modal_title_row(
        SharedString::from(plan_title_text(&plan.title)),
        accent.clone(),
    );

    // ── Build modal card (#454) ─────────────────────────────
    // Fixed title + scrolling body + fixed button row. The card itself must
    // NOT scroll: a push plan with many preview commits used to grow past the
    // viewport, and capping the *card* with `overflow_y_scroll` only made the
    // confirm/cancel row reachable by scrolling to the bottom. `modal_card`
    // caps the height and the body takes the overflow, so the buttons stay on
    // screen no matter how long the plan is.
    //
    // The body does not scroll: each list/prose panel inside carries its own
    // capped scroll region instead (`modal_shell`'s rule — one scroller per
    // panel, never one inside another). A user asked for the commit list to be
    // "boxed and scrollable in the box" like the discard target list
    // (2026-09-06), which is the same shape the destructive cards use.
    // The whole dialog as text (title, current→predicted, notes, commits,
    // recovery): popup content had no way to be copied at all.
    let title_text = plan_title_text(&plan.title);
    let spec = dialog_a11y(
        &title_text,
        (!has_blockers).then_some(confirm_label.as_ref()),
        plan.destructive,
        stage,
    );
    let on_confirm = (!has_blockers).then(|| confirm_handler.clone());
    let card = apply_dialog(
        "plan-card",
        modal_card(MODAL_W_MD).id("plan-card"),
        spec,
        on_confirm,
        cancel_handler.clone(),
    )
    .child(
        div()
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_start()
            .gap_2()
            .child(div().flex_1().min_w(gpui::px(0.)).child(title_row))
            .child(modal_copy_button(
                "plan-card-copy",
                Msg::ModalCopyAll.t(),
                match &extra {
                    Some(extra) => format!(
                        "{}\n{}",
                        plan_clipboard_text(&plan, &plan.preview_commits),
                        extra.clipboard
                    ),
                    None => plan_clipboard_text(&plan, &plan.preview_commits),
                },
                cx,
            )),
    );

    // Fixed blocks stay flex_shrink_0-wrapped: only the panels give up height,
    // and without the guard flex would compress rows instead of scrolling
    // (same T027 bug class as the discard list).
    let mut body = modal_body().child(
        div()
            .flex_shrink_0()
            .child(render_current_predicted(&plan, accent.clone())),
    );

    // ── Warnings ─────────────────────────────────────────
    // #625: a warning carrying a path list renders as sentence + list, the
    // same way the blocker block below does. The dirty-Pull restore collision
    // is a *warning* (the user may still proceed), and it exists to name
    // paths — a comma wall inside the sentence is what it replaced.
    if !plan.warnings.is_empty() {
        let mut warn_col = div().flex().flex_col().gap_1();
        for (wi, w) in plan.warnings.iter().enumerate() {
            match note_path_list(w) {
                Some((summary, files)) => {
                    warn_col = warn_col.child(render_note_row(
                        format!("plan-warning-{wi}").into(),
                        false,
                        "\u{26a0}",
                        current_theme().color_warning,
                        &summary,
                        accent.is_some(),
                    ));
                    warn_col = warn_col.child(note_path_list_element(&files));
                }
                None => {
                    warn_col = warn_col.child(render_note_row(
                        format!("plan-warning-{wi}").into(),
                        false,
                        "\u{26a0}",
                        current_theme().color_warning,
                        &plan_note_text(w),
                        accent.is_some(),
                    ));
                }
            }
        }
        body = body.child(warn_col.flex_shrink_0());
    }
    if let Some(extra) = extra {
        body = body.child(div().flex_shrink_0().child(extra.element));
    }

    // ── Commits to push (T-HT-004) ────────────────────────
    // Shown only when preview_commits is non-empty (push plans).
    //
    // #454: was `take(total.min(10))` + an "… and N more" line — the count was
    // honest but the remaining commits were unreachable. Every row is rendered
    // now, inside the shared list panel with its own capped scroll box, so the
    // list reads as a box and scrolls in place (user request 2026-09-06)
    // instead of stretching the card.
    //
    // Plain rows, not a `uniform_list`: the producer caps the preview at 100
    // (`build_push_preview_for_oid`, `MAX_PREVIEW`), so the row count is
    // bounded and virtualization would buy nothing here.
    if !plan.preview_commits.is_empty() {
        let total = plan.preview_commits.len();
        // No row gap: the row pitch must equal MODAL_LIST_ROW_H exactly or the
        // height ceiling (rows x ROW_H) would cut a row short.
        let mut list = div()
            .id("plan-commit-list")
            .flex()
            .flex_col()
            .min_h(gpui::px(0.))
            .max_h(modal_list_max_h(total))
            .overflow_y_scroll();
        #[cfg(feature = "gui-e2e")]
        {
            list = list
                .relative()
                .child(super::modal_shell::modal_probe("modal-target-list"));
        }
        for entry in &plan.preview_commits {
            let line: String = entry.chars().take(72).collect();
            let row = div()
                .flex_shrink_0()
                .h(theme::scaled_px(MODAL_LIST_ROW_H))
                .flex()
                .items_center()
                .overflow_hidden()
                // One line per commit: a long summary used to wrap and
                // overrun this fixed-height row into the next one.
                .whitespace_nowrap()
                .text_ellipsis()
                .child(render_commit_row(&line, accent.clone()));
            #[cfg(feature = "gui-e2e")]
            let row = row
                .relative()
                .child(super::modal_shell::modal_probe(format!(
                    "modal-commit-{entry}"
                )));
            list = list.child(row);
        }
        body = body.child(modal_list_panel(
            SharedString::from("Commits to push"),
            total,
            Some(("plan-commits-copy", plan.preview_commits.join("\n"))),
            None,
            list.into_any_element(),
            cx,
        ));
    }

    // ── Blockers ──────────────────────────────────────────
    // #454: a blocker that carries a *list* (checkout overlap: "40 file(s)
    // that the target also modifies") renders as sentence + list, not as a
    // comma wall inside the sentence. 40 paths joined into prose filled the
    // whole card, which is the wall the audit set out to remove.
    if !plan.blockers.is_empty() {
        let mut block_col = div().flex().flex_col().gap_1();
        for (bi, b) in plan.blockers.iter().enumerate() {
            match note_path_list(b) {
                Some((summary, files)) => {
                    block_col = block_col.child(render_note_row(
                        format!("plan-blocker-{bi}").into(),
                        true,
                        "\u{2717}",
                        current_theme().color_blocker,
                        &summary,
                        accent.is_some(),
                    ));
                    block_col = block_col.child(note_path_list_element(&files));
                }
                None => {
                    block_col = block_col.child(render_note_row(
                        format!("plan-blocker-{bi}").into(),
                        true,
                        "\u{2717}",
                        current_theme().color_blocker,
                        &plan_note_text(b),
                        accent.is_some(),
                    ));
                }
            }
        }
        body = body.child(block_col.flex_shrink_0());
    }

    // ── Recovery (supporting detail) ──────────────────────
    // Capped and scrollable in place: the body no longer scrolls, so a long
    // recovery text would otherwise be clipped by the card (the recovery
    // instructions are the reason a destructive operation is allowed at all).
    //
    // Fold only supporting recovery. Preserve explicit choices across resizes;
    // untouched normal-height cards keep their existing plain prose.
    let recovery_text = plan_recovery_text(plan.recovery.as_ref());
    if !recovery_text.is_empty() {
        let prose = move || {
            modal_prose_box(
                "plan-recovery-scroll",
                match accent {
                    Some((_, color)) => render_recovery_box(&recovery_text, color),
                    None => div()
                        .text_xs()
                        .text_color(rgb(current_theme().text_muted))
                        .child(SharedString::from(recovery_text))
                        .into_any_element(),
                },
            )
            .into_any_element()
        };
        let compact = modal_compact();
        if compact || overrides.contains_key(SECTION_PLAN_RECOVERY) {
            let open = section_open(overrides, SECTION_PLAN_RECOVERY, !compact);
            body = body.child(modal_recovery_section(
                SECTION_PLAN_RECOVERY,
                open,
                // Built only while open, so a folded section costs nothing.
                open.is_open().then(prose),
                cx,
            ));
        } else {
            body = body.child(prose());
        }
    }

    // Kagi executes the plan through its backend; the CLI spelling is
    // reference text, collapsed by default and separately copyable.
    if let Some(cmd) = plan.equivalent_command.as_deref() {
        body = body.child(render_equivalent_command(cmd, None, overrides, cx));
    }

    // ── Error message (preflight / execute failure) ───────
    if let Some(err) = &error {
        body = body.child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    // ── Buttons ───────────────────────────────────────────
    let mut button_row = div()
        .flex()
        .flex_row()
        .gap_2()
        .justify_end()
        // Cancel button (always present — safe default). #462: measured, so a
        // scenario can assert the footer's own controls are on screen rather
        // than inferring it from the card.
        .child(super::e2e::measure_control(
            "plan-cancel",
            Button::new("plan-cancel")
                .label(Msg::PlanCancel.t())
                .ghost()
                .small()
                .on_click(move |_, w, a| cancel_handler(w, a)),
        ));

    if let Some(commit_id) = create_branch_target {
        let create_handler = cx.listener(move |this, _event: &gpui::ClickEvent, window, cx| {
            this.cancel_modal();
            this.open_create_branch_modal(commit_id.clone(), cx);
            if let Some(fh) = this.root_focus.clone() {
                window.focus(&fh, cx);
            }
            cx.notify();
        });
        button_row = button_row.child(
            Button::new("plan-create-branch")
                .label("Create branch here...")
                .small()
                .on_click(create_handler),
        );
    }

    // Checkout button: only shown when there are no blockers.
    if !has_blockers {
        let button = Button::new("plan-confirm")
            .label(confirm_label)
            .primary()
            .small()
            .on_click(move |_, w, a| confirm_handler(w, a));
        #[cfg(feature = "gui-e2e")]
        let button = {
            // Keep the measurement layer behind the real button and anchor it
            // to the wrapper, independent of absolute static-position layout.
            div()
                .relative()
                .child(
                    gpui::canvas(
                        move |bounds, window, _| {
                            crate::ui::e2e::record_confirm_bounds(
                                window.window_handle().window_id(),
                                bounds,
                            );
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
                // #462: the same wrapper carries the named probe, so a
                // scenario reads this button's real bounds instead of the
                // window-wide "last confirm drawn" slot above.
                .child(super::modal_shell::modal_probe("plan-confirm"))
                .child(button)
        };
        button_row = button_row.child(button);
    }

    // #462: the footer is the block a compact card must never push off the
    // bottom, so it is measured as a whole. `relative` + an absolute,
    // zero-layout probe: the row keeps its own flex sizing.
    #[cfg(feature = "gui-e2e")]
    let button_row = button_row
        .relative()
        .child(super::modal_shell::modal_probe("modal-footer"));

    let card = card.child(body).child(button_row.flex_shrink_0());

    // ── Full-screen overlay wrapper (shared chrome, T-SPLIT-HELPERS-001) ──
    modal_overlay(card)
}
