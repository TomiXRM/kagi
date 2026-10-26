//! Shared keyboard routing for the window-level modal slot.
//!
//! Both workspace and Welcome content pass through this wrapper. Surface-local
//! fallbacks stay workspace-only, but modal confirmation and cancellation cannot
//! diverge between render paths.

use super::*;

impl KagiApp {
    /// Diff-local overlays outrank notifications and compact projections.
    fn cancel_workspace_overlay(&mut self, window: &Window, cx: &mut Context<Self>) -> bool {
        if diff_selection::clear() {
            cx.notify();
            return true;
        }
        if self.pr_mode_visible()
            && super::pr_layout::Layout::for_window(self, window)
                .drawer
                .is_some()
        {
            if let Some(mode) = self.pr_mode_mut() {
                mode.companion = None;
            }
            cx.notify();
            return true;
        }
        false
    }

    fn cancel_workspace_key_target(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui::Focusable as _;
        use gpui_component::WindowExt as _;
        if self.pr_mode_visible() {
            if self.pr_mode().is_some_and(|m| m.active.is_some()) {
                self.pr_mode_home(cx);
            } else {
                self.toggle_pr_mode(cx);
            }
            return;
        }
        if self.ui().main_diff.is_some() {
            self.close_main_diff();
            cx.notify();
        } else if self.workspace_mode() == super::workspace_mode::WorkspaceMode::Graph {
            // Selection clearing precedes the final bottom-pane close. Each
            // Escape consumes only one layer; same-row select toggles off.
            if let Some(index) = self.ui().selected {
                self.select(index);
                cx.notify();
            } else if self.bottom_panel_open
                && !self.home_in_front()
                && !self.conflict_body_visible()
                && !window
                    .focused_input(cx)
                    .is_some_and(|input| input.focus_handle(cx).contains_focused(window, cx))
                && workspace::resolve_workspace(&self.workspace_inputs(false, false, false)).center
                    == workspace::CenterPane::CommitList
            {
                // Hand off every bottom descendant (selector, PTY, lists,
                // Activity controls), not just controls known to the renderer.
                if self
                    .bottom_panel_focus
                    .as_ref()
                    .is_some_and(|scope| scope.contains_focused(window, cx))
                {
                    if let Some(root) = &self.root_focus {
                        root.focus(window, cx);
                    }
                }
                self.bottom_panel_open = false;
                cx.notify();
            }
        }
    }

    /// An IME's Enter accepts the marked text; it must not also confirm a Git
    /// write. All six input-confirm cards keep InputState as their only text
    /// owner, so query that state rather than keeping a second composition flag.
    fn input_modal_is_composing(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        use modals::ActiveModal as M;
        let mut marked = |input: Option<&Entity<gpui_component::input::InputState>>| {
            input.is_some_and(|input| {
                input.update(cx, |state, cx| {
                    gpui::EntityInputHandler::marked_text_range(state, window, cx).is_some()
                })
            })
        };
        match self.active_modal.as_ref() {
            Some(M::CreateBranch(modal)) => marked(modal.input_state.as_ref()),
            Some(M::CreateTag(modal)) => marked(modal.input_state.as_ref()),
            Some(M::CreateWorktree(modal)) => {
                marked(modal.branch_state.as_ref()) || marked(modal.path_state.as_ref())
            }
            Some(M::StashPush(modal)) => marked(modal.input_state.as_ref()),
            Some(M::RenameBranch(modal)) => marked(modal.input_state.as_ref()),
            Some(M::SetUpstream(modal)) => marked(modal.input_state.as_ref()),
            _ => false,
        }
    }

    /// Danger sheets have no default action: root Return leaves them unchanged.
    /// Focused buttons consume their own Enter/Space before this root handler.
    pub(super) fn active_modal_has_no_default(&self) -> bool {
        use modals::ActiveModal as M;
        let danger_plan = |plan: &kagi_git::OperationPlan| {
            modal_renderers::plan_confirm_kind(plan.destructive, &plan.title, false)
                == button_style::ModalButtonKind::Destructive
        };
        match self.active_modal.as_ref() {
            Some(
                M::Amend(_)
                | M::StashDrop(_)
                | M::DeleteBranch(_)
                | M::DeleteRemoteBranch(_)
                | M::ResetCurrent(_)
                | M::ForceLeasePush(_)
                | M::BranchCleanup(_)
                | M::Discard(_)
                | M::OplogRestore(_)
                | M::ConflictAbort(_)
                | M::ConflictResolution(_)
                | M::EditorDirtyGuard(_)
                | M::EditorDeleteConfirm(_)
                | M::RemoveWorktree(_)
                | M::TerminalPaste(_)
                | M::PruneWorktrees(_),
            ) => true,
            Some(M::Checkout(m)) => danger_plan(&m.plan),
            Some(M::Pull(m)) => danger_plan(&m.plan),
            Some(M::Push(m)) => danger_plan(&m.plan),
            Some(M::BranchPlan(m)) => danger_plan(&m.plan),
            Some(M::History(m)) => danger_plan(&m.plan),
            Some(M::PrMerge(m)) => danger_plan(&m.plan),
            Some(M::RebaseCurrentOnto(m)) => danger_plan(&m.plan),
            Some(M::QueuedCommit(m)) => danger_plan(&m.plan),
            Some(M::PushTag(m)) => danger_plan(&m.plan),
            Some(M::Merge(m)) => danger_plan(&m.plan),
            Some(M::TrackingCheckout(m)) => danger_plan(&m.plan),
            Some(M::SwitchToLatest(m)) => danger_plan(&m.plan),
            Some(M::CherryPick(m)) => danger_plan(&m.plan),
            Some(M::Revert(m)) => danger_plan(&m.plan),
            Some(M::Pop(m)) => m.plan.as_deref().is_some_and(danger_plan),
            Some(M::ApplySuggestion(m)) => danger_plan(&m.plan),
            Some(M::AppNotice(notice)) => notice.release_armed,
            None
            | Some(
                M::RemoteBrowse(_)
                | M::Clone(_)
                | M::Update(_)
                | M::SmartCommit(_)
                | M::AnalyzeHelp(_)
                | M::About(_)
                | M::PrFields(_)
                | M::SetUpstream(_)
                | M::RenameBranch(_)
                | M::CreateBranch(_)
                | M::CreateTag(_)
                | M::CreateWorktree(_)
                | M::UnlockWorktree(_)
                | M::WorktreeLockReason(_)
                | M::LockWorktree(_)
                | M::RepairWorktrees(_)
                | M::RepoHealth(_)
                | M::StashPush(_)
                | M::StashApply(_)
                | M::ConflictContinue(_)
                | M::EditorFsPrompt(_)
                | M::TrustRepo(_),
            ) => false,
        }
    }

    /// Alert-style confirmations have no text field to take focus for them.
    /// Share the named-dialog focus trap, including the owner-trust prompt.
    pub(super) fn active_modal_needs_alert_trap(&self) -> bool {
        self.active_modal_has_no_default() || self.trust_repo_modal().is_some()
    }

    /// Cards rendered through input_dialog_frame own their replacement focus,
    /// including fieldless named tasks that reuse the shared modal handle.
    pub(super) fn active_modal_has_named_frame(&self) -> bool {
        use modals::ActiveModal as M;
        match self.active_modal.as_ref() {
            Some(
                M::CreateBranch(_)
                | M::CreateTag(_)
                | M::CreateWorktree(_)
                | M::StashPush(_)
                | M::RenameBranch(_)
                | M::SetUpstream(_)
                | M::WorktreeLockReason(_)
                | M::EditorFsPrompt(_)
                | M::RemoteBrowse(_)
                | M::PrFields(_)
                | M::Clone(_)
                | M::AnalyzeHelp(_)
                | M::About(_),
            ) => true,
            None
            | Some(
                M::Amend(_)
                | M::StashDrop(_)
                | M::DeleteBranch(_)
                | M::DeleteRemoteBranch(_)
                | M::ResetCurrent(_)
                | M::ForceLeasePush(_)
                | M::BranchCleanup(_)
                | M::Discard(_)
                | M::OplogRestore(_)
                | M::ConflictAbort(_)
                | M::ConflictResolution(_)
                | M::EditorDirtyGuard(_)
                | M::EditorDeleteConfirm(_)
                | M::RemoveWorktree(_)
                | M::PruneWorktrees(_)
                | M::Checkout(_)
                | M::Pull(_)
                | M::Push(_)
                | M::BranchPlan(_)
                | M::History(_)
                | M::PrMerge(_)
                | M::RebaseCurrentOnto(_)
                | M::QueuedCommit(_)
                | M::PushTag(_)
                | M::Merge(_)
                | M::TrackingCheckout(_)
                | M::SwitchToLatest(_)
                | M::CherryPick(_)
                | M::Revert(_)
                | M::Pop(_)
                | M::ApplySuggestion(_)
                | M::AppNotice(_)
                | M::Update(_)
                | M::SmartCommit(_)
                | M::UnlockWorktree(_)
                | M::LockWorktree(_)
                | M::RepairWorktrees(_)
                | M::RepoHealth(_)
                | M::StashApply(_)
                | M::ConflictContinue(_)
                | M::TerminalPaste(_)
                | M::TrustRepo(_),
            ) => false,
        }
    }

    /// Attach the complete key routing for the single modal slot. Both the
    /// workspace and Welcome render paths pass through this wrapper, so a new
    /// modal key cannot be wired on only one surface.
    pub(crate) fn attach_active_modal_key_routing(
        &mut self,
        content: gpui::AnyElement,
        workspace_fallbacks: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let escape = cx.listener(move |this, _: &CloseMainDiff, window, cx| {
            // The front-layer boundary owns modal/menu cancellation, including
            // the co-author ListBox's focus return. Do not close menus again here.
            if this.cancel_active_modal(cx) {
                return;
            }
            use gpui::Focusable as _;
            if !super::compact_sidebar::is_open(window, cx)
                && this
                    .sidebar
                    .filter
                    .as_ref()
                    .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window))
            {
                this.clear_sidebar_filter(window, cx);
                return;
            }
            if workspace_fallbacks && this.cancel_workspace_overlay(window, cx) {
                return;
            }
            if this
                .toast_stack
                .clone()
                .is_some_and(|stack| stack.update(cx, |stack, cx| stack.dismiss_newest(cx)))
            {
                return;
            }
            if workspace_fallbacks && super::compact_sidebar::is_open(window, cx) {
                super::compact_sidebar::dismiss(window, cx);
                cx.notify();
                return;
            }
            if workspace_fallbacks && this.compact.enabled && this.compact.popup_owner.is_some() {
                this.close_compact_composer(window, cx);
                cx.notify();
                return;
            }
            if workspace_fallbacks {
                this.cancel_workspace_key_target(window, cx);
            }
        });
        let composer = cx.listener(
            move |this, _: &super::compact::OpenCommitComposer, window, cx| {
                if !workspace_fallbacks
                    || !super::compact::for_window(window)
                    || this.has_modal_or_visible_plan(cx)
                    || this.workspace_mode() != super::workspace_mode::WorkspaceMode::Graph
                    || this.conflict_body_visible()
                    || !this.compact_shortcut_anchor_visible(window, cx)
                {
                    cx.propagate();
                    return;
                }
                if this.compact.popup_owner != this.active_session() {
                    this.open_commit_panel(window, cx);
                    cx.notify();
                }
            },
        );
        let enter = cx.listener(move |this, e: &KeyDownEvent, window, cx| {
            if std::env::var("KAGI_DEBUG_KEYS").as_deref() == Ok("1") {
                eprintln!(
                    "[kagi] key: {:?} char={:?}",
                    e.keystroke.key, e.keystroke.key_char
                );
            }
            let ks = &e.keystroke;
            if ks.key == "enter"
                && !ks.modifiers.platform
                && !ks.modifiers.control
                && !ks.modifiers.alt
                && !ks.modifiers.shift
            {
                if this.input_modal_is_composing(window, cx) {
                    cx.stop_propagation();
                    return;
                }
                if this.front_layer(cx) == front_layer::FrontLayer::Modal
                    && this.active_modal_has_no_default()
                {
                    cx.stop_propagation();
                    return;
                }
                if this.confirm_active_modal(window, cx) {
                    cx.stop_propagation();
                    return;
                }
                if workspace_fallbacks {
                    this.checkout_selected_commit(window, cx);
                }
                cx.notify();
            }
        });

        div()
            .size_full()
            .key_context("KagiWorkspace")
            .on_action(escape)
            .on_action(composer)
            .on_key_down(enter)
            .child(content)
            .into_any()
    }
}
