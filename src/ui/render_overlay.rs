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
// the write latch); see `KagiApp::render_toasts`.

/// The rotating sync icon, at `size` px. **Every** rotating arrow in the app
/// comes from here: a static ⟳ promises motion it does not deliver, which is
/// what a reader reads as "stuck" (user report). `key` keeps each animation
/// instance distinct.
///
/// Reduce motion renders it still - there the stillness is the setting, not a
/// hung operation.
pub(crate) fn sync_spinner(
    size: f32,
    accent: u32,
    key: impl Into<gpui::ElementId>,
) -> gpui::AnyElement {
    use gpui::AnimationExt as _;
    const SPIN_MS: u64 = 700;
    let icon = gpui::svg()
        .path("icons/refresh-cw.svg")
        .flex_shrink_0()
        .w(theme::scaled_px(size))
        .h(theme::scaled_px(size))
        .text_color(rgb(accent));
    if theme::reduce_motion() {
        return icon.into_any_element();
    }
    icon.with_animation(
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

/// The big spinning sync icon shared by the busy snackbar and the
/// sync-flavoured toasts (`ToastKind::Sync`), so every sync-icon snackbar
/// looks identical. ~2× the header spinner (user request) so it reads
/// clearly as "working".
pub(crate) fn big_sync_icon(accent: u32, key: impl Into<gpui::ElementId>) -> gpui::AnyElement {
    sync_spinner(32.0, accent, key)
}

/// How far a toast card is displaced at `delta` of its slide animation.
///
/// Goes through `scaled_px` because the stack's own left inset does: at the
/// minimum 0.7x zoom a raw 12px slide against an 8.4px inset would put the card
/// back over the window edge, which is exactly what #709 was.
fn toast_slide_offset(delta: f32) -> gpui::Pixels {
    theme::scaled_px(TOAST_SLIDE_PX * delta)
}

impl gpui::Render for toast_stack::ToastStack {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut stack = div().flex().flex_col().gap_2();
        for toast in self.toasts() {
            let (accent, glyph) = match toast.kind {
                // A bullet, not ⟳: an Info toast reports something that already
                // happened ("Copied …"), and a rotating arrow that never turns
                // reads as an operation that never finished (user report).
                // In-flight messages use `ToastKind::Sync` and its spinner.
                ToastKind::Info => (theme().color_branch, "\u{2022}"), // •
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
            // Reduce motion: skip the horizontal slide (a vestibular trigger) and
            // show the toast at its resting state — visible while present, hidden
            // once leaving — so it appears/disappears without motion.
            use gpui::AnimationExt as _;
            let animated = if theme::reduce_motion() {
                if leaving {
                    card.opacity(0.0).into_any_element()
                } else {
                    card.into_any_element()
                }
            } else if leaving {
                card.with_animation(
                    ("kagi-toast-exit", id),
                    gpui::Animation::new(Duration::from_millis(TOAST_EXIT_MS))
                        .with_easing(gpui::quadratic),
                    |el, delta| el.ml(-toast_slide_offset(delta)).opacity(1.0 - delta),
                )
                .into_any_element()
            } else {
                card.with_animation(
                    ("kagi-toast-enter", id),
                    gpui::Animation::new(Duration::from_millis(TOAST_ENTER_MS))
                        .with_easing(gpui::ease_out_quint()),
                    |el, delta| el.ml(-toast_slide_offset(1.0 - delta)).opacity(delta),
                )
                .into_any_element()
            };
            stack = stack.child(animated);
        }
        stack
    }
}

// The Operation Log panel's `impl Render for OpLogPanel` moved to
// `src/ui/oplog_render.rs` (issue #468): it grew a variable-height
// `gpui::list` + a selectable detail block, and this file keeps the toasts.

impl KagiApp {
    /// The Welcome screen returns before the normal overlay compositor, so
    /// window-global modals need the same single-slot rendering here.
    pub(super) fn attach_welcome_window_modals(
        &self,
        el: gpui::Div,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let modal_focus = self.modal_focus.clone();
        el.when_some(self.remote_browse().cloned(), |el, modal| {
            el.child(super::e2e::measure_control(
                "active-modal/remote-browse",
                render_remote_browse(modal, modal_focus, cx),
            ))
        })
        .when_some(self.update_modal(), |el, _modal| {
            let Some((plan, _)) = self.update_available.as_ref() else {
                return el;
            };
            el.child(super::e2e::measure_control(
                "active-modal/update",
                render_update_modal(
                    plan.clone(),
                    self.update_installing,
                    self.update_status.clone(),
                    window,
                    cx,
                ),
            ))
        })
    }

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
        remote_browse: Option<RemoteBrowseModal>,
        update_modal: Option<UpdateModal>,
        stash_push_modal: Option<StashPushModal>,
        stash_apply_modal: Option<StashApplyModal>,
        cherry_pick_modal: Option<CherryPickModal>,
        revert_modal: Option<RevertModal>,
        delete_branch_modal: Option<DeleteBranchModal>,
        delete_remote_branch_modal: Option<DeleteRemoteBranchModal>,
        reset_current_modal: Option<ResetCurrentModal>,
        force_lease_push_modal: Option<ForceLeasePushModal>,
        push_tag_modal: Option<PushTagModal>,
        rebase_current_onto_modal: Option<RebaseCurrentOntoModal>,
        branch_cleanup_modal: Option<BranchCleanupModal>,
        discard_modal: Option<DiscardModal>,
        editor_dirty_guard_modal: Option<EditorDirtyGuardModal>,
        editor_fs_prompt_modal: Option<EditorFsPromptModal>,
        editor_delete_confirm_modal: Option<EditorDeleteConfirmModal>,
        file_menu: Option<file_menu::FileMenu>,
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
        // ── Operation Undo / Redo modal (T-UNDOREDO-001) ──
        .when_some(history_modal, |el, modal| {
            el.child(render_history_modal(modal, cx))
        })
        // ── Sequencer conflict-continue confirmation (ADR-0068) ──
        .when_some(conflict_continue_modal, |el, modal| {
            el.child(render_conflict_continue_modal(modal, cx))
        })
        // ── Abort confirmation (#704) — read from `self`, not passed in: the
        //    operation strip can open it with no conflict view in existence. ──
        .when_some(self.conflict_abort_modal().cloned(), |el, modal| {
            el.child(super::conflict_abort::render_conflict_abort_modal(
                modal, cx,
            ))
        })
        .when_some(amend_modal, |el, modal| {
            el.child(render_amend_modal(
                modal,
                &self.modal_section_overrides,
                self.modal_list_scroll.clone(),
                // #476 slice 3: read live, from the same panel the op resolves.
                self.panel_worktree_label(cx),
                cx,
            ))
        })
        .when_some(pop_modal, |el, modal| el.child(render_pop_modal(modal, cx)))
        // ── Stash drop modal overlay (ADR-0087) ─────────
        .when_some(self.pr_merge_modal().cloned(), |el, modal| {
            el.child(render_pr_merge_modal(modal, cx))
        })
        .when_some(self.pr_fields_modal().cloned(), |el, modal| {
            el.child(super::pr_fields::render_pr_fields_modal(modal, cx))
        })
        .when_some(stash_drop_modal, |el, modal| {
            el.child(render_stash_drop_modal(modal, cx))
        })
        // ── Repository owner-trust prompt (ADR-0160 / #310) ──
        .when_some(self.trust_repo_modal().cloned(), |el, modal| {
            el.child(super::trust_prompt::render_trust_repo_modal(modal, cx))
        })
        // ── Unlock-worktree confirmation ─────────────────
        .when_some(unlock_worktree_modal, |el, modal| {
            el.child(render_unlock_worktree_modal(modal, cx))
        })
        // ── Worktree lifecycle confirmations (issue #340) ──
        .when_some(self.app_notice().cloned(), |el, notice| {
            el.child(super::e2e::measure_control(
                "active-modal/app-notice",
                modal_renderers::modal_overlay(
                    div()
                        .p_4()
                        .bg(rgb(theme().bg_base))
                        .child(notice.message)
                        .child(
                            div()
                                .id("app-notice-dismiss")
                                .child(if notice.inspect.is_some() {
                                    Msg::AppReconcileInspect.t()
                                } else if notice.acknowledge.is_some() {
                                    Msg::AppReconcileConfirm.t()
                                } else {
                                    Msg::AppNoticeDismiss.t()
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.confirm_app_notice(cx);
                                })),
                        ),
                ),
            ))
        })
        .when_some(self.remove_worktree_modal().cloned(), |el, modal| {
            el.child(render_remove_worktree_modal(modal, cx))
        })
        .when_some(self.lock_worktree_modal().cloned(), |el, modal| {
            el.child(render_lock_worktree_modal(modal, cx))
        })
        .when_some(self.prune_worktrees_modal().cloned(), |el, modal| {
            el.child(render_prune_worktrees_modal(modal, cx))
        })
        .when_some(self.repair_worktrees_modal().cloned(), |el, modal| {
            el.child(render_repair_worktrees_modal(modal, cx))
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
        .when_some(remote_browse, |el, modal| {
            el.child(super::e2e::measure_control(
                "active-modal/remote-browse",
                render_remote_browse(modal, modal_focus.clone(), cx),
            ))
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
        .when_some(reset_current_modal, |el, modal| {
            el.child(render_reset_current_modal(modal, cx))
        })
        .when_some(force_lease_push_modal, |el, modal| {
            el.child(render_force_lease_push_modal(modal, cx))
        })
        .when_some(push_tag_modal, |el, modal| {
            el.child(render_push_tag_modal(modal, cx))
        })
        .when_some(rebase_current_onto_modal, |el, modal| {
            el.child(render_rebase_modal(modal, cx))
        })
        // ── Branch-cleanup modal overlay (ADR-0128) ──────
        .when_some(branch_cleanup_modal, |el, modal| {
            el.child(render_branch_cleanup_modal(modal, cx))
        })
        // ── Discard danger modal overlay (W17-DISCARD) ───
        .when_some(discard_modal, |el, modal| {
            el.child(render_discard_modal(
                modal,
                &self.modal_section_overrides,
                self.modal_list_scroll.clone(),
                // #476 slice 3: read live, from the same panel the op resolves.
                self.panel_worktree_label(cx),
                cx,
            ))
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
        // ── Sidebar PR context menu (GitHub Phase 1) ──
        .when_some(self.ui().pr_menu.clone(), |el, (pr, pos)| {
            el.child(render_pr_menu_overlay(pr, pos, window.viewport_size(), cx))
        })
        // ── Inspector/Compare file context menu (History/Edit/Copy) ──
        .when_some(self.inspector_file_menu, |el, (fi, pos)| {
            el.child(render_inspector_file_menu_overlay(
                fi,
                pos,
                window.viewport_size(),
                cx,
            ))
        })
        // ── Unstaged file context menu (right-click → Discard) ──
        .when_some(
            file_menu.filter(|menu| {
                self.active_session() == Some(menu.owner)
                    && self
                        .ui()
                        .commit_panel
                        .as_ref()
                        .is_some_and(|panel| panel.read(cx).owner == menu.owner)
            }),
            |el, menu| el.child(render_file_menu_overlay(menu, window.viewport_size(), cx)),
        )
        // ── Commit plan modal overlay (T025) ─────────────
        .when(commit_panel_open && commit_plan_modal.is_some(), |el| {
            if let Some(plan_modal) = commit_plan_modal.clone() {
                el.child(render_commit_plan_modal(plan_modal, cx))
            } else {
                el
            }
        })
        // ── Smart Commit modal overlay (single ActiveModal slot) ────
        .when_some(self.smart_commit_modal().cloned(), |el, modal| {
            el.child(render_smart_commit_modal(modal, cx))
        })
        // ── Auto-update modal overlay (ADR-0082) ──────────
        .when_some(update_modal, |el, _modal| {
            let Some((plan, _)) = self.update_available.as_ref() else {
                return el;
            };
            el.child(super::e2e::measure_control(
                "active-modal/update",
                render_update_modal(
                    plan.clone(),
                    self.update_installing,
                    self.update_status.clone(),
                    window,
                    cx,
                ),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{theme, TOAST_INSET_PX};

    /// #709: the cards slide by a negative margin from their inset position, so
    /// a travel longer than the inset puts them past the left window edge and
    /// the user sees a card cut in half for the length of the animation.
    ///
    /// Compares the value the renderer actually uses against the inset the
    /// stack is actually laid out at, at every zoom: the first attempt at this
    /// fix compared the two raw constants while the slide alone skipped
    /// `scaled_px`, and below 1.0x the card crossed the edge again.
    /// Holds `ENV_LOCK` and redirects `KAGI_LOG_DIR`, like every other zoom
    /// test: `set_zoom` drives a process-global atomic *and* writes
    /// settings.json, so without both this races the sibling zoom tests and
    /// saves into the developer's real `~/.kagi` (#740 review).
    #[test]
    fn the_toast_slide_never_crosses_the_window_edge() {
        let _g = crate::ui::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Restores on the way out, panic included: the settings store is
        // process-global, so leaving `KAGI_LOG_DIR` pointing at a deleted
        // tempdir would follow later tests in this binary around (#740 review).
        struct LogDir(Option<std::ffi::OsString>);
        impl Drop for LogDir {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(previous) => std::env::set_var("KAGI_LOG_DIR", previous),
                    None => std::env::remove_var("KAGI_LOG_DIR"),
                }
            }
        }
        let _log_dir = LogDir(std::env::var_os("KAGI_LOG_DIR"));
        let tmp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("KAGI_LOG_DIR", tmp.path());

        let before = theme::zoom();
        for zoom in [theme::ZOOM_MIN, 1.0, theme::ZOOM_MAX] {
            theme::set_zoom(zoom);
            let travel = super::toast_slide_offset(1.0);
            let inset = theme::scaled_px(TOAST_INSET_PX);
            assert!(
                travel <= inset,
                "at {zoom}x zoom a {travel:?} slide from a {inset:?} inset leaves the window"
            );
        }
        theme::set_zoom(before);
    }
}
