//! In-memory conflict choices and navigation.
use super::*;

impl ConflictView {
    pub(crate) fn menu_command_state(&self, id: &str) -> crate::ui::commands::CommandState {
        use crate::ui::commands::CommandState::{Disabled, Enabled};
        let Some(mode) = self.mode.as_ref() else {
            return Disabled(Msg::ConflictReviewNeeded.t());
        };
        if self.writer_busy {
            return Disabled(Msg::OpInProgress.t());
        }
        match id {
            "repo.conflictAbort" => Enabled,
            "repo.conflictSkip" if mode.session.op.is_sequencer() => Enabled,
            "repo.conflictContinue" if self.saved_review_needed => {
                Disabled(Msg::ConflictReviewNeeded.t())
            }
            "repo.conflictContinue" if mode.can_continue() => Enabled,
            _ => Disabled(
                mode.continue_blocker()
                    .unwrap_or(Msg::ConflictReviewNeeded)
                    .t(),
            ),
        }
    }

    /// Apply a per-file side choice to the in-memory resolution buffer, then
    /// recompute that file's status.  The repository is untouched (in-memory
    /// first); the buffer is autosaved so the partial resolution survives.
    ///
    /// On success this refreshes the file's status and autosaves; logging /
    /// toasting is the caller's responsibility (the `KagiApp` wrapper).
    ///
    /// Returns `None` when there is no active conflict mode — a no-op the caller
    /// must NOT log (mirrors the original early-return-without-logging path);
    /// `Some(Ok(()))` on apply, `Some(Err(_))` on buffer failure.
    pub fn apply_choice(
        &mut self,
        path: &std::path::Path,
        choice: kagi_git::ResolutionChoice,
    ) -> Option<Result<(), kagi_git::GitError>> {
        if self.writer_busy {
            return None;
        }
        let c = self.mode.as_mut()?;
        if let Err(e) = c.buffer.apply_choice(path, choice) {
            return Some(Err(e));
        }
        // Refresh status for this file from the buffer.
        c.refresh_file_status(path);
        // Autosave (ADR-0057): never lose a partial resolution.
        self.autosave_drafts();
        Some(Ok(()))
    }

    pub fn set_file_side(
        &mut self,
        path: &std::path::Path,
        side: kagi_git::resolution::SelectionSide,
        taken: bool,
    ) {
        if self.writer_busy
            || self
                .mode
                .as_ref()
                .is_some_and(|mode| flow::manual_override(mode, path))
        {
            return;
        }
        let Some(c) = self.mode.as_mut() else {
            return;
        };
        if c.buffer.set_file_side_selection(path, side, taken) {
            self.after_selection_change(path, None);
        }
    }

    pub fn set_hunk_side(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        side: kagi_git::resolution::SelectionSide,
        taken: bool,
    ) {
        if self.writer_busy
            || self
                .mode
                .as_ref()
                .is_some_and(|mode| flow::manual_override(mode, path))
        {
            return;
        }
        let Some(c) = self.mode.as_mut() else {
            return;
        };
        if c.buffer
            .set_hunk_side_selection(path, hunk_index, side, taken)
        {
            self.after_selection_change(path, Some(hunk_index));
        }
    }

    pub fn set_hunk_line(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        side: kagi_git::resolution::SelectionSide,
        line_index: usize,
        taken: bool,
    ) {
        if self.writer_busy
            || self
                .mode
                .as_ref()
                .is_some_and(|mode| flow::manual_override(mode, path))
        {
            return;
        }
        let Some(c) = self.mode.as_mut() else {
            return;
        };
        if c.buffer
            .set_hunk_line_selection(path, hunk_index, side, line_index, taken)
        {
            self.after_selection_change(path, Some(hunk_index));
        }
    }

    pub fn set_hunk_order(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        order: kagi_git::resolution::LineOrder,
    ) {
        if self.writer_busy
            || self
                .mode
                .as_ref()
                .is_some_and(|mode| flow::manual_override(mode, path))
        {
            return;
        }
        let Some(c) = self.mode.as_mut() else {
            return;
        };
        if c.buffer.set_hunk_line_order(path, hunk_index, order) {
            self.after_selection_change(path, Some(hunk_index));
        }
    }

    pub(super) fn after_selection_change(
        &mut self,
        path: &std::path::Path,
        selected_hunk: Option<usize>,
    ) {
        self.reset_all_target = None;
        if let Some(hunk) = selected_hunk {
            self.selected_hunk = hunk;
        }
        if let Some(i) = self.editor_inputs.as_mut() {
            i.content_sig = 0;
        }
        let Some(c) = self.mode.as_mut() else {
            return;
        };
        c.refresh_file_status(path);
        self.autosave_drafts();
    }
}
impl ConflictView {
    /// Open the dedicated Conflict Editor for the conflicting file at `path`.
    /// Builds (idempotently) the per-file `HunkModel` from the repo's zdiff3
    /// materialization, then sets `editing`. Read-only repo open via `repo_path`.
    /// (Moved from `KagiApp::conflict_open_editor`, sourcing `repo_path` from self.)
    pub fn conflict_open_editor(&mut self, path: &std::path::Path) {
        self.selected_saved = None;
        self.saved_preview = None;
        if self.editing.as_deref() != Some(path) {
            self.selected_hunk = 0;
            self.result_scroll_handle
                .scroll_to_item(0, gpui::ScrollStrategy::Top);
            self.result_editing = false;
            self.reset_all_target = None;
        }
        // Materialize the markers (needs the repo) and build the hunk model.
        if let Ok(repo) = kagi_git::Backend::open(&self.repo_path) {
            if let Some(c) = self.mode.as_mut() {
                if let Some(markers) = repo.materialized_markers(&c.buffer, path) {
                    c.buffer
                        .ensure_hunks(path, &markers.text, markers.marker_width);
                    c.refresh_file_status(path);
                }
            }
        }
        // Selection and the editor always refer to the same pending path.
        if let Some(c) = self.mode.as_mut() {
            if let Some(idx) = c.session.files.iter().position(|f| f.path == path) {
                c.selected_file = Some(idx);
                c.editing_file = Some(idx);
            }
        }
        self.editing = Some(path.to_path_buf());
        self.page = ConflictPage::Hunk;
    }

    /// T-CONFLICT-UX-010/012: set the focused hunk (selected-hunk highlight).
    pub fn conflict_editor_select_hunk(&mut self, hunk_index: usize) {
        if self.selected_hunk != hunk_index {
            self.reset_all_target = None;
            if let Some(model) = self
                .editing
                .as_deref()
                .and_then(|path| self.mode.as_ref()?.buffer.hunk_model(path))
            {
                let mut row = 0;
                let mut index = 0;
                for region in &model.regions {
                    match region {
                        kagi_git::resolution::Region::Passthrough(lines) => row += lines.len(),
                        kagi_git::resolution::Region::Hunk(hunk) => {
                            if index == hunk_index {
                                super::super::conflict_editor_keys::center_hunk(
                                    &self.ab_scroll_handle,
                                    row,
                                );
                                break;
                            }
                            row += 1 + hunk.current.len().max(hunk.incoming.len()).max(1);
                            index += 1;
                        }
                    }
                }
            }
            self.result_scroll_handle
                .scroll_to_item(0, gpui::ScrollStrategy::Top);
        }
        self.selected_hunk = hunk_index;
    }

    pub fn conflict_editor_set_file_side(
        &mut self,
        path: &std::path::Path,
        side: kagi_git::resolution::SelectionSide,
        taken: bool,
    ) {
        self.set_file_side(path, side, taken);
    }

    pub fn conflict_editor_set_hunk_side(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        side: kagi_git::resolution::SelectionSide,
        taken: bool,
    ) {
        self.set_hunk_side(path, hunk_index, side, taken);
    }

    pub fn conflict_editor_set_hunk_line(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        side: kagi_git::resolution::SelectionSide,
        line_index: usize,
        taken: bool,
    ) {
        self.set_hunk_line(path, hunk_index, side, line_index, taken);
    }

    pub fn conflict_editor_set_hunk_order(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        order: kagi_git::resolution::LineOrder,
    ) {
        self.set_hunk_order(path, hunk_index, order);
    }

    pub fn conflict_editor_reset_hunk(&mut self, path: &std::path::Path, hunk: usize) {
        if self.writer_busy
            || self.editing.as_deref() != Some(path)
            || self
                .mode
                .as_ref()
                .is_some_and(|mode| flow::manual_override(mode, path))
        {
            return;
        }
        if let Some(mode) = self.mode.as_mut() {
            mode.buffer.reset_hunk(path, hunk);
        }
        self.after_selection_change(path, Some(hunk));
    }

    /// T-CONFLICT-POLISH-042: "Reset all" two-stage confirm. First click arms,
    /// second performs the reset. (Moved from `KagiApp`.)
    pub fn conflict_editor_reset_all_request(&mut self, path: &std::path::Path) {
        if self.writer_busy || self.editing.as_deref() != Some(path) {
            return;
        }
        if self.reset_all_target.as_deref() == Some(path) {
            self.reset_all_target = None;
            self.conflict_editor_reset_all(path);
        } else {
            self.reset_all_target = Some(path.to_path_buf());
        }
    }

    /// Explicitly select Preview / Edit; reactivation preserves manual input.
    pub fn conflict_editor_set_result_mode(&mut self, editing: bool) {
        if self.writer_busy {
            return;
        }
        self.page = ConflictPage::Review;
        if self.result_editing == editing {
            return;
        }
        self.reset_all_target = None;
        self.result_editing = editing;
        // Force the inputs to re-sync (mode is part of the content signature).
        if let Some(i) = self.editor_inputs.as_mut() {
            i.content_sig = 0;
        }
    }

    /// Reset every hunk of `path` to unresolved (toolbar "Reset all").
    pub fn conflict_editor_reset_all(&mut self, path: &std::path::Path) {
        if self.writer_busy {
            return;
        }
        // Force the editor inputs to re-sync after the reset.
        if let Some(i) = self.editor_inputs.as_mut() {
            i.content_sig = 0;
        }
        let Some(c) = self.mode.as_mut() else {
            return;
        };
        let n = c.buffer.hunk_count(path);
        for i in 0..n {
            c.buffer.reset_hunk(path, i);
        }
        // Reset leaves marker residue → status becomes NeedsReview (still has a
        // result draft, but unresolved markers remain).
        c.refresh_file_status(path);
        self.autosave_drafts();
    }

    /// Select a file and open its matching content editor (or binary preview).
    /// All row and navigation activations use this path.
    pub fn conflict_select_file(&mut self, idx: usize) {
        let Some(mode) = self.mode.as_mut() else {
            return;
        };
        let Some(file) = mode.session.files.get(idx) else {
            return;
        };
        if mode.selected_file != Some(idx) {
            self.raw_preview.retire();
        }
        let open_path = matches!(file.kind, ConflictKind::Content | ConflictKind::AddAdd)
            .then(|| file.path.clone());
        self.reset_all_target = None;
        self.selected_saved = None;
        self.saved_preview = None;
        mode.selected_file = Some(idx);
        mode.editing_file = open_path.as_ref().map(|_| idx);
        if let Some(p) = open_path {
            self.conflict_open_editor(&p);
        } else {
            self.editing = None;
            self.editor_inputs = None;
            self.page = ConflictPage::Hunk;
        }
    }

    /// Move the selection to the previous / next unresolved file, wrapping.
    pub fn conflict_nav_unresolved(&mut self, dir: i32) {
        let Some(c) = self.mode.as_ref() else {
            return;
        };
        let n = c.session.files.len();
        if n == 0 {
            return;
        }
        let start = c.selected_file.unwrap_or(0);
        let next = (1..=n)
            .map(|step| {
                if dir >= 0 {
                    (start + step) % n
                } else {
                    (start + n - (step % n)) % n
                }
            })
            .find(|&i| !flow::draft_ready(c, &c.session.files[i].path))
            .unwrap_or_else(|| {
                if dir >= 0 {
                    (start + 1) % n
                } else {
                    (start + n - 1) % n
                }
            });
        self.conflict_select_file(next);
    }

    /// Apply a per-file side choice to the in-memory resolution buffer, emit the
    /// `[kagi]` contract line, and (on a buffer error) marshal an error toast to
    /// the parent. The buffer work itself touches no parent state, so it stays
    /// synchronous; only the rare error toast defers.
    pub fn conflict_apply_choice(
        &mut self,
        path: &std::path::Path,
        choice: kagi_git::ResolutionChoice,
        cx: &mut Context<Self>,
    ) {
        match self.apply_choice(path, choice) {
            // No active conflict mode — no-op, no `[kagi]` line (matches original).
            None => {}
            Some(Ok(())) => {
                klog!(
                    "conflict-mode: choice {} for {}",
                    match choice {
                        kagi_git::ResolutionChoice::Current => "current",
                        kagi_git::ResolutionChoice::Incoming => "incoming",
                        kagi_git::ResolutionChoice::BothCurrentFirst => "both(current-first)",
                        kagi_git::ResolutionChoice::BothIncomingFirst => "both(incoming-first)",
                    },
                    path.display()
                );
            }
            Some(Err(e)) => {
                klog!("conflict-mode: choice failed for {}: {}", path.display(), e);
                self.marshal_error_toast(format!("{}", e), cx);
            }
        }
    }

    /// Entry point for "Open external tool" toolbar button (launch is W33). Emits
    /// the contract `klog!` line and marshals the info toast to the parent.
    pub fn conflict_editor_open_external(
        &mut self,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        klog!(
            "conflict-editor: external tool requested for {} (launch is W33)",
            path.display()
        );
        let msg = format!(
            "External merge tool launch is not wired yet ({}).",
            path.display()
        );
        self.marshal_info_toast(msg, cx);
    }

    /// Two-stage Skip arming — the same shape as `abort_request_arm`. Returns
    /// `true` when already armed (the caller should EXECUTE the skip, which
    /// reloads, so it must defer to the parent); `false` when this click only
    /// ARMED it.
    pub fn skip_request_arm(&mut self) -> bool {
        let armed = self
            .mode
            .as_ref()
            .is_some_and(|c| c.skip_approval.is_some());
        if !armed {
            if let Some(c) = self.mode.as_mut() {
                c.skip_approval = Some(c.revision.clone());
            }
            klog!("conflict-mode: skip armed (second confirm required)");
            false
        } else {
            true
        }
    }

    /// Marshal an error toast to the parent (deferred — `push_toast` lives on
    /// `KagiApp`; this may run from a leased listener so it must not touch the
    /// parent synchronously).
    pub(in crate::ui) fn marshal_error_toast(&self, msg: String, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        let owner = self.owner.clone();
        cx.spawn(async move |_view, acx| {
            let _ = weak_app.update(acx, |app, cx| {
                app.push_conflict_owner_toast(&owner, super::ToastKind::Error, msg, cx);
            });
        })
        .detach();
    }

    /// Marshal an info toast to the parent (deferred — see `marshal_error_toast`).
    fn marshal_info_toast(&self, msg: String, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        let owner = self.owner.clone();
        cx.spawn(async move |_view, acx| {
            let _ = weak_app.update(acx, |app, cx| {
                app.push_conflict_owner_toast(&owner, super::ToastKind::Info, msg, cx);
            });
        })
        .detach();
    }
}
