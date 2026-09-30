//! #817 / #812: menu overlays that take focus hand it back when they close.
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

use gpui::{Context, Window};

use super::commands::MenuOverlay;
use super::KagiApp;

impl KagiApp {
    /// Remember the focus to return to. Kept from the first overlay when one
    /// replaces another, so the return target is never an overlay's own input.
    pub(super) fn capture_overlay_return_focus(&mut self, window: &Window, cx: &Context<Self>) {
        if self.overlay_return_focus.is_none() {
            self.overlay_return_focus = window.focused(cx).or_else(|| self.root_focus.clone());
        }
    }

    /// Put focus back where the overlay found it.
    pub(super) fn restore_overlay_return_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(focus) = self.overlay_return_focus.take() {
            window.focus(&focus, cx);
        }
    }

    /// Render pass: the closes that run without a `Window` (Escape through
    /// `cancel_active_modal`, the × and backdrop listeners) land here.
    pub(super) fn sync_overlay_return_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu_overlay.is_none() {
            self.restore_overlay_return_focus(window, cx);
        }
    }

    /// Open Settings (toolbar button, menu, palette).
    pub(super) fn open_settings_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.capture_overlay_return_focus(window, cx);
        self.menu_overlay = Some(MenuOverlay::Settings);
        // Ensure an Ollama probe has run so the Smart Commit model picker is
        // usable even if the commit panel was never opened.
        self.refresh_smart_commit_detection(cx);
        // Seed the Analyze-ignore editor with the on-disk file contents.
        self.ensure_analyze_ignore_input(window, cx);
        cx.notify();
    }

    /// Close the palette before running its command: focus returns first, so
    /// the command acts where the user was, and a modal it opens receives
    /// Enter / Escape (or focuses its own input afterwards).
    pub(super) fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_overlay = None;
        self.restore_overlay_return_focus(window, cx);
    }
}
