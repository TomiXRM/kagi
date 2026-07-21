//! Independently-rendered overlay entities split out of `render.rs`
//! (T-SPLIT-RENDER-001 / ADR-0116 Wave 3): the toast stack and operation-log
//! panel each render as their own GPUI entity so a push / row-expand only
//! re-renders that subtree, not all of `KagiApp` (ADR-0110 Phase 5). Behaviour
//! is unchanged — this is a pure physical move from `render.rs`.

#![allow(clippy::too_many_arguments)]

use super::render_helpers::*;
use super::*;
use crate::ui::modal_renderers::*;

// ──────────────────────────────────────────────────────────────
// Toast overlay (ADR-0110 Phase 5): the toast cards render as their own
// `Entity<ToastStack>` so a push/expire only re-renders this subtree, not
// the whole `KagiApp`. The busy snackbar stays on `KagiApp` (driven by
// `busy_op`); see `KagiApp::render_toasts`.

/// The big spinning sync icon shared by the busy snackbar and the
/// sync-flavoured no-op toasts (`ToastKind::Sync`), so every sync-icon
/// snackbar looks identical. `key` keeps each animation instance distinct.
pub(crate) fn big_sync_icon(accent: u32, key: impl Into<gpui::ElementId>) -> gpui::AnyElement {
    use gpui::AnimationExt as _;
    const SPIN_MS: u64 = 700;
    gpui::svg()
        .path("icons/refresh-cw.svg")
        // ~2× the header spinner (user request) so the snackbar reads
        // clearly as "working".
        .w(theme::scaled_px(32.0))
        .h(theme::scaled_px(32.0))
        .text_color(rgb(accent))
        .with_animation(
            key,
            gpui::Animation::new(Duration::from_millis(SPIN_MS)).repeat(),
            |svg, delta| {
                svg.with_transformation(gpui::Transformation::rotate(gpui::radians(
                    delta * std::f32::consts::TAU,
                )))
            },
        )
        .into_any_element()
}

impl gpui::Render for toast_stack::ToastStack {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut stack = div().flex().flex_col().gap_2();
        for toast in self.toasts() {
            let (accent, glyph) = match toast.kind {
                ToastKind::Info => (theme().color_branch, "\u{27f3}"), // ⟳
                ToastKind::Success => (theme().color_success, "\u{2713}"), // ✓
                ToastKind::Error => (theme().color_blocker, "\u{2715}"), // ✕
                ToastKind::Sync => (theme().color_branch, ""),
            };
            let id = toast.id;
            let is_sync = toast.kind == ToastKind::Sync;
            // Sync toasts reuse the busy snackbar's big spinning icon (user
            // request: "already up to date" must match an in-flight op); the
            // others keep the compact text glyph.
            let icon_el: gpui::AnyElement = if is_sync {
                big_sync_icon(accent, ("kagi-toast-sync", id))
            } else {
                div()
                    .text_color(rgb(accent))
                    .child(SharedString::from(glyph))
                    .into_any_element()
            };
            let leaving = toast.dismissing.is_some();
            let dismiss = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
                this.begin_exit(id, cx);
            });
            // Explicit width so the animated margin-left slides the whole card
            // horizontally (a stretched flex child wouldn't translate cleanly).
            let card = div()
                .w(theme::scaled_px(460.))
                .flex()
                .flex_row()
                .when(is_sync, |d| d.items_center().gap_3())
                .when(!is_sync, |d| d.items_start().gap_2())
                .px_4()
                .py_3()
                .rounded(theme::scaled_px(8.))
                .bg(rgb(theme().panel))
                .border_1()
                .border_color(rgb(accent))
                .text_base()
                .text_color(rgb(theme().text_main))
                .child(div().flex_shrink_0().child(icon_el))
                .child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .child(toast.message.clone()),
                )
                .child(
                    div()
                        .id(("toast-dismiss", id))
                        .flex_shrink_0()
                        .px_1()
                        .text_color(rgb(theme().text_muted))
                        .hover(|s| s.text_color(rgb(theme().text_main)))
                        .on_click(dismiss)
                        .child(SharedString::from("\u{00d7}")),
                );

            // Slide + fade: in from the left on appear, out to the left on
            // dismiss. Keyed by toast id so the animation plays once and holds.
            use gpui::AnimationExt as _;
            let animated = if leaving {
                card.with_animation(
                    ("kagi-toast-exit", id),
                    gpui::Animation::new(Duration::from_millis(TOAST_EXIT_MS))
                        .with_easing(gpui::quadratic),
                    |el, delta| el.ml(px(-TOAST_SLIDE_PX * delta)).opacity(1.0 - delta),
                )
                .into_any_element()
            } else {
                card.with_animation(
                    ("kagi-toast-enter", id),
                    gpui::Animation::new(Duration::from_millis(TOAST_ENTER_MS))
                        .with_easing(gpui::ease_out_quint()),
                    |el, delta| el.ml(px(-TOAST_SLIDE_PX * (1.0 - delta))).opacity(delta),
                )
                .into_any_element()
            };
            stack = stack.child(animated);
        }
        stack
    }
}

// ──────────────────────────────────────────────────────────────
// Operation Log overlay (ADR-0110 Phase 5 Step 5.1): the op-log renders as
// its own `Entity<OpLogPanel>` so a push / row-expand re-renders only this
// subtree. Embedded by `KagiApp::render_bottom_panel`.

impl gpui::Render for oplog_panel::OpLogPanel {
    /// Render the Operation Log tab body (T-BP-004).
    ///
    /// Uses `uniform_list` for virtual scroll.  Each row shows:
    ///   `HH:MM:SS  op  outcome-summary` (outcome coloured green/red/yellow).
    /// Clicking a row toggles single-row expansion (before/after + error/blockers).
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        // W12-GCADOPT (§2.10): Scrollbar overlay on the Operation Log list.
        let scrollbar_handle = scroll_handle.clone();

        let oplog_list = uniform_list(
            "oplog-list",
            entry_count,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                let entries: Vec<OpLogEntry> = this.entries().iter().cloned().collect();
                let expanded = this.expanded();
                range
                    .filter_map(|i| entries.get(i).cloned().map(|e| (i, e)))
                    .map(move |(i, entry)| {
                        let time_label = SharedString::from(format_hms(entry.timestamp));
                        let op_label = SharedString::from(entry.op.clone());

                        let (outcome_label, outcome_color) = match &entry.outcome {
                            OpOutcome::Success { after } => (
                                SharedString::from(format!("Success \u{2192} {}", after.head)),
                                theme().color_success,
                            ),
                            OpOutcome::Failed { error } => (
                                SharedString::from(format!("Failed: {}", error)),
                                theme().color_blocker,
                            ),
                            OpOutcome::Refused { blockers } => (
                                SharedString::from(format!(
                                    "Refused ({} blocker{})",
                                    blockers.len(),
                                    if blockers.len() == 1 { "" } else { "s" }
                                )),
                                theme().color_warning,
                            ),
                        };

                        let is_expanded = expanded == Some(i);

                        let row_click =
                            cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
                                this.toggle_expanded(i);
                                cx.notify();
                            });

                        let row_bg = if i % 2 == 0 {
                            theme().panel
                        } else {
                            theme().bg_base
                        };

                        // Summary row.
                        let mut row_div = div()
                            .id(("oplog-row", i))
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
                                    .h(theme::scaled_px(22.))
                                    .child(
                                        div()
                                            .w(theme::scaled_px(60.))
                                            .flex_shrink_0()
                                            .text_xs()
                                            .text_color(rgb(theme().text_muted))
                                            .child(time_label),
                                    )
                                    .child(
                                        div()
                                            .w(theme::scaled_px(100.))
                                            .flex_shrink_0()
                                            .ml(theme::scaled_px(6.))
                                            .text_xs()
                                            .text_color(rgb(theme().text_sub))
                                            .child(op_label),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .ml(theme::scaled_px(6.))
                                            .text_xs()
                                            .text_color(rgb(outcome_color))
                                            .truncate()
                                            .child(outcome_label),
                                    ),
                            );

                        // Expansion detail rows (before + outcome specifics).
                        if is_expanded {
                            let mut detail_lines: Vec<SharedString> = Vec::new();
                            detail_lines.push(SharedString::from(format!(
                                "  before:  {}",
                                entry.before.head
                            )));
                            detail_lines.push(SharedString::from(format!(
                                "  dirty:   {}",
                                entry.before.dirty
                            )));
                            match &entry.outcome {
                                OpOutcome::Success { after } => {
                                    detail_lines.push(SharedString::from(format!(
                                        "  after:   {}",
                                        after.head
                                    )));
                                    detail_lines.push(SharedString::from(format!(
                                        "  dirty:   {}",
                                        after.dirty
                                    )));
                                }
                                OpOutcome::Failed { error } => {
                                    detail_lines
                                        .push(SharedString::from(format!("  error:   {}", error)));
                                }
                                OpOutcome::Refused { blockers } => {
                                    for b in blockers {
                                        detail_lines
                                            .push(SharedString::from(format!("  blocker: {}", b)));
                                    }
                                }
                            }
                            let detail_div = div()
                                .flex()
                                .flex_col()
                                .w_full()
                                .px_3()
                                .py_1()
                                .bg(rgb(theme().selected))
                                .text_xs()
                                .text_color(rgb(theme().text_sub))
                                .children(detail_lines.into_iter().map(|line| div().child(line)));
                            row_div = row_div.child(detail_div);
                        }

                        row_div
                    })
                    .collect()
            }),
        )
        .track_scroll(&scroll_handle)
        .flex_1()
        .min_h(px(0.))
        .bg(rgb(theme().panel));

        with_vertical_scrollbar("oplog-list-scroll", &scrollbar_handle, oplog_list, true)
            .into_any_element()
    }
}

impl KagiApp {
    /// Modal / popover overlay layer (above the body, below the status bar).
    /// Extracted verbatim from `render` (T-SPLIT-RENDER-001 / ADR-0116 Wave 3)
    /// so the entry `render` reads as composition. The pre-cloned modal state is
    /// passed in (cloned at the same point in the frame as before), so the
    /// element tree / evaluation order is unchanged.
    pub(super) fn attach_modal_overlays(
        &self,
        el: gpui::Div,
        plan_modal: Option<CheckoutPlanModal>,
        pull_modal: Option<PullPlanModal>,
        undo_modal: Option<UndoPlanModal>,
        history_modal: Option<HistoryPlanModal>,
        conflict_continue_modal: Option<ConflictContinuePlanModal>,
        amend_modal: Option<AmendPlanModal>,
        pop_modal: Option<PopPlanModal>,
        stash_drop_modal: Option<StashDropModal>,
        push_modal: Option<PushPlanModal>,
        branch_plan_modal: Option<BranchPlanModal>,
        set_upstream_modal: Option<SetUpstreamModal>,
        rename_branch_modal: Option<RenameBranchModal>,
        merge_modal: Option<MergePlanModal>,
        tracking_checkout_modal: Option<TrackingCheckoutPlanModal>,
        switch_to_latest_modal: Option<SwitchToLatestPlanModal>,
        create_branch_modal: Option<CreateBranchModal>,
        create_tag_modal: Option<CreateTagModal>,
        create_worktree_modal: Option<CreateWorktreeModal>,
        unlock_worktree_modal: Option<UnlockWorktreeModal>,
        remote_browse_modal: Option<RemoteBrowseModal>,
        stash_push_modal: Option<StashPushModal>,
        stash_apply_modal: Option<StashApplyModal>,
        cherry_pick_modal: Option<CherryPickModal>,
        revert_modal: Option<RevertModal>,
        delete_branch_modal: Option<DeleteBranchModal>,
        delete_remote_branch_modal: Option<DeleteRemoteBranchModal>,
        branch_cleanup_modal: Option<BranchCleanupModal>,
        discard_modal: Option<DiscardModal>,
        editor_dirty_guard_modal: Option<EditorDirtyGuardModal>,
        editor_fs_prompt_modal: Option<EditorFsPromptModal>,
        editor_delete_confirm_modal: Option<EditorDeleteConfirmModal>,
        file_menu: Option<(usize, gpui::Point<gpui::Pixels>)>,
        modal_focus: Option<FocusHandle>,
        stash_push_focus: Option<FocusHandle>,
        commit_panel_open: bool,
        commit_panel: Option<Entity<commit_panel::CommitPanelView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        // ADR-0118: the plan modal now lives on the entity (`state.plan_modal`).
        let commit_plan_modal = commit_panel
            .as_ref()
            .and_then(|e| e.read(cx).state.plan_modal.clone());
        el.when_some(plan_modal, |el, modal| {
            el.child(render_plan_modal(modal, cx))
        })
        // ── Pull plan modal overlay (T-HT-003) ──────────
        .when_some(pull_modal, |el, modal| {
            el.child(render_pull_modal(modal, cx))
        })
        // ── Undo / Pop plan modal overlays ───────────────
        .when_some(undo_modal, |el, modal| {
            el.child(render_undo_modal(modal, cx))
        })
        // ── Operation Undo / Redo modal (T-UNDOREDO-001) ──
        .when_some(history_modal, |el, modal| {
            el.child(render_history_modal(modal, cx))
        })
        // ── Sequencer conflict-continue confirmation (ADR-0068) ──
        .when_some(conflict_continue_modal, |el, modal| {
            el.child(render_conflict_continue_modal(modal, cx))
        })
        .when_some(amend_modal, |el, modal| {
            el.child(render_amend_modal(modal, cx))
        })
        .when_some(pop_modal, |el, modal| el.child(render_pop_modal(modal, cx)))
        // ── Stash drop modal overlay (ADR-0087) ─────────
        .when_some(stash_drop_modal, |el, modal| {
            el.child(render_stash_drop_modal(modal, cx))
        })
        // ── Unlock-worktree confirmation ─────────────────
        .when_some(unlock_worktree_modal, |el, modal| {
            el.child(render_unlock_worktree_modal(modal, cx))
        })
        // ── Push plan modal overlay (T-HT-004) ──────────
        .when_some(push_modal, |el, modal| {
            el.child(render_push_modal(modal, cx))
        })
        .when_some(branch_plan_modal, |el, modal| {
            el.child(render_branch_plan_modal(modal, cx))
        })
        .when_some(set_upstream_modal, |el, modal| {
            el.child(render_set_upstream_modal(modal, cx))
        })
        .when_some(rename_branch_modal, |el, modal| {
            el.child(render_rename_branch_modal(modal, cx))
        })
        .when_some(merge_modal, |el, modal| {
            el.child(render_merge_modal(modal, cx))
        })
        .when_some(tracking_checkout_modal, |el, modal| {
            el.child(render_tracking_checkout_modal(modal, cx))
        })
        .when_some(switch_to_latest_modal, |el, modal| {
            el.child(render_switch_to_latest_modal(modal, cx))
        })
        // ── Create-branch modal overlay (above everything) ──
        .when_some(create_branch_modal, |el, modal| {
            el.child(render_create_branch_modal(modal, modal_focus.clone(), cx))
        })
        // ── Create-tag modal overlay ─────────────────────
        .when_some(create_tag_modal, |el, modal| {
            el.child(render_create_tag_modal(modal, modal_focus.clone(), cx))
        })
        // ── Create-worktree modal overlay ───────────────
        .when_some(create_worktree_modal, |el, modal| {
            el.child(render_create_worktree_modal(modal, modal_focus.clone(), cx))
        })
        // ── Remote SSH browse modal overlay (ADR-0089) ───
        .when_some(remote_browse_modal, |el, modal| {
            el.child(render_remote_browse_modal(modal, modal_focus.clone(), cx))
        })
        // ── Stash push modal overlay ─────────────────────
        .when_some(stash_push_modal, |el, modal| {
            el.child(render_stash_push_modal(modal, stash_push_focus, cx))
        })
        // ── Stash apply modal overlay ────────────────────
        .when_some(stash_apply_modal, |el, modal| {
            el.child(render_stash_apply_modal(modal, cx))
        })
        // ── Cherry-pick modal overlay (T016) ────────────
        .when_some(cherry_pick_modal, |el, modal| {
            el.child(render_cherry_pick_modal(modal, cx))
        })
        // ── Revert modal overlay (T-CM-034) ──────────────
        .when_some(revert_modal, |el, modal| {
            el.child(render_revert_modal(modal, cx))
        })
        // ── Delete-branch modal overlay (W2-DELETE) ──────
        .when_some(delete_branch_modal, |el, modal| {
            el.child(render_delete_branch_modal(modal, cx))
        })
        .when_some(delete_remote_branch_modal, |el, modal| {
            el.child(render_delete_remote_branch_modal(modal, cx))
        })
        // ── Branch-cleanup modal overlay (ADR-0128) ──────
        .when_some(branch_cleanup_modal, |el, modal| {
            el.child(render_branch_cleanup_modal(modal, cx))
        })
        // ── Discard danger modal overlay (W17-DISCARD) ───
        .when_some(discard_modal, |el, modal| {
            el.child(render_discard_modal(modal, cx))
        })
        // ── Editor Workspace unsaved-changes modal (T-WS-EDITOR-002) ──
        .when_some(editor_dirty_guard_modal, |el, modal| {
            el.child(render_editor_dirty_guard_modal(modal, cx))
        })
        // ── Editor Workspace tree fs-prompt (Rename/New File/New Folder) ──
        .when_some(editor_fs_prompt_modal, |el, modal| {
            el.child(render_editor_fs_prompt_modal(
                modal,
                modal_focus.clone(),
                cx,
            ))
        })
        // ── Editor Workspace tree Delete (Trash) confirm ─────────
        .when_some(editor_delete_confirm_modal, |el, modal| {
            el.child(render_editor_delete_confirm_modal(modal, cx))
        })
        // ── Unstaged file context menu (right-click → Discard) ──
        .when_some(file_menu, |el, (fi, pos)| {
            el.child(render_file_menu_overlay(fi, pos, cx))
        })
        // ── Commit plan modal overlay (T025) ─────────────
        .when(commit_panel_open && commit_plan_modal.is_some(), |el| {
            if let Some(plan_modal) = commit_plan_modal.clone() {
                el.child(render_commit_plan_modal(plan_modal, cx))
            } else {
                el
            }
        })
        // ── Smart Commit modal overlay (T-COMMIT-016) ────
        .when_some(self.smart_commit.modal.clone(), |el, modal| {
            el.child(render_smart_commit_modal(modal, cx))
        })
        // ── Auto-update modal overlay (ADR-0082) ──────────
        .when_some(
            if self.update_modal_open {
                self.update_available.as_ref().map(|(p, _)| {
                    (
                        p.clone(),
                        self.update_installing,
                        self.update_status.clone(),
                    )
                })
            } else {
                None
            },
            |el, (plan, installing, status)| {
                el.child(render_update_modal(plan, installing, status, window, cx))
            },
        )
    }
}
