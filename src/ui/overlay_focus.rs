//! #817 / #812: keyboard focus around the menu overlays and the plan modals.
//!
//! The Command Palette focuses its search input on open, and Settings' theme
//! `Select` / Analyze-ignore editor take focus when used. Both entities are
//! retained after the overlay closes, so the window's focus is left on an
//! element that is no longer drawn. gpui then dispatches keys from the
//! dispatch tree's root, above the workspace: the modal slot's Enter / Escape
//! (`attach_active_modal_key_routing`) and the list's ↑↓ never run. That is
//! why a Push plan opened from the palette ignored Enter and Escape while the
//! same modal opened from the branch menu (focus still on the root) did not.
//!
//! Opening such an overlay records where focus was; closing it — by whatever
//! route: Escape, ×, the backdrop, or running a palette command — puts it
//! back (or on the root when nothing was focused).
//!
//! Where focus was is not always where a modal's keys work, though: the
//! embedded terminal takes focus when it starts (at launch, with the bottom
//! panel open), and Escape is bound `!Terminal` so the terminal keeps it. A
//! plan modal without a text field therefore asks for the root on open, the
//! same rule the conflict Abort confirmation follows (#755).

use gpui::{Context, FocusHandle, Window};

use super::commands::MenuOverlay;
use super::front_layer::FrontLayer;
use super::KagiApp;
use crate::app::SessionId;

/// The focus belongs to the screen on which an overlay was opened, not to a
/// retained entity from a tab that may have been closed or switched away.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FocusScreen {
    Home,
    Session(SessionId),
    None,
}

#[derive(Clone)]
pub(super) struct PendingFocus {
    focus: FocusHandle,
    screen: FocusScreen,
}

#[derive(Clone)]
pub(super) struct AlertReturnFocus {
    opener: Option<PendingFocus>,
    trap: FocusHandle,
}

impl AlertReturnFocus {
    pub(super) fn for_settings(window: &Window, cx: &gpui::App, trap: FocusHandle) -> Self {
        Self {
            opener: window.focused(cx).map(|focus| PendingFocus {
                focus,
                screen: FocusScreen::None,
            }),
            trap,
        }
    }

    pub(super) fn restore_settings(self, window: &mut Window, cx: &mut gpui::App) {
        if let Some(opener) = self.opener {
            window.focus(&opener.focus, cx);
        }
    }
}

impl KagiApp {
    fn focus_screen(&self) -> FocusScreen {
        if self.home_in_front() {
            FocusScreen::Home
        } else {
            self.active_session()
                .map(FocusScreen::Session)
                .unwrap_or(FocusScreen::None)
        }
    }

    fn pending_root_focus(&self) -> Option<PendingFocus> {
        self.root_focus.clone().map(|focus| PendingFocus {
            focus,
            screen: self.focus_screen(),
        })
    }

    /// Remember the focus to return to. Kept from the first overlay when one
    /// replaces another, so the return target is never an overlay's own input.
    pub(super) fn capture_overlay_return_focus(&mut self, window: &Window, cx: &Context<Self>) {
        if self.pending_focus.is_none() {
            self.pending_focus =
                window
                    .focused(cx)
                    .or_else(|| self.root_focus.clone())
                    .map(|focus| PendingFocus {
                        focus,
                        screen: self.focus_screen(),
                    });
        }
    }

    /// A modal with no text field owns Enter / Escape through the root, so it
    /// takes the root whatever held focus (the terminal, an input) when it
    /// opened. Applied on the next render: the openers have no `Window`.
    pub(super) fn focus_root_for_modal(&mut self) {
        self.pending_focus = self.pending_root_focus();
    }

    /// Queue a stable control using the existing screen and dispatch-membership checks.
    pub(super) fn restore_control_focus(&mut self, focus: FocusHandle) {
        self.pending_focus = Some(PendingFocus {
            focus,
            screen: self.focus_screen(),
        });
    }

    /// A tab/Home/repository departure owns dismissal, even if a menu command
    /// already closed the dropdown before navigation. BranchPicker snapshots
    /// the old repository's branches; all menu overlays belong to the screen
    /// that opened them. Never infer departure from a previous pending focus.
    pub(super) fn close_departing_screen_overlays(&mut self) {
        if self.menu_overlay.is_some() || self.platform_menu_open.is_some() {
            self.menu_overlay = None;
            self.platform_menu_open = None;
            self.pending_focus = self.pending_root_focus();
        }
    }

    /// Apply the pending focus only if its screen and its element still belong
    /// to the most recently drawn workspace. GPUI retains focus handles for
    /// unmounted elements, so a live handle alone is not a valid return target.
    pub(super) fn apply_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pending) = self.pending_focus.take() {
            let valid = pending.screen == self.focus_screen()
                && self
                    .root_focus
                    .as_ref()
                    .is_some_and(|root| root.contains(&pending.focus, window));
            let focus = if valid {
                Some(pending.focus)
            } else {
                self.root_focus.clone()
            };
            if let Some(focus) = focus {
                window.focus(&focus, cx);
                self.restored_focus = valid.then_some(focus);
            }
        }
    }

    /// Render pass: the closes that run without a `Window` (Escape through
    /// `cancel_active_modal`, the × and backdrop listeners) and the modal
    /// openers land here.
    ///
    /// Settings yields to a visible modal that arrives while it is open
    /// (#976 review). Its return target is dropped rather than applied.
    pub(super) fn sync_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_modal_needs_alert_trap()
            && self.front_layer(cx) == FrontLayer::Modal
            && self.alert_return_focus.is_none()
        {
            let trap = self
                .modal_focus
                .get_or_insert_with(|| cx.focus_handle())
                .clone();
            self.alert_return_focus = Some(AlertReturnFocus {
                opener: window.focused(cx).map(|focus| PendingFocus {
                    focus,
                    screen: self.focus_screen(),
                }),
                trap,
            });
        }
        // A restored control may remain drawn during its close animation and
        // unmount later. Only rescue that exact focus; user-directed focus
        // changes must never be stolen by an old restore target.
        if self.check_restored_focus(window, cx) {
            // This render still sees the previous frame's dispatch tree: the
            // control may unmount in the very frame being drawn (the last
            // frame of its close animation), after which no frame is due.
            // Check again once this frame has been drawn, against its tree,
            // so the rescue needs neither another frame nor an input
            // (#976 review).
            cx.defer_in(window, |this, window, cx| {
                this.check_restored_focus(window, cx);
            });
        }
        let menu_visible = self.visible_platform_menu_section().is_some();
        if menu_visible {
            self.capture_overlay_return_focus(window, cx);
        }
        if matches!(self.menu_overlay, Some(MenuOverlay::Settings))
            && self.has_modal_or_visible_plan(cx)
        {
            self.menu_overlay = None;
            self.pending_focus = (!self.active_modal_input_focused(window, cx))
                .then(|| self.pending_root_focus())
                .flatten();
        }
        if !matches!(self.menu_overlay, Some(MenuOverlay::Settings)) {
            self.clear_analyze_reset_alert(window.window_handle().window_id());
            self.clear_settings_smart_consent(window.window_handle().window_id());
        }
        // The menu sits above Settings and input modals. Release their focus
        // while it is open; when it closes, the existing pending/restored
        // membership checks return focus to the still-drawn owner.
        if menu_visible {
            // An input modal may arrive above Settings while this menu is
            // open. Its InputState focuses during modal render, after the
            // Settings yield. Save that live input before moving focus back
            // to the root; the existing membership check restores it on close.
            if self.active_modal_input_focused(window, cx) {
                self.pending_focus = window.focused(cx).map(|focus| PendingFocus {
                    focus,
                    screen: self.focus_screen(),
                });
            }
            if let Some(root) = &self.root_focus {
                if !root.is_focused(window) {
                    window.focus(root, cx);
                }
            }
        }
        // The palette keeps its InputState after the dropdown takes the root.
        // Returning to the palette must focus that same input, not apply the
        // opener's pending focus (which belongs behind the palette).
        if !menu_visible
            && matches!(self.menu_overlay, Some(MenuOverlay::CommandPalette))
            && self.front_layer(cx) == FrontLayer::Menu
            && !self.workspace_menus_covered(cx)
            && self
                .root_focus
                .as_ref()
                .is_some_and(|root| root.is_focused(window))
        {
            if let Some(input) = &self.command_palette_input {
                input.update(cx, |state, cx| state.focus(window, cx));
            }
        }
        // Backstop (#976 review): only when Settings is the visible front
        // layer does focus belong in its trap. A gpui-component popup drawn
        // outside the app's tree is left alone.
        if self.front_layer(cx) == FrontLayer::Settings {
            let trap = self
                .settings_smart_consent()
                .filter(|alert| alert.window == window.window_handle().window_id())
                .map(|alert| alert.focus.clone())
                .or_else(|| {
                    self.analyze_reset_alert()
                        .filter(|alert| alert.window == window.window_handle().window_id())
                        .map(|alert| alert.focus.clone())
                })
                .or_else(|| self.settings_focus.clone());
            if let Some(trap) = trap {
                let behind = window.focused(cx).is_none()
                    || (self.root_focus.as_ref())
                        .is_some_and(|root| root.contains_focused(window, cx));
                if behind && !trap.is_focused(window) && !trap.contains_focused(window, cx) {
                    window.focus(&trap, cx);
                }
            }
        }
        // Focus-owning overlays retain their opener until they close.
        if !menu_visible
            && !matches!(
                self.menu_overlay,
                Some(
                    MenuOverlay::Settings
                        | MenuOverlay::CommandPalette
                        | MenuOverlay::BranchPicker { .. }
                        | MenuOverlay::Info { .. }
                )
            )
        {
            self.apply_pending_focus(window, cx);
        }
        if !menu_visible
            && self.front_layer(cx) == FrontLayer::Modal
            && self.active_modal_needs_alert_trap()
        {
            // Uniform rows reveal during the next layout. A retained handle
            // owned by this restore card is valid before it mounts there.
            let inspection_owns_focus = self
                .oplog_restore_modal()
                .is_some_and(|modal| modal.inspection.owns_row_focus(window));
            let focus = self.modal_focus.get_or_insert_with(|| cx.focus_handle());
            if let Some(saved) = self.alert_return_focus.as_mut() {
                if saved.trap != *focus {
                    saved.trap = focus.clone();
                }
            }
            if !inspection_owns_focus
                && !focus.is_focused(window)
                && !focus.contains_focused(window, cx)
            {
                window.focus(focus, cx);
            }
        }
        if !self.active_modal_needs_alert_trap() {
            if let Some(saved) = self.alert_return_focus.take() {
                let trapped =
                    saved.trap.is_focused(window) || saved.trap.contains_focused(window, cx);
                // A new named frame may intentionally reuse modal_focus.
                // Its own focus is not a stranded alert focus.
                let new_owner = self.active_modal_has_named_frame()
                    || self.active_modal_input_focused(window, cx);
                if trapped && !new_owner && !menu_visible {
                    let opener = (!self.has_active_modal())
                        .then_some(saved.opener)
                        .flatten()
                        .filter(|opener| {
                            opener.screen == self.focus_screen()
                                && self
                                    .root_focus
                                    .as_ref()
                                    .is_some_and(|root| root.contains(&opener.focus, window))
                        });
                    if let Some(focus) = opener
                        .map(|opener| opener.focus)
                        .or_else(|| self.root_focus.clone())
                    {
                        window.focus(&focus, cx);
                        self.restored_focus = Some(focus);
                    }
                }
            }
        }
    }

    /// Watch a focused surface that may disappear without a window-aware close.
    /// Use the same exact-handle guard as overlay restoration, including a check
    /// after this paint: the previous dispatch tree can still contain the body.
    pub(super) fn track_retiring_focus(
        &mut self,
        focus: gpui::FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.restored_focus = Some(focus);
        cx.defer_in(window, |this, window, cx| {
            this.check_restored_focus(window, cx);
        });
    }

    /// The restored-focus check, against the frame drawn last. Returns
    /// whether the restored control is still being tracked.
    fn check_restored_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(restored) = self.restored_focus.take() else {
            return false;
        };
        if window.focused(cx).as_ref() != Some(&restored) {
            return false;
        }
        if self
            .root_focus
            .as_ref()
            .is_some_and(|root| root.contains(&restored, window))
        {
            self.restored_focus = Some(restored);
            return true;
        }
        if let Some(root) = &self.root_focus {
            window.focus(root, cx);
        }
        false
    }

    /// A command acting on panes behind Settings closes it and discards its
    /// return target. Move focus to the visible root as well: when a command
    /// hides the focused terminal (rather than opening it), neither its old
    /// handle nor Settings' now-unmounted trap may receive the next key.
    pub(super) fn close_settings_for_command(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.menu_overlay, Some(MenuOverlay::Settings)) {
            self.menu_overlay = None;
            self.pending_focus = None;
            if let Some(root) = &self.root_focus {
                window.focus(root, cx);
            }
        }
    }

    /// A menu overlay or named task replaces Settings (Keyboard Shortcuts,
    /// About, or the native-menu branch picker). Release the unmounted trap;
    /// named dialogs take their own focus, and ordinary overlays use the root.
    pub(super) fn leave_settings_for_overlay(&mut self) {
        self.with_ui(|ui| ui.retire_issue_conversation_scope());
        if matches!(self.menu_overlay, Some(MenuOverlay::Settings)) {
            self.pending_focus = self.pending_root_focus();
        }
    }

    /// Open or raise Settings (toolbar button, menu, palette). macOS uses the
    /// non-modal native host; the following focus rules apply to the portable
    /// overlay only.
    ///
    /// Portable focus moves into the panel — to its trap container, not a control
    /// (#974): until it does, Tab still goes wherever focus was, e.g. the
    /// terminal's shell, and Escape (bound `!Terminal`) cannot close
    /// Settings. The container is no Tab stop and draws no ring, so opening
    /// by pointer shows none; the first Tab enters the panel's first stop.
    /// Closing returns focus as above.
    ///
    /// A modal or a drawn commit plan blocks it — also when a platform menu
    /// dropdown over them is the front layer (#976 review); an undrawn plan
    /// behind Home does not.
    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        {
            #[cfg(feature = "gui-e2e")]
            let native = cx
                .try_global::<super::settings_view::SettingsHostForE2e>()
                .is_none_or(|host| host.native);
            #[cfg(not(feature = "gui-e2e"))]
            let native = true;
            if native {
                self.close_layers_above(super::front_layer::LayerKind::MenuOverlay, cx);
                let owner = cx.entity();
                cx.defer(move |cx| super::settings_window::open_or_raise(owner, cx));
                return;
            }
        }
        if self.has_modal_or_visible_plan(cx) {
            return;
        }
        // A menu drawn above Settings' layer (the PR / filter / Inspector
        // file menus, the platform dropdown) would keep the front and leave
        // Settings' Tab cycling unseen behind it (#976 review).
        self.close_layers_above(super::front_layer::LayerKind::MenuOverlay, cx);
        self.capture_overlay_return_focus(window, cx);
        self.with_ui(|ui| ui.retire_issue_conversation_scope());
        self.menu_overlay = Some(MenuOverlay::Settings);
        let trap = self
            .settings_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        window.focus(&trap, cx);
        // Ensure an Ollama probe has run so the Smart Commit model picker is
        // usable even if the commit panel was never opened.
        self.refresh_smart_commit_detection(cx);
        // Seed the Analyze-ignore editor with the on-disk file contents.
        self.ensure_analyze_ignore_input(window, cx);
        cx.notify();
    }

    /// Close the palette before running its command: focus returns first, so
    /// the command acts where the user was; a modal it opens then asks for
    /// the root (or focuses its own input).
    pub(super) fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_overlay = None;
        self.apply_pending_focus(window, cx);
    }
}
