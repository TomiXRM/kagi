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

use gpui::{Context, SharedString, Window};

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
    ///
    /// Settings yields to a modal that arrives while it is open (an async
    /// plan landing, #976 review): it draws behind the modal layer, so it
    /// closes, and a focus still in its trap goes to the root, where the
    /// modal's keys are routed. A modal that already took the focus (its own
    /// field) keeps it: Settings' return target is dropped, not applied.
    pub(super) fn sync_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.menu_overlay, Some(MenuOverlay::Settings)) && self.modal_in_front(cx) {
            self.menu_overlay = None;
            let trapped = (self.settings_focus.as_ref())
                .is_some_and(|trap| trap.contains_focused(window, cx));
            self.pending_focus = trapped.then(|| self.root_focus.clone()).flatten();
        }
        // Backstop (#976 review): while Settings is open with no modal in
        // front, the focus belongs in its trap. Whatever moved it to the
        // app's own panes behind — a command that focuses one, or a path not
        // yet known — it goes back to the container once, here, on the next
        // frame. A popup gpui-component's Root draws for a control in
        // Settings (the theme picker's list) is outside the app's tree and is
        // left alone.
        if matches!(self.menu_overlay, Some(MenuOverlay::Settings)) {
            if let Some(trap) = self.settings_focus.clone() {
                let behind = window.focused(cx).is_none()
                    || (self.root_focus.as_ref())
                        .is_some_and(|root| root.contains_focused(window, cx));
                if behind && !trap.is_focused(window) && !trap.contains_focused(window, cx) {
                    window.focus(&trap, cx);
                }
            }
        }
        // The menu overlays that hold no focus of their own (Info, the
        // branch picker) route their keys through the root, so a pending
        // focus lands while they are open too.
        if !matches!(
            self.menu_overlay,
            Some(MenuOverlay::Settings | MenuOverlay::CommandPalette)
        ) {
            self.apply_pending_focus(window, cx);
        }
    }

    /// A command that acts on the panes behind Settings (open the terminal,
    /// New Tab, ...) closes Settings first and drops its return target, so
    /// the focus the command gives lands on a screen with no trap over it
    /// (#976 review). The one path for every such command.
    pub(super) fn close_settings_for_command(&mut self) {
        if matches!(self.menu_overlay, Some(MenuOverlay::Settings)) {
            self.menu_overlay = None;
            self.pending_focus = None;
        }
    }

    /// Another menu overlay replaces Settings (About / Keyboard Shortcuts /
    /// the branch picker from the native menu, #976 review): the trap is
    /// unmounted with it, so the focus must not stay there. Those overlays
    /// own Escape through the root, so the focus goes to the root on the
    /// next render pass ([`Self::sync_pending_focus`]).
    pub(super) fn leave_settings_for_overlay(&mut self) {
        if matches!(self.menu_overlay, Some(MenuOverlay::Settings)) {
            self.pending_focus = self.root_focus.clone();
        }
    }

    /// A modal drawn in front of the menu overlays: the modal slot, or the
    /// Commit Panel's plan confirmation (its own storage) — the latter only
    /// when it is drawn, under the same condition as `attach_modal_layer`:
    /// not with Home in front, and only while the panel is open (#976
    /// review). A plan kept behind Home does not block Settings.
    fn modal_in_front(&self, cx: &Context<Self>) -> bool {
        self.has_active_modal()
            || (!self.home_in_front()
                && self.ui().commit_panel_open
                && (self.ui().commit_panel.as_ref())
                    .is_some_and(|panel| panel.read(cx).state.plan_modal.is_some()))
    }

    /// Open Settings (toolbar button, menu, palette).
    ///
    /// Focus moves into the panel — to its trap container, not to a control
    /// (#974): until it does, Tab still goes wherever focus was, e.g. the
    /// terminal's shell, and Escape (bound `!Terminal`) cannot close
    /// Settings. The container is no Tab stop and draws no ring, so opening
    /// by pointer shows none; the first Tab enters the panel's first stop.
    /// Closing returns focus as above.
    ///
    /// Not while a modal is in front (#976 review): Settings draws behind
    /// the modal layer — the modal slot and the Commit Panel's plan alike —
    /// so taking the focus into it would leave the visible modal's field and
    /// keys dead. The modal is finished or cancelled first, as the
    /// one-modal-at-a-time rule has it.
    pub(super) fn open_settings_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.modal_in_front(cx) {
            return;
        }
        self.capture_overlay_return_focus(window, cx);
        self.menu_overlay = Some(MenuOverlay::Settings);
        let trap = self
            .settings_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        window.focus(&trap, cx);
        // The Select entity survives closing Settings. Menu and palette theme
        // changes bypass its Confirm event, so sync its highlighted row on open.
        if let Some(select) = &self.theme_select {
            let slug = SharedString::from(super::theme::theme().slug.to_string());
            select.update(cx, |state, cx| {
                state.set_selected_value(&slug, window, cx);
            });
        }
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
