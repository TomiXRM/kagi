//! Shared keyboard routing for the window-level modal slot.
//!
//! Both workspace and Welcome content pass through this wrapper. Surface-local
//! fallbacks stay workspace-only, but modal confirmation and cancellation cannot
//! diverge between render paths.

use super::*;

impl KagiApp {
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

    fn cancel_workspace_key_target(&mut self, _window: &Window, cx: &mut Context<Self>) {
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
            // Last in the chain: with nothing else to close, Escape clears the
            // Graph selection (and with it the commit details pane). `select`
            // on the selected row is the existing toggle-off path.
            if let Some(index) = self.ui().selected {
                self.select(index);
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

    /// Return is the safe default, not an implicit selection of a danger action.
    /// Focused buttons consume their own Enter/Space before this root handler.
    fn active_modal_defaults_to_cancel(&self) -> bool {
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
                | M::EditorDirtyGuard(_)
                | M::EditorDeleteConfirm(_)
                | M::RemoveWorktree(_)
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
            if this.cancel_active_modal(cx) {
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
            if workspace_fallbacks {
                this.cancel_workspace_key_target(window, cx);
            }
        });
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
                    && this.active_modal_defaults_to_cancel()
                {
                    this.cancel_active_modal(cx);
                    cx.stop_propagation();
                    return;
                }
                if !this.confirm_active_modal(window, cx) && workspace_fallbacks {
                    this.checkout_selected_commit(window, cx);
                }
                cx.notify();
            }
        });

        div()
            .size_full()
            .on_action(escape)
            .on_key_down(enter)
            .child(content)
            .into_any()
    }
}
