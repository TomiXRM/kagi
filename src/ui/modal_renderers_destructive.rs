//! History-rewriting / destructive modal renderers split out of
//! `modal_renderers.rs` (T-SPLIT-MODALS-001 / ADR-0116 Wave 3): amend (two-stage
//! rewrite-history confirm) and discard (two-stage permanent-discard confirm).
//! These build bespoke cards rather than delegating to `render_plan_modal_card`.
//! Pure physical move — behaviour unchanged.

#![allow(clippy::too_many_arguments)]

use super::button_style::KagiButton;
use super::i18n::Msg;
use super::modal_renderers::{
    modal_overlay, render_current_predicted, render_modal_title_row, render_recovery_box, ModalIcon,
};
use super::modal_shell::{
    modal_body, modal_card, modal_change_summary, modal_chip, modal_file_row, modal_list_max_h,
    modal_list_panel, modal_prose_box, modal_section, modal_section_chipped, section_open,
    MODAL_W_MD,
};
use super::modals::*;
use super::theme::theme as current_theme;
use super::KagiApp;
use gpui::{div, prelude::*, rgb, Context, KeyDownEvent, SharedString};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::Sizable as _;
use kagi_ui_core::i18n::{plan_note_text, plan_recovery_text, plan_title_text};

/// Every fully-bespoke destructive modal in this file badges itself with
/// `trash-2` (no upstream `IconName` variant — raw asset path, same as the
/// toolbar's Editor/Graph glyphs) in `color_blocker`, matching every other
/// destructive plan-confirmation modal (user request 2026-07-23).
const DESTRUCTIVE_ICON: ModalIcon = ModalIcon::Path("icons/trash-2.svg");

/// #454 section id. Only *supporting* detail may hide behind disclosure: the
/// list of files an operation acts on stays visible (see `render_amend_modal`).
const SECTION_SKIPPED: &str = "discard-skipped";
/// #454: section ids for the panels a card can collapse. Warnings and recovery
/// default to **open** on destructive cards (safety), but the user may fold
/// them — `modal_section_overrides` records only the flip.
const SECTION_AMEND_WARNINGS: &str = "amend-warnings";
const SECTION_AMEND_RECOVERY: &str = "amend-recovery";
const SECTION_DISCARD_WARNINGS: &str = "discard-warnings";
const SECTION_DISCARD_RECOVERY: &str = "discard-recovery";

/// Amend confirmation overlay (T-COMMIT-011, ADR-0040 / 0023).
///
/// History-rewriting → **two-stage confirm**.  The first Confirm click arms the
/// action (`confirm_armed` flips to true); the button then turns into an
/// explicit, red final-confirm that lists what is lost (the old SHA).  No typed
/// confirmation is required (ADR-0023).
pub(crate) fn render_amend_modal(
    modal: AmendPlanModal,
    // #454: user's section open/closed overrides (`KagiApp` owns them; the
    // renderer owns the defaults). The folded-file list is NOT collapsible;
    // the warnings and recovery panels are.
    overrides: &std::collections::HashSet<&'static str>,
    // #454: scroll handle for the folded-file `uniform_list` (owned by
    // `KagiApp` so the position survives re-renders while the modal is open).
    list_scroll: gpui::UniformListScrollHandle,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let armed = modal.confirm_armed;
    let has_blockers = !modal.plan.blockers.is_empty();
    let plan = modal.plan.clone();
    let error = modal.error.clone();

    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_amend_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        // First click arms; second click executes (handled in start_amend).
        this.start_amend(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });

    // #454: fixed title + non-scrolling body + fixed button row. The buttons
    // must never scroll out of view on a destructive confirm, and the only
    // scroll region is the file list itself.
    //
    // The header carries the stakes as a chip (mock: `Cannot be undone` next to
    // the title) — driven by `plan.destructive`, not by the renderer guessing.
    let mut title_row = div()
        .flex_shrink_0()
        .flex()
        .flex_row()
        // items_start, not items_center: a wrapped title would otherwise push
        // the chip to its vertical middle, where it reads as floating.
        .items_start()
        .gap_2()
        // flex_1 + min_w(0): without it a long title claims the whole row and
        // squeezes the chip to zero width (observed on the amend card, whose
        // title carries the commit subject).
        .child(
            div()
                .flex_1()
                .min_w(gpui::px(0.))
                .child(render_modal_title_row(
                    SharedString::from(plan_title_text(&plan.title)),
                    Some((DESTRUCTIVE_ICON, current_theme().color_blocker)),
                )),
        );
    if plan.destructive {
        title_row = title_row.child(modal_chip(
            Msg::ModalCannotUndo.t(),
            current_theme().color_blocker,
        ));
    }
    let card = modal_card(MODAL_W_MD).child(title_row);
    let mut body = modal_body().child(render_current_predicted(
        &plan,
        Some((DESTRUCTIVE_ICON, current_theme().color_blocker)),
    ));

    // #454 Phase 2: the staged files this amend folds in were cut to the first
    // 10 rows with no "+N more" and no scroll — with 487 staged files the other
    // 477 were unreachable. Now every row is reachable through a `uniform_list`.
    //
    // SAFETY: this list is what the operation *acts on*, so it is NOT
    // collapsible. Disclosure (`modal_section`) is only for supporting detail —
    // a destructive confirm must never be able to hide its own targets, and a
    // sticky user override must not carry that state into the next confirm.
    if !plan.preview_files.is_empty() {
        let total = plan.preview_files.len();
        let files = plan.preview_files.clone();
        // Height follows the content up to a window-relative ceiling: a 3-file
        // amend gets a 3-row box, a 172-file amend fills 40% of the window
        // instead of the 9 rows a fixed 160px box showed (measured on screen,
        // #454). Past the ceiling the `uniform_list` scrolls.
        let list_h = modal_list_max_h(total);
        let list = super::render_helpers::with_vertical_scrollbar(
            "amend-files-scroll",
            &list_scroll,
            gpui::uniform_list(
                "amend-files-list",
                total,
                move |range: std::ops::Range<usize>, _window, _cx| {
                    range
                        .filter_map(|i| {
                            files
                                .get(i)
                                .map(|f| modal_file_row(f.path.display().to_string(), &f.change))
                        })
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&list_scroll)
            .h(list_h),
            true,
        );
        // The target list is a section panel like every other block in the
        // card (mock: `対象ファイル` + count chip), but NOT collapsible — see
        // the SAFETY note above. `section_open(.., true)`-style disclosure is
        // deliberately not wired here.
        body = body.child(modal_list_panel(
            Msg::AmendFoldedFiles.t(),
            total,
            // Per-kind tally above the list (#454 Phase 2 item 6).
            div()
                .min_h(gpui::px(0.))
                .flex()
                .flex_col()
                .gap_2()
                .children(modal_change_summary(&plan.preview_files))
                .child(list)
                .into_any_element(),
        ));
    }

    // Warnings get the mock's section treatment (title + count chip) but stay
    // **open**: in a destructive card they explain what will NOT be touched,
    // which is safety-relevant (issue #454 advisory). The mock collapses them;
    // hiding "these files will not be discarded" behind a caret on a confirm
    // dialog is the one place that trade goes the other way.
    if !plan.warnings.is_empty() {
        let mut warn_col = div().flex().flex_col().gap_1();
        for w in &plan.warnings {
            warn_col = warn_col.child(
                div()
                    .flex_shrink_0()
                    .text_sm()
                    .text_color(rgb(current_theme().color_warning))
                    .child(SharedString::from(format!(
                        "\u{26a0} {}",
                        plan_note_text(w)
                    ))),
            );
        }
        let open = section_open(overrides, SECTION_AMEND_WARNINGS, true);
        body = body.child(modal_section(
            SECTION_AMEND_WARNINGS,
            Msg::ModalWarningsSection.t(),
            plan.warnings.len(),
            open,
            open.then(|| warn_col.into_any_element()),
            cx,
        ));
    }

    // Blockers.
    if has_blockers {
        let mut block_col = div().flex().flex_col().gap_1();
        for b in &plan.blockers {
            block_col = block_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(current_theme().color_blocker))
                    .overflow_hidden()
                    .child(SharedString::from(format!(
                        "\u{2717} {}",
                        plan_note_text(b)
                    ))),
            );
        }
        body = body.child(block_col);
    }

    // Recovery: the mock's `復元方法` panel with an `oplog` chip.
    let recovery_text = plan_recovery_text(plan.recovery.as_ref());
    if !recovery_text.is_empty() {
        let open = section_open(overrides, SECTION_AMEND_RECOVERY, true);
        body = body.child(modal_section_chipped(
            SECTION_AMEND_RECOVERY,
            Msg::ModalRecoverySection.t(),
            Some(SharedString::from(Msg::ModalRecoveryChip.t())),
            open,
            // Built only while open, so a folded section costs nothing.
            open.then(|| {
                modal_prose_box(
                    "modal-recovery-scroll",
                    render_recovery_box(&recovery_text, current_theme().color_blocker),
                )
                .into_any_element()
            }),
            cx,
        ));
    }

    // When armed: explicit "what is lost" second-stage notice (ADR-0023).
    if armed && !has_blockers {
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div().text_sm().text_color(rgb(current_theme().color_blocker))
                        .child(SharedString::from("\u{26a0} This rewrites history. Click \u{201c}Rewrite history\u{201d} to confirm.")),
                )
                .child(
                    div().text_xs().text_color(rgb(current_theme().text_sub)).overflow_hidden()
                        .child(SharedString::from(
                            "The current commit's SHA will be replaced. The old commit becomes unreachable from the branch (recoverable via git reflog / reset --hard <old>).",
                        )),
                ),
        );
    }

    // Error.
    if let Some(err) = &error {
        body = body.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    // Buttons.
    let mut button_row = div().flex().flex_row().gap_2().justify_end().child(
        Button::new("amend-cancel")
            .label(Msg::PlanCancel.t())
            .ghost()
            .small()
            .on_click(cancel_handler),
    );

    if !has_blockers {
        // Stage 1 label = "Amend\u{2026}", stage 2 (armed) = red "Rewrite history".
        let label = if armed {
            "Rewrite history"
        } else {
            "Amend\u{2026}"
        };
        let confirm = if armed {
            KagiButton::accent("amend-confirm", label, current_theme().color_blocker, cx)
        } else {
            Button::new("amend-confirm").label(label).primary()
        };
        button_row = button_row.child(confirm.small().on_click(confirm_handler));
    }

    let card = card.child(body).child(button_row);

    // ── Full-screen overlay wrapper (shared chrome, T-SPLIT-HELPERS-001) ──
    modal_overlay(card).into_any_element()
}

/// Discard confirmation overlay (W17-DISCARD, ADR-0046).
///
/// Danger (red) card: target file list (scrollable), any skipped
/// untracked/conflicted files, recovery note, Cancel + red Discard.
/// ESC cancels. Both the backdrop AND the card call `.occlude()` to defeat the
/// known click-through bug. The Discard button is hidden when there are blockers
/// or zero targets.
pub(crate) fn render_discard_modal(
    modal: DiscardModal,
    // #454: user's section open/closed overrides (`KagiApp` owns them; the
    // renderer owns the defaults). Only the skipped list is collapsible here.
    overrides: &std::collections::HashSet<&'static str>,
    // #454: scroll handle for the target-file `uniform_list` ("Discard all" is
    // the biggest list in the app, so it is virtualized like amend's).
    list_scroll: gpui::UniformListScrollHandle,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let plan = modal.plan.clone();
    let has_blockers = !plan.blockers.is_empty();
    let target_count = modal.paths.len();
    let can_discard = !has_blockers && target_count > 0;
    // Two-stage confirm (T-REARCH-014): first click arms, second executes.
    let armed = modal.confirm_armed;

    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_discard_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.start_discard(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh, cx);
        }
        cx.notify();
    });
    let esc_cancel = cx.listener(|this, e: &KeyDownEvent, window, cx| {
        if e.keystroke.key == "escape" {
            this.cancel_discard_modal();
            if let Some(fh) = this.root_focus.clone() {
                window.focus(&fh, cx);
            }
            cx.stop_propagation();
            cx.notify();
        }
    });

    let title = if modal.is_all {
        format!("Discard all changes ({})", target_count)
    } else {
        plan_title_text(&plan.title)
    };

    // ── Target file list (virtualized + scrollable) ─────────
    // #454: "Discard all changes (487)" is the biggest list in the app, so this
    // is a `uniform_list` — a plain div loop rebuilt every row every frame.
    // Height follows the row count up to the shared window-relative ceiling,
    // so a 2-file discard is a 2-row box and a 200-file discard fills 40% of
    // the window and scrolls.
    // Paths are NOT pre-truncated: cutting the head keeps the least
    // identifying part, and `overflow_hidden` already clips what does not fit.
    let target_count_rows = modal.paths.len();
    let target_h = modal_list_max_h(target_count_rows);
    let target_paths = modal.paths.clone();
    // Change kinds come from the plan (`plan_discard` fills `preview_files`
    // from the working-tree status), matched by path so the badge cannot drift
    // if the two vectors are ordered differently.
    let target_kinds: std::collections::HashMap<String, kagi_domain::status::ChangeKind> = plan
        .preview_files
        .iter()
        .map(|f| (f.path.display().to_string(), f.change.clone()))
        .collect();
    let file_list = super::render_helpers::with_vertical_scrollbar(
        "discard-file-scroll",
        &list_scroll,
        gpui::uniform_list(
            "discard-file-list",
            target_count_rows,
            move |range: std::ops::Range<usize>, _window, _cx| {
                range
                    .filter_map(|i| {
                        target_paths.get(i).map(|p| {
                            let change = target_kinds
                                .get(p)
                                .cloned()
                                .unwrap_or(kagi_domain::status::ChangeKind::Modified);
                            modal_file_row(p.clone(), &change)
                        })
                    })
                    .collect::<Vec<_>>()
            },
        )
        .track_scroll(&list_scroll)
        .h(target_h),
        true,
    );

    // ── Card ─────────────────────────────────────────────────
    // Icon badge (trash-2 / color_blocker) now carries the danger signal that
    // the full-card red border used to — matches every other destructive
    // plan-confirmation modal (user request 2026-07-23), one less box.
    //
    // #454: fixed title + non-scrolling body + fixed button row
    // (`modal_card`), so a long skipped/blocker list can no longer push
    // Cancel/Discard out of view. The target-file list is the scroll region
    // and is NOT collapsible — a destructive confirm always shows what it
    // acts on.
    let mut title_row = div()
        .flex_shrink_0()
        .flex()
        .flex_row()
        // items_start, not items_center: a wrapped title would otherwise push
        // the chip to its vertical middle, where it reads as floating.
        .items_start()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w(gpui::px(0.))
                .child(render_modal_title_row(
                    SharedString::from(title),
                    Some((DESTRUCTIVE_ICON, current_theme().color_blocker)),
                )),
        );
    if plan.destructive {
        title_row = title_row.child(modal_chip(
            Msg::ModalCannotUndo.t(),
            current_theme().color_blocker,
        ));
    }
    let card = modal_card(MODAL_W_MD).child(title_row);
    // Target files: a section panel with a count chip (mock `対象ファイル`),
    // never collapsible — a destructive confirm always shows what it acts on.
    // No targets (a blocked "Discard all" with nothing unstaged) renders no
    // panel: an empty box with a `0` chip is chrome for nothing, and the
    // blocker below already says why.
    let mut body = modal_body();
    if target_count > 0 {
        body = body.child(modal_list_panel(
            Msg::ModalTargetFiles.t(),
            target_count,
            // Per-kind tally above the list (#454 Phase 2 item 6): `M 115  D 5`
            // says what kind of change is at stake without scrolling.
            div()
                .min_h(gpui::px(0.))
                .flex()
                .flex_col()
                .gap_2()
                .children(modal_change_summary(&plan.preview_files))
                .child(file_list)
                .into_any_element(),
        ));
    }

    // ── Skipped (untracked / conflicted) ────────────────────
    // #454: was `take(20)` — the rest of the skipped paths were unreachable.
    // These are the files discard does NOT touch, i.e. supporting detail, so
    // this is the one part of the card behind disclosure. The count stays in
    // the header even while collapsed, so "how many were skipped" is never
    // hidden — only the paths are, and the section lists all of them.
    if !modal.skipped.is_empty() {
        let open = section_open(overrides, SECTION_SKIPPED, false);
        let section_body = open.then(|| {
            // Cap the opened section too: the body does not scroll, so without
            // its own box hundreds of skipped paths would push the blockers
            // and the recovery note out of the card — the two things a user
            // needs next to the Discard button. Same shape as the target list.
            let mut skip_col = div()
                .id("discard-skipped-list")
                .flex()
                .flex_col()
                .gap_px()
                .min_h(gpui::px(0.))
                .max_h(modal_list_max_h(modal.skipped.len()))
                .overflow_y_scroll();
            // No `chars().take(80)`: cutting the head keeps the least
            // identifying part of a deep path, and `overflow_hidden` below
            // already clips whatever does not fit the card (#454 review).
            for p in &modal.skipped {
                skip_col = skip_col.child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(rgb(current_theme().text_muted))
                        .overflow_hidden()
                        .child(SharedString::from(format!(
                            "\u{2014} {} (untracked/conflicted)",
                            p
                        ))),
                );
            }
            skip_col.into_any_element()
        });
        body = body.child(modal_section(
            SECTION_SKIPPED,
            Msg::DiscardSkippedSection.t(),
            modal.skipped.len(),
            open,
            section_body,
            cx,
        ));
    }

    // ── Warnings / Blockers ─────────────────────────────────
    // Warnings become the mock's titled panel with a count chip, but stay
    // **open**: here they say which files will NOT be discarded, and hiding
    // that behind a caret on a destructive confirm is the wrong trade.
    if !plan.warnings.is_empty() {
        let mut warn_col = div().flex().flex_col().gap_px();
        for w in &plan.warnings {
            warn_col = warn_col.child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(rgb(current_theme().color_warning))
                    .overflow_hidden()
                    .child(SharedString::from(format!(
                        "\u{26a0} {}",
                        plan_note_text(w)
                    ))),
            );
        }
        let open = section_open(overrides, SECTION_DISCARD_WARNINGS, true);
        body = body.child(modal_section(
            SECTION_DISCARD_WARNINGS,
            Msg::ModalWarningsSection.t(),
            plan.warnings.len(),
            open,
            open.then(|| warn_col.into_any_element()),
            cx,
        ));
    }
    if has_blockers {
        let mut block_col = div().flex().flex_col().gap_px();
        for b in &plan.blockers {
            block_col = block_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(current_theme().color_blocker))
                    .overflow_hidden()
                    .child(SharedString::from(format!(
                        "\u{2717} {}",
                        plan_note_text(b)
                    ))),
            );
        }
        body = body.child(block_col);
    }

    // ── Recovery note ───────────────────────────────────────
    // Mock's `復元方法` panel: titled, `oplog` chip, open by default — the
    // backup blob reference is the reason discard is allowed to exist.
    let recovery_text = plan_recovery_text(plan.recovery.as_ref());
    if !recovery_text.is_empty() {
        let open = section_open(overrides, SECTION_DISCARD_RECOVERY, true);
        body = body.child(modal_section_chipped(
            SECTION_DISCARD_RECOVERY,
            Msg::ModalRecoverySection.t(),
            Some(SharedString::from(Msg::ModalRecoveryChip.t())),
            open,
            // Built only while open, so a folded section costs nothing.
            open.then(|| {
                modal_prose_box(
                    "modal-recovery-scroll",
                    render_recovery_box(&recovery_text, current_theme().color_blocker),
                )
                .into_any_element()
            }),
            cx,
        ));
    }

    // ── Error (preflight / execute failure) ─────────────────
    if let Some(err) = &modal.error {
        body = body.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    // ── Two-stage "what is lost" warning (armed second stage) ──
    // Mirrors amend's armed notice (ADR-0023). Only shown after the first
    // click armed the action, so the user sees an explicit final warning.
    if armed && can_discard {
        body = body.child(
            div()
                .text_sm()
                .text_color(rgb(current_theme().color_blocker))
                .child(SharedString::from(
                    "\u{26a0} Working-tree changes will be lost. Click \u{201c}Permanently discard\u{201d} to confirm.",
                )),
        );
    }

    // ── Buttons ─────────────────────────────────────────────
    let mut button_row = div().flex().flex_row().gap_2().justify_end().child(
        Button::new("discard-cancel")
            .label(Msg::PlanCancel.t())
            .ghost()
            .small()
            .on_click(cancel_handler),
    );
    if can_discard {
        // Two-stage confirm (T-REARCH-014): first click arms the red Discard
        // button (label becomes the explicit "Permanently discard N files");
        // the second click executes. Mirrors amend's confirm_armed pattern.
        // Both stages stay red — discard is always a destructive op.
        let label = if armed {
            format!("Permanently discard {} file(s)", target_count)
        } else {
            format!("Discard {} file(s)", target_count)
        };
        button_row = button_row.child(
            KagiButton::accent("discard-confirm", label, current_theme().color_blocker, cx)
                .small()
                .on_click(confirm_handler),
        );
    }
    let card = card.child(body).child(button_row);

    // ── Full-screen overlay (shared chrome, T-SPLIT-HELPERS-001) ──
    // ESC cancels via the root key handler; the card itself also occludes
    // (ADR-0046 / W17), else clicks fall through to the UI beneath. Chaining
    // `.on_key_down` onto the shared overlay is DOM-equivalent — event handlers
    // are stored independently of the child element list.
    modal_overlay(card.occlude())
        .on_key_down(esc_cancel)
        .into_any_element()
}
