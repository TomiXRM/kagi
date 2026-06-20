//! Conflict-resolution operations for `KagiApp` (issue #13 Phase 4, P1).
//!
//! Extracted verbatim from `ui/mod.rs`: the conflict editor and the
//! conflict-session operations (`conflict_*`, `confirm/cancel_conflict_continue`).
//! Behaviour is unchanged. Per Rust visibility a descendant module can access
//! the private fields/methods of `KagiApp`, so no visibility was widened.

#![allow(clippy::too_many_arguments)]

use crate::ui::*;

impl KagiApp {
    /// Open the dedicated Conflict Editor for the conflicting file at `path`.
    ///
    /// Builds (idempotently) the per-file [`HunkModel`] in the buffer from the
    /// repository's zdiff3 materialization, then sets `conflict_editing`.  If the
    /// file has no usable text merge (binary / single-sided) the editor still
    /// opens and shows guidance (the hunk model is absent).  The repository is
    /// opened read-only; nothing is written.
    pub fn conflict_open_editor(&mut self, path: &std::path::Path) {
        // Materialize the markers (needs the repo) and build the hunk model.
        if let Some(repo_path) = self.repo_path.clone() {
            if let Ok(repo) = kagi::git::Backend::open(&repo_path) {
                if let Some(c) = self.conflict.as_mut() {
                    if let Some(markers) = repo.materialized_markers(&c.buffer, path) {
                        c.buffer.ensure_hunks(path, &markers);
                    }
                }
            }
        }
        // Keep the Dashboard selection in sync so back/forth is coherent.
        if let Some(c) = self.conflict.as_mut() {
            if let Some(idx) = c.session.files.iter().position(|f| f.path == path) {
                c.selected_file = Some(idx);
                c.editing_file = Some(idx);
            }
        }
        self.conflict_editing = Some(path.to_path_buf());
    }

    /// T-CONFLICT-UX-010/012: set the focused hunk (selected-hunk highlight).
    pub fn conflict_editor_select_hunk(&mut self, hunk_index: usize) {
        self.conflict_selected_hunk = hunk_index;
    }

    fn conflict_editor_after_selection_change(
        &mut self,
        path: &std::path::Path,
        selected_hunk: Option<usize>,
    ) {
        self.conflict_reset_all_armed = false;
        if let Some(hunk) = selected_hunk {
            self.conflict_selected_hunk = hunk;
        }
        if let Some(i) = self.conflict_editor_inputs.as_mut() {
            i.content_sig = 0;
        }
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        let residue = c.buffer.files_with_marker_residue();
        if let Some(f) = c.session.files.iter_mut().find(|f| f.path == path) {
            f.status = if !c.buffer.has_resolution(path) {
                kagi::git::ConflictStatus::Unresolved
            } else if residue.contains(&f.path) {
                kagi::git::ConflictStatus::NeedsReview
            } else {
                kagi::git::ConflictStatus::Resolved
            };
        }
        let _ = c.buffer.autosave();
    }

    pub fn conflict_editor_set_file_side(
        &mut self,
        path: &std::path::Path,
        side: kagi::git::resolution::SelectionSide,
        taken: bool,
    ) {
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        if c.buffer.set_file_side_selection(path, side, taken) {
            self.conflict_editor_after_selection_change(path, None);
        }
    }

    pub fn conflict_editor_set_hunk_side(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        side: kagi::git::resolution::SelectionSide,
        taken: bool,
    ) {
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        if c.buffer
            .set_hunk_side_selection(path, hunk_index, side, taken)
        {
            self.conflict_editor_after_selection_change(path, Some(hunk_index));
        }
    }

    pub fn conflict_editor_set_hunk_line(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        side: kagi::git::resolution::SelectionSide,
        line_index: usize,
        taken: bool,
    ) {
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        if c.buffer
            .set_hunk_line_selection(path, hunk_index, side, line_index, taken)
        {
            self.conflict_editor_after_selection_change(path, Some(hunk_index));
        }
    }

    pub fn conflict_editor_set_hunk_order(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        order: kagi::git::resolution::LineOrder,
    ) {
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        if c.buffer.set_hunk_line_order(path, hunk_index, order) {
            self.conflict_editor_after_selection_change(path, Some(hunk_index));
        }
    }

    /// T-CONFLICT-POLISH-042: "Reset all" is destructive (drops every hunk
    /// choice for this file), so it is two-stage: the first click arms the
    /// confirmation, the second performs the reset.  The armed flag is cleared
    /// by any other editor interaction (handled where those run) and on the
    /// performed reset.
    pub fn conflict_editor_reset_all_request(&mut self, path: &std::path::Path) {
        if self.conflict_reset_all_armed {
            self.conflict_reset_all_armed = false;
            self.conflict_editor_reset_all(path);
        } else {
            self.conflict_reset_all_armed = true;
        }
    }

    /// T-CONFLICT-UX-015: toggle the Result pane between Preview (read-only) and
    /// Edit (editable) mode.  Leaving Edit mode does not discard the text — the
    /// edits were already pulled into the buffer via `set_manual_text` during
    /// the sync pass.
    pub fn conflict_editor_toggle_result_mode(&mut self) {
        self.conflict_result_editing = !self.conflict_result_editing;
        // Force the inputs to re-sync (mode is part of the content signature).
        if let Some(i) = self.conflict_editor_inputs.as_mut() {
            i.content_sig = 0;
        }
    }

    /// Reset every hunk of `path` to unresolved (toolbar "Reset all").
    pub fn conflict_editor_reset_all(&mut self, path: &std::path::Path) {
        // Force the editor inputs to re-sync after the reset.
        if let Some(i) = self.conflict_editor_inputs.as_mut() {
            i.content_sig = 0;
        }
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        let n = c.buffer.hunk_count(path);
        for i in 0..n {
            c.buffer.reset_hunk(path, i);
        }
        // Reset leaves marker residue → status becomes NeedsReview (still has a
        // result draft, but unresolved markers remain).
        let residue = c.buffer.files_with_marker_residue();
        if let Some(f) = c.session.files.iter_mut().find(|f| f.path == path) {
            f.status = if residue.contains(&f.path) {
                kagi::git::ConflictStatus::NeedsReview
            } else if c.buffer.has_resolution(path) {
                kagi::git::ConflictStatus::Resolved
            } else {
                kagi::git::ConflictStatus::Unresolved
            };
        }
        let _ = c.buffer.autosave();
    }

    /// Move the editor's view to the next (`dir > 0`) / previous (`dir < 0`)
    /// **unresolved** hunk by selecting an adjacent still-conflicted file when the
    /// current one is done.  MVP: hunks scroll within the file; this navigates the
    /// file selection so prev/next always lands on work to do.
    pub fn conflict_editor_nav_hunk(&mut self, dir: i32) {
        // For MVP, prev/next reuse the Dashboard unresolved-file navigation and
        // re-open the editor on the newly selected file.
        self.conflict_nav_unresolved(dir);
        if let Some(c) = self.conflict.as_ref() {
            if let Some(idx) = c.selected_file {
                if let Some(f) = c.session.files.get(idx) {
                    let p = f.path.clone();
                    self.conflict_open_editor(&p);
                }
            }
        }
    }

    /// Entry point for "Open external tool" (ADR-0060 / ADR-0064 toolbar).  The
    /// actual launch is W33's lane; here we only record the intent + toast so the
    /// button is wired and discoverable.
    pub fn conflict_editor_open_external(&mut self, path: &std::path::Path) {
        eprintln!(
            "[kagi] conflict-editor: external tool requested for {} (launch is W33)",
            path.display()
        );
        self.push_toast(
            ToastKind::Info,
            SharedString::from(format!(
                "External merge tool launch is not wired yet ({}).",
                path.display()
            )),
        );
    }

    /// Save resolution (ADR-0068 / T-CONFLICT-UX-013/014): write the resolved
    /// Result to the **working tree**, run the marker-residue check (markers
    /// remaining BLOCK the save), then **stage** the file so its index unmerged
    /// entries (stage 1/2/3) collapse to stage 0.  Moves the file into Resolved
    /// Files, re-evaluates the continue gate, autosaves the buffer, and records
    /// the resolution action to the operation log (T-035).  No commit is created.
    pub fn conflict_editor_save(&mut self, path: &std::path::Path) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(c) = self.conflict.as_ref() else {
            return;
        };

        // Before/after hashes of the file's resolved text for the oplog.
        let before_text = self
            .conflict_editing_before_text
            .get(path)
            .cloned()
            .unwrap_or_default();
        let after_text = c.buffer.resolved_text(path).unwrap_or_default();
        let before_hash = short_hash(&before_text);
        let after_hash = short_hash(&after_text);

        // Per-hunk action summary for the log.
        let actions = c
            .buffer
            .hunk_model(path)
            .map(|m| {
                m.hunks()
                    .iter()
                    .enumerate()
                    .map(|(i, h)| format!("{}:{}", i, hunk_choice_slug(&h.choice)))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let session_slug = c.session.op.slug().to_string();
        let op_name = format!("conflict-save:{}", session_slug);
        let before = StateSummary {
            head: format!("session={} file={}", session_slug, path.display()),
            dirty: format!("hunks=[{}] before={}", actions, before_hash),
        };

        // Open the repo and perform the real Save: WT write + marker block + stage
        // (index unmerged → stage 0).  Marker residue is a HARD block here.
        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", "session unavailable")),
                );
                return;
            }
        };
        let buffer = match self.conflict.as_ref() {
            Some(c) => c.buffer.clone(),
            None => return,
        };
        match repo.execute_conflict_save(&buffer, path) {
            Ok(_outcome) => {
                // Staged → mark the file Resolved and re-evaluate the gate.
                if let Some(c) = self.conflict.as_mut() {
                    let _ = c.buffer.autosave();
                    let residue = c.buffer.files_with_marker_residue();
                    if let Some(f) = c.session.files.iter_mut().find(|f| f.path == path) {
                        f.status = if residue.contains(&f.path) {
                            kagi::git::ConflictStatus::NeedsReview
                        } else {
                            kagi::git::ConflictStatus::Resolved
                        };
                    }
                }
                let after = StateSummary {
                    head: format!(
                        "staged (stage 0) before={} after={}",
                        before_hash, after_hash
                    ),
                    dirty: "clean".to_string(),
                };
                self.record_op(&op_name, before, OpOutcome::Success { after }, &repo_path);
                self.conflict_editing_before_text
                    .insert(path.to_path_buf(), after_text);
                // Re-detect so the staged file leaves the conflicted index set.
                self.conflict_detected_for = None;
                self.detect_conflict_mode();
                self.push_toast(
                    ToastKind::Success,
                    SharedString::from(Msg::EditorSavedResolved.t()),
                );
            }
            Err(_e) => {
                // Marker residue / write failure: hard block (ADR-0068).
                let err_msg = format!("{}", "session unavailable");
                self.record_op(
                    &op_name,
                    before,
                    OpOutcome::Refused {
                        blockers: vec![err_msg],
                    },
                    &repo_path,
                );
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(Msg::EditorMarkerWarning.t()),
                );
            }
        }
    }

    /// Select a conflicting file (open its detail + Result preview).
    pub fn conflict_select_file(&mut self, idx: usize) {
        let mut open_path: Option<PathBuf> = None;
        if let Some(c) = self.conflict.as_mut() {
            if let Some(f) = c.session.files.get(idx) {
                c.selected_file = Some(idx);
                // W32: activating a content conflict opens the dedicated
                // hunk-level Conflict Editor (binary / single-sided files have no
                // hunk model and stay on the Dashboard choose UI).
                if f.kind == kagi::git::ConflictKind::Content {
                    open_path = Some(f.path.clone());
                }
            }
        }
        if let Some(p) = open_path {
            self.conflict_open_editor(&p);
        }
    }

    /// Move the selection to the previous (`dir < 0`) or next (`dir > 0`)
    /// **unresolved** file, wrapping around (KDiff3-style nav).
    pub fn conflict_nav_unresolved(&mut self, dir: i32) {
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        let n = c.session.files.len();
        if n == 0 {
            return;
        }
        let start = c.selected_file.unwrap_or(0);
        // Scan up to n positions in the requested direction for an unresolved file.
        for step in 1..=n {
            let i = if dir >= 0 {
                (start + step) % n
            } else {
                (start + n - (step % n)) % n
            };
            if c.session.files[i].status == kagi::git::ConflictStatus::Unresolved {
                c.selected_file = Some(i);
                return;
            }
        }
        // None unresolved — just step to the neighbour so nav still feels alive.
        let i = if dir >= 0 {
            (start + 1) % n
        } else {
            (start + n - 1) % n
        };
        c.selected_file = Some(i);
    }

    /// Apply a per-file side choice to the in-memory resolution buffer, then
    /// recompute that file's status.  The repository is untouched (in-memory
    /// first); the buffer is autosaved so the partial resolution survives.
    pub fn conflict_apply_choice(
        &mut self,
        path: &std::path::Path,
        choice: kagi::git::ResolutionChoice,
    ) {
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        match c.buffer.apply_choice(path, choice) {
            Ok(()) => {
                // Refresh status for this file from the buffer.
                let residue = c.buffer.files_with_marker_residue();
                if let Some(f) = c.session.files.iter_mut().find(|f| f.path == path) {
                    f.status = if residue.contains(&f.path) {
                        kagi::git::ConflictStatus::NeedsReview
                    } else {
                        kagi::git::ConflictStatus::Resolved
                    };
                }
                // Autosave (ADR-0057): never lose a partial resolution.
                let _ = c.buffer.autosave();
                eprintln!(
                    "[kagi] conflict-mode: choice {} for {}",
                    match choice {
                        kagi::git::ResolutionChoice::Current => "current",
                        kagi::git::ResolutionChoice::Incoming => "incoming",
                        kagi::git::ResolutionChoice::BothCurrentFirst => "both(current-first)",
                        kagi::git::ResolutionChoice::BothIncomingFirst => "both(incoming-first)",
                    },
                    path.display()
                );
            }
            Err(e) => {
                eprintln!(
                    "[kagi] conflict-mode: choice failed for {}: {}",
                    path.display(),
                    e
                );
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("{}", "session unavailable")),
                );
            }
        }
    }

    /// Continue the in-progress operation (ADR-0068 routing — T-CONFLICT-FLOW-030/
    /// 032).  Gates through `plan_conflict_continue_route`, then:
    ///
    /// - **merge** → transition to the commit message panel pre-filled with the
    ///   merge message (`conflict_merge_commit_pending = true`).  **No commit is
    ///   created here** — the commit panel's commit button calls
    ///   `start_merge_commit`, which creates the 2-parent merge commit.
    /// - **rebase / cherry-pick / revert** → open the `<op> --continue`
    ///   confirmation modal (`conflict_continue_modal`); the sequencer runs only
    ///   when the user confirms (`confirm_conflict_continue`).
    pub fn conflict_continue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(mode) = self.conflict.clone() else {
            return;
        };

        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", "session unavailable")),
                );
                return;
            }
        };

        let op_name = format!("{}-continue", mode.session.op.slug());
        let route = match repo.plan_conflict_continue_route(
            &mode.session,
            &mode.buffer,
            &mode.current_branch,
        ) {
            Ok(r) => r,
            Err(_e) => {
                klog!("refused: {} blocked: {}", op_name, "session unavailable");
                // Surface the specific (localized) blocking reason (ADR-0067).
                if let Some(first) = repo.continue_blockers(&mode.session, &mode.buffer).first() {
                    self.push_toast(ToastKind::Error, conflict_view::blocker_msg(first).t());
                } else {
                    self.push_toast(
                        ToastKind::Error,
                        SharedString::from(format!("{}", "session unavailable")),
                    );
                }
                self.record_op(
                    &op_name,
                    StateSummary {
                        head: format!("op={}", mode.session.op.slug()),
                        dirty: "blocked".to_string(),
                    },
                    OpOutcome::Refused {
                        blockers: vec![format!("{}", "session unavailable")],
                    },
                    &repo_path,
                );
                cx.notify();
                return;
            }
        };

        match route {
            kagi::git::ContinueRoute::MergeCommitPanel { message } => {
                // Transition to the commit message panel pre-filled with the merge
                // message.  MERGE_HEAD stays present so the commit becomes a merge
                // commit.  No commit is created here (ADR-0068).
                //
                // Stage the resolutions into the index first: the per-file Save is
                // optional, so the index may still hold unmerged entries.  Without
                // this the commit panel shows nothing staged (Commit disabled) and
                // execute_merge_commit refuses the still-conflicted index.
                if let Err(_e) = repo.stage_conflict_resolution(&mode.session, &mode.buffer) {
                    klog!(
                        "refused: {} stage failed: {}",
                        op_name,
                        "session unavailable"
                    );
                    self.push_toast(
                        ToastKind::Error,
                        SharedString::from(format!(
                            "Could not stage resolution: {}",
                            "session unavailable"
                        )),
                    );
                    cx.notify();
                    return;
                }
                eprintln!(
                    "[kagi] {}: routing to commit message panel (merge)",
                    op_name
                );
                self.open_commit_panel(window, cx);
                self.commit_template_mode = false;
                if let Some(input) = self.commit_input.clone() {
                    input.update(cx, |state, cx| state.set_value(message.clone(), window, cx));
                }
                if let Some(panel) = self.commit_panel.as_mut() {
                    panel.commit_msg = message.clone();
                }
                self.conflict_merge_commit_pending = true;
            }
            kagi::git::ContinueRoute::SequencerPlan(plan) => {
                // Confirmation modal before advancing the sequencer.
                eprintln!(
                    "[kagi] {}: opening continue confirmation (sequencer)",
                    op_name
                );
                self.set_conflict_continue_modal(ConflictContinuePlanModal {
                    plan: std::sync::Arc::new(*plan),
                    error: None,
                });
            }
        }
        cx.notify();
    }

    /// Confirm the sequencer `<op> --continue` plan (T-CONFLICT-FLOW-032): run
    /// `execute_conflict_continue` (which stages the resolution and advances the
    /// sequencer), record the oplog, drop the autosaved buffer, and reload.
    pub fn confirm_conflict_continue(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(mode) = self.conflict.clone() else {
            return;
        };
        let Some(modal) = self.conflict_continue_modal().cloned() else {
            return;
        };
        let plan = modal.plan;

        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", "session unavailable")),
                );
                return;
            }
        };
        let op_name = format!("{}-continue", mode.session.op.slug());

        match repo.execute_conflict_continue(&mode.session, &mode.buffer) {
            Ok(_outcome) => {
                klog!("executed: {}", op_name);
                let _ = kagi::git::ResolutionBuffer::clear(&repo_path);
                let after = StateSummary {
                    head: plan.predicted.head.clone(),
                    dirty: "staged".to_string(),
                };
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                self.clear_conflict_continue_modal();
                self.reload();
            }
            Err(_e) => {
                let err_msg = format!("{}", "session unavailable");
                klog!("{} failed: {}", op_name, err_msg);
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                if let Some(modal) = self.conflict_continue_modal_mut() {
                    modal.error = Some(SharedString::from(err_msg));
                }
            }
        }
        cx.notify();
    }

    /// Cancel the sequencer continue confirmation modal.
    pub fn cancel_conflict_continue(&mut self) {
        self.clear_conflict_continue_modal();
    }

    /// Abort the in-progress operation through the existing plan pipeline:
    /// `plan_conflict_abort` → `execute_conflict_abort` → oplog → re-detect.
    /// Abort is always available (no blockers); the partial resolution buffer is
    /// preserved by the backend (ADR-0057).
    pub fn conflict_abort(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(mode) = self.conflict.clone() else {
            return;
        };

        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", "session unavailable")),
                );
                return;
            }
        };

        let plan = match repo.plan_conflict_abort(&mode.session) {
            Ok(p) => p,
            Err(_e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("abort plan error: {}", "session unavailable")),
                );
                return;
            }
        };
        let op_name = format!("{}-abort", mode.session.op.slug());

        match repo.execute_conflict_abort(&mode.session, &mode.buffer) {
            Ok(_outcome) => {
                klog!("executed: {}", op_name);
                let after = StateSummary {
                    head: plan.predicted.head.clone(),
                    dirty: "clean".to_string(),
                };
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                self.reload();
            }
            Err(_e) => {
                let err_msg = format!("{}", "session unavailable");
                klog!("{} failed: {}", op_name, err_msg);
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Failed { error: err_msg },
                    &repo_path,
                );
            }
        }
        cx.notify();
    }

    /// Two-stage Abort (ADR-0067): the first click arms the confirm, the second
    /// executes.  Surfaces the "saved resolution may be lost" warning in the UI
    /// (the dashboard shows the hint while armed).
    pub fn conflict_abort_request(&mut self, cx: &mut Context<Self>) {
        let armed = self
            .conflict
            .as_ref()
            .map(|c| c.abort_armed)
            .unwrap_or(false);
        if !armed {
            if let Some(c) = self.conflict.as_mut() {
                c.abort_armed = true;
            }
            klog!("conflict-mode: abort armed (second confirm required)");
            return;
        }
        // Armed → execute (conflict_abort re-detects and rebuilds the mode).
        self.conflict_abort(cx);
    }

    /// Skip the current sequencer step (rebase / cherry-pick / revert) through
    /// the plan pipeline (T-042, ADR-0067): `plan_conflict_skip` → execute →
    /// oplog → re-detect.  Merge has no skip (the button is hidden for merge;
    /// the backend `plan_conflict_skip` also errors for merge as a guard).
    pub fn conflict_skip(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(mode) = self.conflict.clone() else {
            return;
        };

        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", "session unavailable")),
                );
                return;
            }
        };

        let plan = match repo.plan_conflict_skip(&mode.session) {
            Ok(p) => p,
            Err(_e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("skip plan error: {}", "session unavailable")),
                );
                return;
            }
        };
        let op_name = format!("{}-skip", mode.session.op.slug());

        match repo.execute_conflict_skip(&mode.session, &mode.buffer) {
            Ok(_outcome) => {
                klog!("executed: {}", op_name);
                let after = StateSummary {
                    head: plan.predicted.head.clone(),
                    dirty: "current step dropped".to_string(),
                };
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                self.reload();
            }
            Err(_e) => {
                let err_msg = format!("{}", "session unavailable");
                klog!("{} failed: {}", op_name, err_msg);
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Failed { error: err_msg },
                    &repo_path,
                );
            }
        }
        cx.notify();
    }

    /// Open the configured external merge tool for the selected conflict file
    /// (ADR-0060 / T-050).  Reads `settings.json` `"mergetool"` and substitutes
    /// `$LOCAL` / `$BASE` / `$REMOTE` / `$MERGED`.  If unset, shows how to
    /// configure it (we do NOT invent a default tool).  No plan needed
    /// (read-only launch); a note is recorded to the oplog footer via the toast.
    pub fn conflict_open_external_tool(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        let Some(c) = self.conflict.as_ref() else {
            return;
        };
        let Some(idx) = c.selected_file else { return };
        let Some(file) = c.session.files.get(idx) else {
            return;
        };

        let template = match settings::read_setting("mergetool") {
            Some(t) if !t.trim().is_empty() => t,
            _ => {
                self.push_toast(ToastKind::Info, Msg::ConflictExternalToolUnset.t());
                return;
            }
        };

        let workdir = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let merged = workdir.join(&file.path);
        let merged_str = merged.to_string_lossy().into_owned();
        // $LOCAL/$BASE/$REMOTE are the current/base/incoming versions; in the
        // in-memory MVP we point every side at the conflicted working-tree file
        // (which contains the markers) so external tools that re-parse markers
        // (e.g. `code --wait`, `vimdiff $MERGED`) work.  Tools needing distinct
        // side files are a v0.2 enhancement (materialize the three sides first).
        let cmd = template
            .replace("$LOCAL", &merged_str)
            .replace("$BASE", &merged_str)
            .replace("$REMOTE", &merged_str)
            .replace("$MERGED", &merged_str);

        klog!("conflict-mode: launch external tool: {}", cmd);
        match std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(&workdir)
            .spawn()
        {
            Ok(_) => self.push_toast(
                ToastKind::Info,
                SharedString::from(format!("{}: {}", Msg::ConflictExternalTool.t(), merged_str)),
            ),
            Err(_e) => self.push_toast(
                ToastKind::Error,
                SharedString::from(format!("external tool failed: {}", "session unavailable")),
            ),
        }
    }

    /// Open the integrated terminal at the repository root (ADR-0060 / T-051).
    pub fn conflict_open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.bottom_panel_open = true;
        self.bottom_tab = BottomTab::Terminal;
        self.ensure_terminal(window, cx);
    }

    /// Copy the selected conflict file's absolute path to the clipboard
    /// (ADR-0060 / T-052).
    pub fn conflict_copy_path(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.conflict.as_ref() else {
            return;
        };
        let Some(idx) = c.selected_file else { return };
        let Some(file) = c.session.files.get(idx) else {
            return;
        };
        let abs = match self.repo_path.clone() {
            Some(p) => p.join(&file.path).to_string_lossy().into_owned(),
            None => file.path.to_string_lossy().into_owned(),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(abs.clone()));
        self.push_toast(ToastKind::Success, SharedString::from(abs));
    }

    /// Copy the git command suggestion for the current operation + intent
    /// (ADR-0060 / T-052), e.g. `git merge --continue` / `git rebase --abort` /
    /// `git rebase --skip`.
    pub fn conflict_copy_git_command(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.conflict.as_ref() else {
            return;
        };
        let slug = c.session.op.slug();
        let is_sequencer = c.session.op.is_sequencer();
        // Offer the most useful command for the current state: continue when the
        // gate is open, otherwise abort; sequencer ops also note --skip.
        let cmd = if c.can_continue() {
            format!("git {} --continue", slug)
        } else if is_sequencer {
            format!("git {} --skip   # or: git {} --abort", slug, slug)
        } else {
            format!("git {} --abort", slug)
        };
        cx.write_to_clipboard(ClipboardItem::new_string(cmd.clone()));
        self.push_toast(ToastKind::Success, SharedString::from(cmd));
    }
}
