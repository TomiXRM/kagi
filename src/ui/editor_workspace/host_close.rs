//! Host destruction protects every retained editor, not just the visible pane.
use super::{EditorDirtyGuardModal, EditorPendingIntent, FooterStatus, KagiApp, Msg, ToastKind};
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
        self.update_installing = false;
        self.update_status = Some(Msg::UpdateRestartReady.t().into());
        self.update_pending_relaunch = Some(relaunch);
        // Installation finishes asynchronously. Only its own presentation may
        // be cleared; a newer user decision keeps the single modal slot.
        self.clear_update_modal();
        if self.has_active_modal() {
            cx.notify();
            return;
        }
        if self.may_close_host() && self.host_dirty_buffer_count(cx) == 0 {
            self.finish_host_close(EditorPendingIntent::Relaunch, cx);
            return;
        }
        if self.offer_editor_dirty_guard_from_async(EditorDirtyGuardModal {
            intent: EditorPendingIntent::Relaunch,
            saving: None,
            draft_save_token: None,
            conflict_discard_armed: false,
        }) {
            klog!("editor-ws: dirty-guard");
            if !self.may_close_host() {
                self.explain_host_save_wait(cx);
            } else if self.failed_conflict_draft_count(cx) > 0
                && !self.dirty_guard_has_failed_conflict_draft(cx)
            {
                self.flush_host_conflict_drafts(cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn retry_update_relaunch(&mut self, cx: &mut Context<Self>) {
        if self.update_pending_relaunch.is_none() {
            return;
        }
        self.clear_update_modal();
        let intent = EditorPendingIntent::Relaunch;
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
        let editors: usize = self
            .ui
            .values()
            .filter_map(|ui| ui.editor_workspace.as_ref())
            .map(|editor| editor.read(cx).dirty_buffer_count())
            .sum();
        editors + self.failed_conflict_draft_count(cx)
    }

    pub(crate) fn failed_conflict_draft_count(&self, cx: &App) -> usize {
        self.ui
            .values()
            .filter_map(|ui| ui.conflict.as_ref())
            .filter(|pane| pane.read(cx).has_unflushed_drafts())
            .count()
    }

    pub(crate) fn failed_conflict_draft_for(
        &self,
        session: crate::app::SessionId,
        cx: &App,
    ) -> bool {
        self.ui
            .get(&session)
            .and_then(|ui| ui.conflict.as_ref())
            .is_some_and(|pane| pane.read(cx).has_unflushed_drafts())
    }

    pub(crate) fn dirty_guard_editor_buffer_count(&self, cx: &App) -> usize {
        let session = self
            .editor_dirty_guard_modal()
            .and_then(|modal| match modal.intent {
                EditorPendingIntent::CloseRepoTab(session) => Some(session),
                _ => None,
            });
        self.ui
            .iter()
            .filter(|(id, _)| session.is_none_or(|session| **id == session))
            .filter_map(|(_, ui)| ui.editor_workspace.as_ref())
            .map(|editor| editor.read(cx).dirty_buffer_count())
            .sum()
    }

    pub(crate) fn dirty_guard_has_unflushed_conflict_draft(&self, cx: &App) -> bool {
        self.editor_dirty_guard_modal()
            .is_some_and(|modal| match modal.intent {
                EditorPendingIntent::CloseRepoTab(session) => {
                    self.failed_conflict_draft_for(session, cx)
                }
                EditorPendingIntent::Quit
                | EditorPendingIntent::CloseWindow(_)
                | EditorPendingIntent::Relaunch => self.failed_conflict_draft_count(cx) > 0,
                _ => false,
            })
    }

    pub(crate) fn dirty_guard_has_failed_conflict_draft(&self, cx: &App) -> bool {
        let Some(modal) = self.editor_dirty_guard_modal() else {
            return false;
        };
        let session = match modal.intent {
            EditorPendingIntent::CloseRepoTab(session) => Some(session),
            EditorPendingIntent::Quit
            | EditorPendingIntent::CloseWindow(_)
            | EditorPendingIntent::Relaunch => None,
            _ => return false,
        };
        self.ui
            .iter()
            .filter(|(id, _)| session.is_none_or(|session| **id == session))
            .filter_map(|(_, ui)| ui.conflict.as_ref())
            .any(|pane| pane.read(cx).draft_persist_failed.is_some())
    }

    pub(crate) fn host_dirty_guard_open(&self) -> bool {
        self.editor_dirty_guard_modal().is_some_and(|modal| {
            matches!(
                modal.intent,
                EditorPendingIntent::Quit
                    | EditorPendingIntent::CloseWindow(_)
                    | EditorPendingIntent::Relaunch
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
                    || matches!(intent, EditorPendingIntent::Relaunch)
                {
                    modal.intent = intent;
                    cx.notify();
                }
            }
            return true;
        }
        // An installed update must retain its continuation even when there is
        // no dirty file yet and an operation still prevents process exit.
        if matches!(intent, EditorPendingIntent::Relaunch) && !self.may_close_host() {
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
        if self.failed_conflict_draft_count(cx) > 0
            && !self.dirty_guard_has_failed_conflict_draft(cx)
        {
            self.flush_host_conflict_drafts(cx);
        }
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
            EditorPendingIntent::Relaunch => {
                let Some(relaunch) = self.update_pending_relaunch.take() else {
                    return;
                };
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
            modal.draft_save_token = None;
        }
    }

    pub(crate) fn host_close_hold_message(&self, restart: bool) -> Msg {
        if self.app_sessions.has_leases() && self.app_sessions.running_lease().is_none() {
            Msg::EditorHostRecoveryRequired
        } else if restart {
            Msg::UpdateRestartWaiting
        } else {
            Msg::EditorHostSaveWaiting
        }
    }

    pub(super) fn explain_host_save_wait(&mut self, cx: &mut Context<Self>) {
        let restart = self
            .editor_dirty_guard_modal()
            .is_some_and(|modal| matches!(modal.intent, EditorPendingIntent::Relaunch))
            && self.host_dirty_buffer_count(cx) == 0;
        let message = self.host_close_hold_message(restart).t();
        self.push_toast(ToastKind::Error, message, cx);
        self.status_footer = FooterStatus::Failed(message.into());
        cx.notify();
    }

    /// Save All serializes through the existing editor write-admission path. A failed
    /// or externally changed file leaves the guard and its original intent intact.
    /// Cancel retires the intent; an already admitted save may finish but cannot exit.
    pub fn save_host_dirty_guard(&mut self, cx: &mut Context<Self>) {
        self.save_host_dirty_guard_inner(true, cx);
    }

    pub(crate) fn flush_host_conflict_drafts(&mut self, cx: &mut Context<Self>) {
        self.save_host_dirty_guard_inner(false, cx);
    }

    fn save_host_dirty_guard_inner(&mut self, save_editors: bool, cx: &mut Context<Self>) {
        let Some(modal) = self.editor_dirty_guard_modal() else {
            return;
        };
        if modal.saving.is_some() {
            return;
        }
        let session = match modal.intent {
            EditorPendingIntent::CloseRepoTab(session) => Some(session),
            EditorPendingIntent::Quit
            | EditorPendingIntent::CloseWindow(_)
            | EditorPendingIntent::Relaunch => None,
            _ => return,
        };
        let conflict = self
            .ui
            .iter()
            .filter(|(id, _)| session.is_none_or(|session| **id == session))
            .filter_map(|(_, ui)| ui.conflict.as_ref())
            .find(|pane| pane.read(cx).has_unflushed_drafts())
            .cloned();
        if let Some(conflict) = conflict {
            let id = conflict.entity_id();
            static NEXT_SAVE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            let token = NEXT_SAVE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if let Some(modal) = self.editor_dirty_guard_modal_mut() {
                modal.saving = Some(id);
                modal.draft_save_token = Some(token);
                modal.conflict_discard_armed = false;
            }
            let task = conflict.update(cx, |view, cx| view.flush_drafts(cx));
            cx.spawn(async move |this, cx| {
                let saved = task.await;
                let _ = this.update(cx, |app, cx| {
                    if app
                        .editor_dirty_guard_modal()
                        .is_none_or(|modal| modal.draft_save_token != Some(token))
                    {
                        return;
                    }
                    app.stop_editor_guard_save();
                    if saved {
                        app.save_host_dirty_guard_inner(save_editors, cx);
                    }
                    cx.notify();
                });
            })
            .detach();
            cx.notify();
            return;
        }
        // Flushing recovery is automatic; writing ordinary editor files still
        // requires the user's explicit Save All action.
        if !save_editors
            && match session {
                Some(session) => self.editor_dirty_for(session, cx),
                None => self.host_dirty_buffer_count(cx) > 0,
            }
        {
            cx.notify();
            return;
        }
        if let Some(session) = self.editor_dirty_guard_modal().and_then(|modal| {
            if modal.saving.is_none() {
                if let EditorPendingIntent::CloseRepoTab(session) = modal.intent {
                    return Some(session);
                }
            }
            None
        }) {
            if let Some(modal) = self.editor_dirty_guard_modal_mut() {
                modal.conflict_discard_armed = false;
            }
            if !self.failed_conflict_draft_for(session, cx) && !self.editor_dirty_for(session, cx) {
                self.clear_editor_dirty_guard_modal();
                self.close_tab_by_session(session, cx);
            }
            cx.notify();
            return;
        }
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
