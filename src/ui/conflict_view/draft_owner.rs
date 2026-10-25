//! A retained Result editor owns one accepted conflict revision, not a path.

use super::{ConflictMode, ConflictView};
use crate::ui::ConflictEditorInputs;
use gpui::{AppContext, Context, Focusable, Window};
use gpui_component::input::InputState;
use kagi_domain::conflict_family::ConflictRevision;
use kagi_git::conflicts::ConflictStatus;
use kagi_git::resolution::ResolutionBuffer;
use std::path::{Path, PathBuf};

impl ConflictView {
    /// Capture the live owned input before its buffer leaves the pane.
    pub(crate) fn capture_result_for_handoff(&mut self, cx: &Context<Self>) {
        if self.result_editing {
            if let (Some(mode), Some(path)) = (self.mode.as_mut(), self.editing.as_deref()) {
                if let Err(error) = pull_owned_editor(self.editor_inputs.as_ref(), mode, path, cx) {
                    self.note_draft_result(Err(error));
                }
            }
        }
    }

    pub(super) fn capture_result_for_navigation(&mut self, cx: &Context<Self>) -> bool {
        let result = if self.result_editing {
            match (self.mode.as_mut(), self.editing.as_deref()) {
                (Some(mode), Some(path)) => {
                    pull_owned_editor(self.editor_inputs.as_ref(), mode, path, cx)
                }
                _ => Ok(None),
            }
        } else {
            Ok(None)
        };
        match result {
            Ok(Some(true)) => {
                self.autosave_drafts();
                true
            }
            Err(error) => {
                self.note_draft_result(Err(error));
                false
            }
            _ => true,
        }
    }
    /// Adopt a changed observation without granting its predecessor's text
    /// resolution authority. Unchanged observations never enter this path.
    pub(crate) fn adopt_detected(
        &mut self,
        mut mode: ConflictMode,
        editing_path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.retire_raw_preview();
        let mut previous = self.mode.take();
        if self.result_editing {
            if let (Some(previous), Some(path)) = (previous.as_mut(), self.editing.as_deref()) {
                if let Err(error) =
                    pull_owned_editor(self.editor_inputs.as_ref(), previous, path, cx)
                {
                    self.note_draft_result(Err(error));
                }
            }
        }
        let selected = self
            .selected_saved
            .clone()
            .or_else(|| {
                previous.as_ref().and_then(|old| {
                    old.selected_file
                        .and_then(|i| old.session.files.get(i).map(|f| f.path.clone()))
                })
            })
            .or_else(|| self.editing.clone())
            .or(editing_path);
        let saved = selected
            .as_ref()
            .and_then(|path| mode.session.resolved.iter().position(|f| &f.path == path));
        let pending = selected
            .as_ref()
            .and_then(|path| mode.session.files.iter().position(|f| &f.path == path))
            .or_else(|| (saved.is_none() && !mode.session.files.is_empty()).then_some(0));
        let saved =
            saved.or_else(|| (pending.is_none() && !mode.session.resolved.is_empty()).then_some(0));
        mode.selected_file = pending;
        mode.editing_file = pending.filter(|&i| {
            matches!(
                mode.session.files[i].kind,
                kagi_git::ConflictKind::Content | kagi_git::ConflictKind::AddAdd
            )
        });
        let next_editing = mode
            .editing_file
            .map(|i| mode.session.files[i].path.clone());
        let same_context = next_editing.as_deref().is_some_and(|path| {
            self.editing.as_deref() == Some(path)
                && previous
                    .as_ref()
                    .is_some_and(|old| same_file_context(old, &mode, path))
        });
        let retain_input = same_context
            && self.editor_inputs.as_ref().is_some_and(|inputs| {
                next_editing.as_ref() == Some(&inputs.path)
                    && previous
                        .as_ref()
                        .is_some_and(|old| old.revision == inputs.revision)
            });
        self.editing_before_text.retain(|path, _| {
            previous
                .as_ref()
                .is_some_and(|old| same_file_context(old, &mode, path))
        });
        let first_detection = previous.is_none();
        if let Some(previous) = previous {
            mode.buffer.retain_drafts(previous.buffer);
            mode.buffer.revalidate_hunk_states();
            mode.refresh_file_statuses();
            // This writes only the existing recovery document, never the repo.
            // It also retains an old live edit if its earlier autosave failed.
            self.queue_draft_snapshot(&mode.buffer);
        }
        if retain_input {
            if let Some(inputs) = self.editor_inputs.as_mut() {
                inputs.revision = mode.revision.clone();
            }
        } else {
            self.editor_inputs = None;
        }
        if !same_context {
            self.page = super::ConflictPage::Hunk;
            self.selected_hunk = 0;
            self.reset_all_target = None;
            self.file_menu = None;
            *self.resolver_keyboard.borrow_mut() = Default::default();
            self.result_hl.borrow_mut().take();
            self.hl_cache.borrow_mut().take();
        }
        self.editing = next_editing;
        self.selected_saved = saved.map(|i| mode.session.resolved[i].path.clone());
        self.saved_review_needed = false;
        self.clear_saved_reads();
        self.mode = Some(mode);
        self.rebuild_tree();
        if first_detection {
            self.initialize_tree_expansion();
        }
        if let Some(saved) = saved {
            self.conflict_open_saved(saved, cx);
        } else if !same_context {
            if let Some(pending) = pending {
                self.conflict_select_file(pending);
            }
        }
        cx.notify();
    }

    /// Pull only from the editor bound to this revision; otherwise initialize
    /// a fresh InputState so old text, selection and undo cannot cross owners.
    pub fn sync_editor_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_owned_editor_inputs(window, cx);
        self.watch_draft_save(cx);
        self.present_draft_failure(cx).detach();
    }

    fn sync_owned_editor_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(inputs) = &self.editor_inputs {
            inputs.result.update(cx, |state, cx| {
                state.set_search_labels(kagi_ui_core::find::labels(), cx);
            });
        }
        if !self.writer_busy
            && self
                .editor_inputs
                .as_ref()
                .is_some_and(|inputs| inputs.result.read(cx).focus_handle(cx).is_focused(window))
        {
            self.result_editing = true;
        }
        let Some(path) = self.editing.as_deref() else {
            self.editor_inputs = None;
            return;
        };
        let Some(mode) = self.mode.as_mut() else {
            self.editor_inputs = None;
            return;
        };
        if self.result_editing {
            match pull_owned_editor(self.editor_inputs.as_ref(), mode, path, cx) {
                Ok(Some(changed)) => {
                    if changed {
                        self.autosave_drafts();
                    }
                    return;
                }
                Err(error) => {
                    self.note_draft_result(Err(error));
                    return;
                }
                Ok(None) => {}
            }
        }
        let Some(model) = mode.buffer.hunk_model(path) else {
            self.editor_inputs = None;
            return;
        };
        let result_text = mode
            .buffer
            .resolved_text(path)
            .unwrap_or_else(|| model.assembled_text());
        let sig = crate::ui::conflict_content_sig(path, &result_text, self.result_editing);
        if self.editor_inputs.as_ref().is_some_and(|inputs| {
            inputs.path == path && inputs.revision == mode.revision && inputs.content_sig == sig
        }) {
            return;
        }
        if self
            .editor_inputs
            .as_ref()
            .is_none_or(|inputs| inputs.path != path || inputs.revision != mode.revision)
        {
            let inputs = new_editor_inputs(path, &mode.revision, window, cx);
            self.result_subscription = Some(cx.subscribe(&inputs.result, |view, _, event, cx| {
                use gpui_component::input::InputEvent;
                if !view.writer_busy && matches!(event, InputEvent::Focus | InputEvent::Change) {
                    view.result_editing = true;
                    view.reset_all_target = None;
                    if matches!(event, InputEvent::Change) {
                        view.capture_result_for_navigation(cx);
                    }
                    cx.notify();
                }
            }));
            self.editor_inputs = Some(inputs);
        }
        if let Some(inputs) = self.editor_inputs.as_mut() {
            inputs
                .result
                .update(cx, |state, cx| state.set_value(result_text, window, cx));
            inputs.content_sig = sig;
        }
    }

    pub(super) fn autosave_drafts(&mut self) {
        let result = self.mode.as_ref().map(|mode| mode.buffer.queue_autosave());
        if let Some(result) = result {
            self.accept_draft_queue(result, true);
        }
    }

    fn queue_draft_snapshot(&mut self, buffer: &ResolutionBuffer) {
        self.accept_draft_queue(buffer.queue_autosave(), true);
    }

    fn accept_draft_queue(
        &mut self,
        result: Result<kagi_git::resolution::DraftSave, kagi_git::GitError>,
        decisions_changed: bool,
    ) {
        if decisions_changed {
            self.intent_token =
                super::NEXT_INTENT_TOKEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        match result {
            Ok(ticket) => {
                self.draft_save = Some(ticket);
            }
            Err(error) => self.note_draft_result(Err(error.to_string())),
        }
    }

    pub(crate) fn has_unflushed_drafts(&self) -> bool {
        self.draft_persist_failed.is_some()
            || self
                .mode
                .as_ref()
                .is_some_and(|mode| mode.buffer.has_unflushed_drafts())
    }

    #[cfg(feature = "gui-e2e")]
    pub fn draft_autosave_settled_for_e2e(&self) -> bool {
        !self.draft_watch_running
            && self.draft_flushes == 0
            && self.draft_failure_pending.is_none()
            && self
                .draft_save
                .as_ref()
                .is_none_or(|ticket| ticket.generation() == self.draft_watched_generation)
    }

    fn watch_draft_save(&mut self, cx: &mut Context<Self>) {
        if self.draft_watch_running {
            return;
        }
        let Some(ticket) = self
            .draft_save
            .clone()
            .filter(|ticket| ticket.generation() != self.draft_watched_generation)
        else {
            return;
        };
        self.draft_watch_running = true;
        self.draft_watched_generation = ticket.generation();
        let wait_ticket = ticket.clone();
        let task = cx.background_spawn(async move { wait_ticket.wait() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |view, cx| {
                view.draft_watch_running = false;
                if ticket.is_current() {
                    view.note_draft_result(result.map(|_| ()).map_err(|error| error.to_string()));
                    view.present_draft_failure(cx).detach();
                }
                view.watch_draft_save(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn note_draft_result(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => self.draft_persist_failed = None,
            Err(error) => {
                if self.draft_persist_failed.as_ref() != Some(&error) {
                    self.draft_failure_pending = Some(error.clone());
                }
                self.draft_persist_failed = Some(error);
            }
        }
    }

    fn present_draft_failure(&mut self, cx: &mut Context<Self>) -> gpui::Task<()> {
        let Some(error) = self.draft_failure_pending.take() else {
            return gpui::Task::ready(());
        };
        let parent = self.app.clone();
        let owner = self.owner.clone();
        let repo = self.repo_path.clone();
        cx.spawn(async move |_, cx| {
            let _ = parent.update(cx, |app, cx| {
                let current = app.conflict_action_owner_on_screen(&owner);
                // Unlike ordinary short operation results, a failed autosave
                // must announce even while its Graph workspace is foreground.
                // This shared path records once and keeps the preview bounded.
                app.record_op_result(
                    kagi_git::oplog::OpLogEntry::new(
                        "conflict-draft-autosave",
                        repo.display().to_string(),
                        kagi_git::StateSummary {
                            head: "conflict".into(),
                            dirty: "unsaved decisions".into(),
                        },
                        kagi_git::oplog::OpOutcome::Failed { error },
                    ),
                    cx,
                    true,
                    None,
                    None,
                    true,
                    current,
                    None,
                    false,
                    None,
                    None,
                );
            });
        })
    }

    fn prepare_draft_flush(&mut self, cx: &Context<Self>) -> bool {
        if !self.capture_result_for_navigation(cx) {
            return false;
        }
        if self.draft_persist_failed.is_some()
            || self
                .draft_save
                .as_ref()
                .is_none_or(|ticket| !ticket.is_current())
                && self.has_unflushed_drafts()
        {
            // A retry changes writer generation, not the decisions frozen by
            // this action. A changed Result was already invalidated by capture.
            if let Some(mode) = self.mode.as_ref() {
                self.accept_draft_queue(mode.buffer.queue_autosave(), false);
            }
        }
        true
    }

    /// The non-cancellable OS exit path has only GPUI's shared 200 ms budget.
    /// Capture before windows disappear, then urgently flush owned tickets off
    /// the UI thread; do not depend on an entity callback surviving shutdown.
    pub(super) fn flush_drafts_on_quit(&mut self, cx: &mut Context<Self>) -> gpui::Task<()> {
        if !self.prepare_draft_flush(cx) {
            eprintln!("Conflict draft shutdown: could not capture the latest Result");
        }
        let ticket = self.draft_save.clone();
        cx.background_spawn(async move {
            if let Some(ticket) = ticket {
                if let Err(error) = ticket.flush() {
                    eprintln!("Conflict draft shutdown flush: {error}");
                }
            }
        })
    }

    /// Capture first; the blocking barrier runs before repository admission and
    /// never under its lock. Explicit actions retry a failed current body.
    pub(crate) fn flush_drafts(&mut self, cx: &mut Context<Self>) -> gpui::Task<bool> {
        if !self.prepare_draft_flush(cx) {
            return gpui::Task::ready(false);
        }
        let Some(mode) = self.mode.as_ref() else {
            return gpui::Task::ready(self.draft_persist_failed.is_none());
        };
        let revision = mode.revision.clone();
        let ticket = self.draft_save.clone();
        self.draft_flushes += 1;
        let task = cx.background_spawn(async move {
            let result = ticket.as_ref().map_or(Ok(()), |ticket| ticket.flush());
            (result, ticket)
        });
        cx.spawn(async move |this, cx| {
            let (result, ticket) = task.await;
            let notification = this
                .update(cx, |view, cx| {
                    let current = ticket.as_ref().is_none_or(|ticket| ticket.is_current())
                        && view.mode.as_ref().is_some_and(|mode| {
                            mode.revision == revision && !mode.buffer.has_unflushed_drafts()
                        });
                    if result.is_err() || current {
                        view.note_draft_result(result.map_err(|error| error.to_string()));
                    }
                    if let Some(error) = view.draft_persist_failed.as_ref() {
                        view.draft_failure_pending = Some(error.clone());
                    }
                    view.present_draft_failure(cx)
                })
                .ok();
            // Finish the draft diagnostic before the caller presents its more
            // specific operation refusal; detached callbacks must not erase it.
            if let Some(notification) = notification {
                notification.await;
            }
            this.update(cx, |view, cx| {
                view.draft_flushes -= 1;
                view.capture_result_for_handoff(cx);
                let current = ticket.as_ref().is_none_or(|ticket| ticket.is_current())
                    && view.mode.as_ref().is_some_and(|mode| {
                        mode.revision == revision && !mode.buffer.has_unflushed_drafts()
                    });
                cx.notify();
                current && view.draft_persist_failed.is_none()
            })
            .unwrap_or(false)
        })
    }
}

/// None means a foreign/stale editor; Some reports whether owned text changed.
/// Persistence belongs to the caller, allowing adoption to save the combined
/// active/recovery document once, after the old buffer has been moved.
fn pull_owned_editor(
    inputs: Option<&ConflictEditorInputs>,
    mode: &mut ConflictMode,
    path: &Path,
    cx: &Context<ConflictView>,
) -> Result<Option<bool>, String> {
    let Some(inputs) = inputs.filter(|inputs| {
        inputs.path == path && inputs.revision == mode.revision && inputs.content_sig != 0
    }) else {
        return Ok(None);
    };
    let Some(model) = mode.buffer.hunk_model(path) else {
        return Ok(None);
    };
    let result_text = mode
        .buffer
        .resolved_text(path)
        .unwrap_or_else(|| model.assembled_text());
    let edited = inputs.result.read(cx).value();
    let changed = !edited
        .split_terminator('\n')
        .eq(result_text.split_terminator('\n'));
    if !changed {
        return Ok(Some(false));
    }
    if let Err(error) = mode.buffer.set_manual_text(path, &edited) {
        klog!("conflict-mode: resolution draft update failed: {error}");
        return Err(error.to_string());
    }
    mode.refresh_file_status(path);
    Ok(Some(true))
}

fn new_editor_inputs(
    path: &Path,
    revision: &ConflictRevision,
    window: &mut Window,
    cx: &mut Context<ConflictView>,
) -> ConflictEditorInputs {
    let lang = crate::ui::diff_view::lang_for_path(path).unwrap_or("text");
    let result = cx.new(|cx| {
        InputState::new(window, cx)
            .code_editor(lang)
            .search_labels(kagi_ui_core::find::labels())
            .line_number(true)
    });
    ConflictEditorInputs {
        path: path.to_path_buf(),
        revision: revision.clone(),
        result,
        content_sig: 0,
    }
}

fn same_file_context(previous: &ConflictMode, next: &ConflictMode, path: &Path) -> bool {
    previous
        .buffer
        .file_identity(path)
        .is_some_and(|identity| next.buffer.file_identity(path) == Some(identity))
        && previous.buffer.worktree_fingerprint(path) == next.buffer.worktree_fingerprint(path)
}

impl ConflictMode {
    pub(super) fn refresh_file_status(&mut self, path: &Path) {
        if let Some(file) = self.session.files.iter_mut().find(|file| file.path == path) {
            file.status = resolution_status(&self.buffer, path);
        }
    }

    fn refresh_file_statuses(&mut self) {
        for file in &mut self.session.files {
            file.status = resolution_status(&self.buffer, &file.path);
        }
    }
}

fn resolution_status(buffer: &ResolutionBuffer, path: &Path) -> ConflictStatus {
    if !buffer.has_resolution(path) {
        ConflictStatus::Unresolved
    } else if buffer.cached_marker_residue(path) {
        ConflictStatus::NeedsReview
    } else {
        ConflictStatus::Resolved
    }
}
