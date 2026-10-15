//! Host destruction protects every retained editor, not just the visible pane.
use super::{EditorPendingIntent, FooterStatus, KagiApp, Msg, ToastKind};
use gpui::{App, Context};

impl KagiApp {
    #[cfg(feature = "gui-e2e")]
    pub fn request_update_relaunch_for_e2e(
        &mut self,
        relaunch: crate::update::Relaunch,
        cx: &mut Context<Self>,
    ) {
        self.request_update_relaunch(relaunch, cx);
    }

    pub(crate) fn request_update_relaunch(
        &mut self,
        relaunch: crate::update::Relaunch,
        cx: &mut Context<Self>,
    ) {
        let intent = EditorPendingIntent::Relaunch(relaunch);
        if !self.host_dirty_guard_open()
            && self.may_close_host()
            && self.host_dirty_buffer_count(cx) == 0
        {
            self.finish_host_close(intent, cx);
        } else {
            self.hold_host_close(intent, cx);
        }
    }

    pub(crate) fn host_dirty_buffer_count(&self, cx: &App) -> usize {
        self.ui
            .values()
            .filter_map(|ui| ui.editor_workspace.as_ref())
            .map(|editor| editor.read(cx).dirty_buffer_count())
            .sum()
    }

    pub(crate) fn host_dirty_guard_open(&self) -> bool {
        self.editor_dirty_guard_modal().is_some_and(|modal| {
            matches!(
                modal.intent,
                EditorPendingIntent::Quit
                    | EditorPendingIntent::CloseWindow(_)
                    | EditorPendingIntent::Relaunch(_)
            )
        })
    }

    /// Shared by Quit, update relaunch, Close Window and the native close callback.
    /// Existing write/clone holds remain authoritative before and after approval.
    pub(crate) fn hold_host_close(
        &mut self,
        intent: EditorPendingIntent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.host_dirty_guard_open() {
            if let Some(modal) = self.editor_dirty_guard_modal_mut() {
                if (matches!(intent, EditorPendingIntent::Quit)
                    && matches!(modal.intent, EditorPendingIntent::CloseWindow(_)))
                    || matches!(intent, EditorPendingIntent::Relaunch(_))
                {
                    modal.intent = intent;
                    cx.notify();
                }
            }
            return true;
        }
        // An installed update must retain its continuation even when there is
        // no dirty file yet and an operation still prevents process exit.
        if matches!(intent, EditorPendingIntent::Relaunch(_)) && !self.may_close_host() {
            self.open_editor_dirty_guard(intent, cx);
            self.explain_host_save_wait(cx);
            return true;
        }
        if self.hold_host_operation(cx) {
            return true;
        }
        if self.host_dirty_buffer_count(cx) == 0 {
            return false;
        }
        self.open_editor_dirty_guard(intent, cx);
        true
    }

    pub(super) fn finish_host_close(
        &mut self,
        intent: EditorPendingIntent,
        cx: &mut Context<Self>,
    ) {
        if self.hold_host_operation(cx) {
            return;
        }
        match intent {
            EditorPendingIntent::Quit => crate::ui::commands::quit_app(cx),
            EditorPendingIntent::Relaunch(relaunch) => {
                #[cfg(feature = "gui-e2e")]
                if super::super::e2e::capture_update_relaunch(cx) {
                    return;
                }
                super::super::settings::flush();
                relaunch.spawn_and_exit();
            }
            EditorPendingIntent::CloseWindow(window) => {
                let owner = cx.entity().downgrade();
                // Modal callbacks already borrow this window; update it after dispatch.
                cx.defer(move |cx| {
                    let _ = window.update(cx, |_, window, cx| {
                        let held = owner
                            .update(cx, |app, cx| app.hold_host_operation(cx))
                            .unwrap_or(true);
                        if !held {
                            window.remove_window();
                        }
                    });
                });
            }
            _ => unreachable!("only host-close intents reach this boundary"),
        }
    }

    pub(super) fn stop_editor_guard_save(&mut self) {
        if let Some(modal) = self.editor_dirty_guard_modal_mut() {
            modal.saving = None;
        }
    }

    pub(super) fn explain_host_save_wait(&mut self, cx: &mut Context<Self>) {
        let message = Msg::EditorHostSaveWaiting.t();
        self.push_toast(ToastKind::Error, message, cx);
        self.status_footer = FooterStatus::Failed(message.into());
        cx.notify();
    }

    /// Save All serializes through the existing editor write-admission path. A failed
    /// or externally changed file leaves the guard and its original intent intact.
    /// Cancel retires the intent; an already admitted save may finish but cannot exit.
    pub fn save_host_dirty_guard(&mut self, cx: &mut Context<Self>) {
        if !self.host_dirty_guard_open()
            || self
                .editor_dirty_guard_modal()
                .is_some_and(|m| m.saving.is_some())
        {
            return;
        }
        if !self.may_close_host() {
            self.explain_host_save_wait(cx);
            return;
        }
        let editor = self
            .tabs
            .iter()
            .filter_map(|tab| self.ui.get(&tab.session))
            .filter_map(|ui| ui.editor_workspace.as_ref())
            .find(|editor| editor.read(cx).any_dirty())
            .cloned();
        let Some(editor) = editor else {
            if let Some(modal) = self.take_editor_dirty_guard_modal() {
                self.finish_host_close(modal.intent, cx);
            }
            return;
        };
        if let Some(modal) = self.editor_dirty_guard_modal_mut() {
            modal.saving = Some(editor.entity_id());
        }
        if !editor.update(cx, |editor, cx| editor.save_next_dirty(cx)) {
            self.stop_editor_guard_save();
        }
        cx.notify();
    }
}
