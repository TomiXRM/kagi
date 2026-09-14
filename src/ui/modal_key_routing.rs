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
        if self.close_coauthor_menu(cx) {
            return;
        }
        if self.inspector_file_menu.is_some() {
            self.inspector_file_menu = None;
            cx.notify();
            return;
        }
        if self.pr_menu.is_some() {
            self.pr_menu = None;
            cx.notify();
            return;
        }
        if self.tag_menu.is_some() {
            self.tag_menu = None;
            cx.notify();
            return;
        }
        if self.pr_mode.is_some() {
            if self.pr_mode.as_ref().is_some_and(|m| m.active.is_some()) {
                self.pr_mode_home(cx);
            } else {
                self.toggle_pr_mode(cx);
            }
            return;
        }
        if self.commit_menu.is_some() {
            self.commit_menu = None;
            cx.notify();
        } else if self.branch_menu.is_some() {
            self.branch_menu = None;
            cx.notify();
        } else if self.main_diff.is_some() {
            self.close_main_diff();
            cx.notify();
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
