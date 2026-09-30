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

use gpui::{Context, Window};

use super::commands::MenuOverlay;
use super::KagiApp;

impl KagiApp {
    /// Remember the focus to return to. Kept from the first overlay when one
    /// replaces another, so the return target is never an overlay's own input.
    pub(super) fn capture_overlay_return_focus(&mut self, window: &Window, cx: &Context<Self>) {
        if self.pending_focus.is_none() {
            self.pending_focus = window.focused(cx).or_else(|| self.root_focus.clone());
        }
    }

    /// A modal with no text field owns Enter / Escape through the root, so it
    /// takes the root whatever held focus (the terminal, an input) when it
    /// opened. Applied on the next render: the openers have no `Window`.
    pub(super) fn focus_root_for_modal(&mut self) {
        self.pending_focus = self.root_focus.clone();
    }

    /// Apply the pending focus now.
    pub(super) fn apply_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.pending_focus.take() {
            window.focus(&focus, cx);
        }
    }

    /// Render pass: the closes that run without a `Window` (Escape through
    /// `cancel_active_modal`, the × and backdrop listeners) and the modal
    /// openers land here.
    pub(super) fn sync_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu_overlay.is_none() {
            self.apply_pending_focus(window, cx);
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
    /// the command acts where the user was; a modal it opens then asks for
    /// the root (or focuses its own input).
    pub(super) fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_overlay = None;
        self.apply_pending_focus(window, cx);
    }
}
