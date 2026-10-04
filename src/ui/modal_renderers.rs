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

use super::dialog_a11y::{apply_dialog, apply_group, dialog_a11y, ConfirmStage, DialogHandler};
use super::i18n::Msg;
use super::modal_command::{render_equivalent_command, render_recovery_commands};
use super::modal_copy::{modal_copy_button, plan_clipboard_text};
use super::modal_shell::{
    modal_body, modal_card, modal_compact, modal_list_max_h, modal_list_panel, note_path_list,
    note_path_list_element, MODAL_LIST_ROW_H, MODAL_W_MD,
};
use super::plan_card_rows::{render_commit_row, render_note_row};
use super::theme::{self, theme as current_theme};
use super::{KagiApp, MONO_FONT};
use gpui::{div, prelude::*, rgb, Context, SharedString};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Icon, IconName, Sizable as _};
use kagi_domain::plan_note::ShellKind;
use kagi_git::{CommitId, OperationPlan};
use kagi_ui_core::i18n::{plan::plan_heading_text, plan_note_text, plan_recovery_text, plan_title_text};

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

/// The same quiet status pill used by Stash Push and every other plan card.
fn plan_status_chip(text: &str, kind: &str) -> gpui::AnyElement {
    let t = current_theme();
    let (color, fill_alpha, text_alpha) = match kind {
        "staged" | "clean" => (t.color_success, 0x33, 0xff),
        "modified" => (t.change_modified, 0x22, if t.dark { 0xe0 } else { 0xff }),
        "conflicted" => (t.color_blocker, 0x33, 0xff),
        _ => (t.text_main, 0x13, 0xcc),
    };
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .h(theme::scaled_px(if modal_compact() { 20. } else { 24. }))
        .px_2()
        .rounded(theme::scaled_px(6.))
        .bg(gpui::rgba((color << 8) | fill_alpha))
        .font_family(MONO_FONT)
        .text_xs()
        .text_color(gpui::rgba((color << 8) | text_alpha))
        .child(SharedString::from(text.to_owned()))
        .into_any_element()
}

/// Count-only statuses can be split; operation prose must remain one verbatim chip.
fn plan_status_chips(dirty: &str) -> Vec<gpui::AnyElement> {
    if dirty == "clean" {
        // The one status the card names itself; counts come from the plan.
        return vec![plan_status_chip(Msg::PlanStateClean.t(), "clean")];
    }
    let mut chips = Vec::new();
    for part in dirty.split(", ") {
        let Some((count, kind)) = part.split_once(' ') else {
            return vec![plan_status_chip(dirty, "other")];
        };
        if count.parse::<usize>().is_err()
            || !matches!(kind, "staged" | "modified" | "untracked" | "conflicted")
        {
            return vec![plan_status_chip(dirty, "other")];
        }
        chips.push(plan_status_chip(part, kind));
    }
    chips
}

fn plan_state(
    head: &str,
    dirty: &str,
    label: &'static str,
    id: &'static str,
    label_id: &'static str,
    head_id: &'static str,
) -> gpui::AnyElement {
    let full = SharedString::from(format!("{label}: {head} [{dirty}]"));
    apply_group(id, div().id(id), full.clone())
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(full.clone()).build(window, cx)
        })
        .relative()
        .flex_shrink_0()
        .flex()
        .flex_row()
        .items_start()
        .gap_3()
        .px_3()
        .py_2()
        .when(modal_compact(), |row| row.px_2().py_1())
        .child(
            div()
                .w(theme::scaled_px(64.))
                .flex_shrink_0()
                .whitespace_nowrap()
                .pt(theme::scaled_px(3.))
                .text_xs()
                .text_color(rgb(current_theme().text_label))
                .child(SharedString::from(label))
                .when(cfg!(feature = "gui-e2e"), |label| {
                    label.relative().child(super::e2e::measure_inside(label_id))
                }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(gpui::px(0.))
                .gap(theme::scaled_px(6.))
                .child(
                    div()
                        .relative()
                        .font_family(MONO_FONT)
                        .text_size(theme::scaled_px(13.))
                        .line_height(gpui::relative(1.5))
                        .text_color(rgb(current_theme().text_main))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(head.to_owned()))
                        .when(cfg!(feature = "gui-e2e"), |head| {
                            head.child(super::e2e::measure_inside(head_id))
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap(theme::scaled_px(6.))
                        .children(plan_status_chips(dirty)),
                ),
        )
        .when(cfg!(feature = "gui-e2e"), |row| {
            row.child(super::e2e::measure_inside(id))
        })
        .into_any_element()
}

pub(crate) fn render_current_predicted(plan: &OperationPlan) -> gpui::AnyElement {
    div()
        .id("plan-state-comparison")
        .relative()
        .w_full()
        .min_w(gpui::px(0.))
        .rounded(theme::scaled_px(10.))
        .bg(rgb(current_theme().bg_base))
        .border_1()
        .border_color(gpui::rgba(theme::panel_style().1))
        .overflow_x_scroll()
        .flex()
        .flex_col()
        .child(plan_state(
            &plan.current.head,
            &plan.current.dirty,
            Msg::InputStashCurrent.t(),
            "plan-state-current",
            "plan-state-current-label",
            "plan-state-current-head",
        ))
        .child(
            div()
                .border_t_1()
                .border_color(rgb(current_theme().surface))
                .child(plan_state(
                    &plan.predicted.head,
                    &plan.predicted.dirty,
                    Msg::InputStashAfter.t(),
                    "plan-state-after",
                    "plan-state-after-label",
                    "plan-state-after-head",
                )),
        )
        .when(cfg!(feature = "gui-e2e"), |panel| {
            panel.child(super::e2e::measure_inside("plan-state-comparison"))
        })
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

/// Plan-only heading: an inline operation icon, short localized title and up
/// to two mono target chips. Non-plan modals keep `render_modal_title_row`.
pub(crate) fn render_plan_heading(
    title: &'static str,
    chips: [Option<std::borrow::Cow<'_, str>>; 2],
    accent: PlanCardAccent,
    copy: Option<gpui::AnyElement>,
) -> gpui::Div {
    let (icon, color) = accent;
    let mut row = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .id("plan-heading-icon")
                .relative()
                .flex_shrink_0()
                .child(
                    modal_icon_element(icon)
                        .with_size(gpui_component::Size::Size(theme::scaled_px(18.)))
                        .text_color(rgb(color)),
                )
                .child(super::e2e::measure_inside("plan-heading-icon")),
        )
        .child(
            div()
                .id("plan-heading-title")
                .relative()
                .flex_shrink_0()
                .whitespace_nowrap()
                .text_lg()
                .text_color(rgb(current_theme().text_main))
                .child(title)
                .child(super::e2e::measure_inside("plan-heading-title")),
        );
    for (index, chip) in chips.into_iter().enumerate() {
        if let Some(chip) = chip {
            let id = match index {
                0 => "plan-heading-chip-0",
                _ => "plan-heading-chip-1",
            };
            let (bg, border, foreground) = theme::badge_style(current_theme().text_muted);
            row = row.child(
                div()
                    .id(id)
                    .relative()
                    .min_w(gpui::px(0.))
                    .max_w(theme::scaled_px(150.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_family(MONO_FONT)
                    .text_xs()
                    .px_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(gpui::rgba(border))
                    .bg(gpui::rgba(bg))
                    .text_color(rgb(foreground))
                    .child(SharedString::from(chip.into_owned()))
                    .child(super::e2e::measure_inside(id)),
            );
        }
    }
    row = row.child(div().flex_1());
    if let Some(copy) = copy {
        row = row.child(copy);
    }
    row
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
    let (heading_title, mut heading_chips) = plan_heading_text(&plan.title);
    if heading_chips.iter().all(Option::is_none) {
        heading_chips[0] = Some(std::borrow::Cow::Owned(plan.current.head.to_string()));
    }
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
    )
    .with_recovery(&plan_recovery_text(plan.recovery.as_ref()));
    let on_confirm = (!has_blockers).then(|| confirm_handler.clone());
    let card = apply_dialog(
        "plan-card",
        modal_card(MODAL_W_MD).id("plan-card"),
        spec,
        on_confirm,
        cancel_handler.clone(),
    )
    .child(render_plan_heading(
        heading_title,
        heading_chips,
        accent.clone().unwrap_or((
            ModalIcon::Named(IconName::Plus),
            current_theme().color_success,
        )),
        Some(
            modal_copy_button(
                "plan-card-copy",
                Msg::ModalCopyAll.t(),
                match &extra {
                    Some(extra) => format!(
                        "{}\n{}",
                        plan_clipboard_text(&plan, &plan.preview_commits, ShellKind::current()),
                        extra.clipboard
                    ),
                    None => plan_clipboard_text(&plan, &plan.preview_commits, ShellKind::current()),
                },
                cx,
            )
            .into_any_element(),
        ),
    ));

    // Fixed blocks stay flex_shrink_0-wrapped: only the panels give up height,
    // and without the guard flex would compress rows instead of scrolling
    // (same T027 bug class as the discard list).
    let mut body = modal_body().child(div().flex_shrink_0().child(render_current_predicted(&plan)));

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

    // Only structured commands supported by this shell are offered in the
    // compact disclosure. The full explanation remains in Copy all and AX.
    if super::modal_command::plan_ready(&plan) {
        if let Some(commands) = super::modal_renderers_plan::offered_recovery_commands(
            plan.recovery.as_ref(),
        ) {
            body = body.child(render_recovery_commands(
                commands,
                "plan-recovery",
                "plan-recovery-copy",
                "plan-recovery-body",
                overrides,
                cx,
            ));
        }
    }
    // Kagi executes the plan through its backend; the CLI spelling is
    // reference text, collapsed by default and separately copyable.
    if let Some(cmd) = super::modal_command::equivalent_command(&plan) {
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
