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

use super::theme::{self, theme as current_theme};
use super::KagiApp;
use gpui::{div, prelude::*, rgb, Context, Entity, SharedString, Window};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::{Icon, IconName, Sizable as _};
use kagi_git::{BranchRenameValidation, CommitId, OperationPlan};
use kagi_ui_core::i18n::{plan_note_text, plan_recovery_text, plan_title_text};

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

pub(crate) fn render_input_plan_modal(
    title: String,
    label: &'static str,
    input_state: Option<Entity<InputState>>,
    plan: Option<std::sync::Arc<OperationPlan>>,
    validation: Option<BranchRenameValidation>,
    error: Option<SharedString>,
    confirm_label: &'static str,
    accent: Option<PlanCardAccent>,
    cancel_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    confirm_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::AnyElement {
    let has_blockers = plan
        .as_ref()
        .map(|p| !p.blockers.is_empty())
        .unwrap_or(true);
    let mut card = div()
        .w(theme::scaled_px(480.))
        .bg(rgb(current_theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(render_modal_title_row(
            SharedString::from(title),
            accent.clone(),
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().text_label))
                        .child(SharedString::from(label)),
                )
                .children(input_state.as_ref().map(|st| Input::new(st).small())),
        );

    if let Some(BranchRenameValidation::Invalid(reason)) = validation {
        // W29-I18N-WAVE2: localize the keyed branch-name reason.
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(SharedString::from(crate::ui::i18n::branch_name_error(
                    &reason,
                ))),
        );
    }

    if let Some(plan) = plan {
        card = card.child(render_current_predicted(&plan, accent.clone()));

        if !plan.warnings.is_empty() {
            let mut warn_col = div().flex().flex_col().gap_1();
            for warning in &plan.warnings {
                warn_col = warn_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_warning))
                        .overflow_hidden()
                        .child(SharedString::from(format!(
                            "\u{26a0} {}",
                            plan_note_text(warning)
                        ))),
                );
            }
            card = card.child(warn_col);
        }
        if !plan.blockers.is_empty() {
            let mut block_col = div().flex().flex_col().gap_1();
            for blocker in &plan.blockers {
                block_col = block_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(current_theme().color_blocker))
                        .overflow_hidden()
                        .child(SharedString::from(format!(
                            "\u{2717} {}",
                            plan_note_text(blocker)
                        ))),
                );
            }
            card = card.child(block_col);
        }
    }

    if let Some(err) = error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err),
        );
    }

    let mut buttons = div().flex().flex_row().gap_2().justify_end().child(
        Button::new("branch-input-cancel")
            .label("Cancel")
            .ghost()
            .small()
            .on_click(cancel_handler),
    );
    if !has_blockers {
        buttons = buttons.child(
            Button::new("branch-input-confirm")
                .label(SharedString::from(confirm_label))
                .primary()
                .small()
                .on_click(confirm_handler),
        );
    }
    card = card.child(buttons);

    modal_overlay(card).into_any_element()
}

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

/// Shared plan-confirmation card: title / current→predicted / warnings /
/// blockers / recovery / error / Cancel + confirm buttons.  The confirm
/// button is hidden whenever the plan has blockers.
///
/// ── Richer-card helpers (accent-gated; Pull/Push/Create-Branch today) ────
///
/// One current/predicted comparison block. Plain (`accent: None`) matches
/// every modal's original two-line layout exactly. The icon-badge accent
/// path wraps both states in a tinted card with a centred accent-coloured
/// arrow between them instead of a "→ Predicted" text label. Also used by
/// the bespoke create-branch card (user request 2026-07-23: match Pull's
/// state-transition display instead of a separate one-off layout).
pub(crate) fn render_current_predicted(
    plan: &OperationPlan,
    accent: Option<PlanCardAccent>,
) -> gpui::AnyElement {
    let state_line = |head: &str, dirty: &str| {
        div()
            .flex()
            .flex_row()
            .gap_2()
            .text_sm()
            .child(
                div()
                    .text_color(rgb(current_theme().text_main))
                    .child(SharedString::from(head.to_string())),
            )
            .child(
                div()
                    .text_color(rgb(current_theme().text_sub))
                    .child(SharedString::from(format!("[{}]", dirty))),
            )
    };
    match accent {
        None => div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(current_theme().text_label))
                    .child(SharedString::from("Current")),
            )
            .child(state_line(&plan.current.head, &plan.current.dirty))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(current_theme().text_label))
                    .child(SharedString::from("\u{2192} Predicted")),
            )
            .child(state_line(&plan.predicted.head, &plan.predicted.dirty))
            .into_any_element(),
        Some((_, color)) => div()
            .rounded_md()
            .bg(rgb(current_theme().surface))
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(current_theme().text_label))
                    .child(SharedString::from("CURRENT")),
            )
            .child(state_line(&plan.current.head, &plan.current.dirty))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .py(theme::scaled_px(1.))
                    .text_color(rgb(color))
                    .child(SharedString::from("\u{2193}")),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(current_theme().text_label))
                    .child(SharedString::from("PREDICTED")),
            )
            .child(state_line(&plan.predicted.head, &plan.predicted.dirty))
            .into_any_element(),
    }
}

/// One warning/blocker line. `chip: false` (every modal except Pull/Push)
/// collapses to the original single-line "glyph␠text" row, unchanged.
/// `chip: true` wraps the same glyph + text in a tinted, bordered row (same
/// `theme::badge_style` recipe as the ref-badge chips in the commit graph).
fn render_note_row(glyph: &'static str, color: u32, text: &str, chip: bool) -> gpui::AnyElement {
    if !chip {
        return div()
            .text_sm()
            .text_color(rgb(color))
            .overflow_hidden()
            .child(SharedString::from(format!("{} {}", glyph, text)))
            .into_any_element();
    }
    let (bg, border, _) = theme::badge_style(color);
    div()
        .flex()
        .flex_row()
        .items_start()
        .gap_2()
        .rounded_md()
        .bg(gpui::rgba(bg))
        .border_1()
        .border_color(gpui::rgba(border))
        .px_2()
        .py(theme::scaled_px(4.))
        .child(
            div()
                .flex_shrink_0()
                .text_color(rgb(color))
                .child(SharedString::from(glyph)),
        )
        .child(
            div()
                .flex_1()
                .min_w(gpui::px(0.))
                .text_sm()
                .text_color(rgb(current_theme().text_main))
                .overflow_hidden()
                .child(SharedString::from(text.to_string())),
        )
        .into_any_element()
}

/// One "commits to push" preview row (T-HT-004). Plain (`accent: None`):
/// unchanged single truncated line. Chip style: the sha gets its own small
/// badge, the summary follows in regular text — same visual family as
/// [`render_note_row`]'s chips.
fn render_commit_row(line: &str, accent: Option<PlanCardAccent>) -> gpui::AnyElement {
    let Some((_, color)) = accent else {
        return div()
            .text_xs()
            .text_color(rgb(current_theme().text_sub))
            .overflow_hidden()
            .child(SharedString::from(line.to_string()))
            .into_any_element();
    };
    // Producer format is "<8-char-sha>  <summary>" (ops/push.rs). Fall back to
    // the whole line as the "sha" slot if that shape ever changes — this is
    // purely cosmetic splitting, never a behavioural decision (ADR-0129).
    let (sha, summary) = match line.split_once("  ") {
        Some((s, rest)) if s.len() == 8 && s.bytes().all(|c| c.is_ascii_hexdigit()) => {
            (s, rest.trim_start())
        }
        _ => (line, ""),
    };
    let (bg, border, _) = theme::badge_style(color);
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_shrink_0()
                .px_1()
                .rounded_sm()
                .bg(gpui::rgba(bg))
                .border_1()
                .border_color(gpui::rgba(border))
                .text_color(rgb(color))
                .text_xs()
                .child(SharedString::from(sha.to_string())),
        )
        .child(
            div()
                .flex_1()
                .min_w(gpui::px(0.))
                .text_xs()
                .text_color(rgb(current_theme().text_sub))
                .overflow_hidden()
                .child(SharedString::from(summary.to_string())),
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
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(move |this, _e: &gpui::ClickEvent, window, cx| {
        cancel_action(this, cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(move |this, _e: &gpui::ClickEvent, window, cx| {
        confirm_action(this, cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    render_plan_modal_card_styled(
        plan,
        error,
        confirm_label,
        cancel_handler,
        confirm_handler,
        create_branch_target,
        accent,
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
                .child(
                    div()
                        .flex_shrink_0()
                        .w(theme::scaled_px(40.))
                        .h(theme::scaled_px(40.))
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
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .overflow_hidden()
                        .child(title),
                )
                .into_any_element()
        }
        None => div()
            .text_color(rgb(current_theme().text_main))
            .text_xl()
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
    cancel_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    confirm_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    create_branch_target: Option<CommitId>,
    accent: Option<PlanCardAccent>,
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

    // ── Build modal card ────────────────────────────────────
    let mut card = div()
        .w(theme::scaled_px(480.))
        .bg(rgb(current_theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(title_row)
        // ── Current → Predicted ───────────────────────────
        .child(render_current_predicted(&plan, accent.clone()));

    // ── Warnings ─────────────────────────────────────────
    if !plan.warnings.is_empty() {
        let mut warn_col = div().flex().flex_col().gap_1();
        for w in &plan.warnings {
            warn_col = warn_col.child(render_note_row(
                "\u{26a0}",
                current_theme().color_warning,
                &plan_note_text(w),
                accent.is_some(),
            ));
        }
        card = card.child(warn_col);
    }

    // ── Commits to push (T-HT-004) ────────────────────────
    // Shown only when preview_commits is non-empty (push plans).
    if !plan.preview_commits.is_empty() {
        let total = plan.preview_commits.len();
        let show_count = total.min(10);
        let label = format!("Commits to push ({})", total);
        let mut commit_col = div().flex().flex_col().gap_1().child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().text_label))
                .child(SharedString::from(label)),
        );
        for entry in plan.preview_commits.iter().take(show_count) {
            let line: String = entry.chars().take(72).collect();
            commit_col = commit_col.child(render_commit_row(&line, accent.clone()));
        }
        if total > 10 {
            commit_col = commit_col.child(
                div()
                    .text_xs()
                    .text_color(rgb(current_theme().text_muted))
                    .child(SharedString::from(format!(
                        "\u{2026} and {} more",
                        total - 10
                    ))),
            );
        }
        card = card.child(commit_col);
    }

    // ── Blockers ──────────────────────────────────────────
    if !plan.blockers.is_empty() {
        let mut block_col = div().flex().flex_col().gap_1();
        for b in &plan.blockers {
            block_col = block_col.child(render_note_row(
                "\u{2717}",
                current_theme().color_blocker,
                &plan_note_text(b),
                accent.is_some(),
            ));
        }
        card = card.child(block_col);
    }

    // ── Recovery ──────────────────────────────────────────
    let recovery_text = plan_recovery_text(plan.recovery.as_ref());
    if !recovery_text.is_empty() {
        card = card.child(match accent {
            Some((_, color)) => render_recovery_box(&recovery_text, color),
            None => div()
                .text_xs()
                .text_color(rgb(current_theme().text_muted))
                .overflow_hidden()
                .child(SharedString::from(recovery_text))
                .into_any_element(),
        });
    }

    // ── Error message (preflight / execute failure) ───────
    if let Some(err) = &error {
        card = card.child(
            div()
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
        // Cancel button (always present — safe default)
        .child(
            Button::new("plan-cancel")
                .label("Cancel")
                .ghost()
                .small()
                .on_click(cancel_handler),
        );

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
        button_row = button_row.child(
            Button::new("plan-confirm")
                .label(confirm_label)
                .primary()
                .small()
                .on_click(confirm_handler),
        );
    }

    card = card.child(button_row);

    // ── Full-screen overlay wrapper (shared chrome, T-SPLIT-HELPERS-001) ──
    modal_overlay(card)
}

// ──────────────────────────────────────────────────────────────
// ActiveModal — single active-modal enum (ADR-0076 / issue #13 P7)
// ──────────────────────────────────────────────────────────────

// ActiveModal moved back to modals.rs (state, not rendering).
