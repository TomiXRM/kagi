//! Shared keyboard routing for the window-level modal slot.
//!
//! Both workspace and Welcome content pass through this wrapper. Surface-local
//! fallbacks stay workspace-only, but modal confirmation and cancellation cannot
//! diverge between render paths.

use super::*;

impl KagiApp {
    fn cancel_workspace_key_target(&mut self, cx: &mut Context<Self>) {
        if diff_selection::clear() {
            cx.notify();
            return;
        }
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

    /// Attach the complete key routing for the single modal slot. Both the
    /// workspace and Welcome render paths pass through this wrapper, so a new
    /// modal key cannot be wired on only one surface.
    pub(crate) fn attach_active_modal_key_routing(
        &mut self,
        content: gpui::AnyElement,
        workspace_fallbacks: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let escape = cx.listener(move |this, _: &CloseMainDiff, _window, cx| {
            if !this.cancel_active_modal(cx) && workspace_fallbacks {
                this.cancel_workspace_key_target(cx);
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
                if !this.confirm_active_modal(cx) && workspace_fallbacks {
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
