//! Conflict-resolution operations for `KagiApp` (issue #13 Phase 4, P1).
//!
//! Extracted verbatim from `ui/mod.rs`: the conflict editor and the
//! conflict-session operations (`conflict_*`, `confirm/cancel_conflict_continue`).
//! Behaviour is unchanged. Per Rust visibility a descendant module can access
//! the private fields/methods of `KagiApp`, so no visibility was widened.

#![allow(clippy::too_many_arguments)]

use crate::ui::*;

impl KagiApp {
    /// ADR-0118 / T-ENTITY-CONFLICT-001: read a clone of the active
    /// [`ConflictMode`] out of the `Entity<ConflictView>`, or `None` when there
    /// is no conflict. Safe to call from `KagiApp` listeners / deferred parent
    /// callbacks (the entity is not leased there); MUST NOT be called from a
    /// leased `ConflictView` listener.
    ///
    /// The buffer-only / view-only editor actions (`conflict_open_editor`,
    /// `conflict_select_file`, `conflict_nav_unresolved`, `conflict_apply_choice`,
    /// `conflict_editor_*`, `conflict_abort_request` arming) moved onto
    /// `ConflictView` (entity-internal — see `conflict_view.rs`). The methods that
    /// remain here drive the Backend (`reload`/`detect`) or read the snapshot, so
    /// they are dispatched via deferred `spawn_in`/`update_in` from child
    /// listeners and operate on the parent.
    fn conflict_mode_snapshot(&self, cx: &Context<Self>) -> Option<conflict_view::ConflictMode> {
        self.conflict.as_ref().and_then(|e| e.read(cx).mode.clone())
    }

    /// Save resolution (ADR-0068 / T-CONFLICT-UX-013/014): write the resolved
    /// Result to the **working tree**, run the marker-residue check (markers
    /// remaining BLOCK the save), then **stage** the file so its index unmerged
    /// entries (stage 1/2/3) collapse to stage 0.  Moves the file into Resolved
    /// Files, re-evaluates the continue gate, autosaves the buffer, and records
    /// the resolution action to the operation log (T-035).  No commit is created.
    pub fn conflict_editor_save(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(entity) = self.conflict.clone() else {
            return;
        };
        let Some(c) = self.conflict_mode_snapshot(cx) else {
            return;
        };

        // Before/after hashes of the file's resolved text for the oplog. The
        // before-text now lives on the entity (`editing_before_text`).
        let before_text = entity
            .read(cx)
            .editing_before_text
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
                    cx,
                );
                return;
            }
        };
        let buffer = c.buffer.clone();
        match repo.execute_conflict_save(&buffer, path) {
            Ok(_outcome) => {
                // Staged → mark the file Resolved and re-evaluate the gate, then
                // record the after-text, all on the entity.
                let after_text_for_entity = after_text.clone();
                entity.update(cx, |v, _| {
                    if let Some(c) = v.mode.as_mut() {
                        let _ = c.buffer.autosave();
                        let residue = c.buffer.files_with_marker_residue();
                        if let Some(f) = c.session.files.iter_mut().find(|f| f.path == path) {
                            f.status = if residue.contains(&f.path) {
                                kagi_git::ConflictStatus::NeedsReview
                            } else {
                                kagi_git::ConflictStatus::Resolved
                            };
                        }
                    }
                    v.editing_before_text
                        .insert(path.to_path_buf(), after_text_for_entity);
                });
                let after = StateSummary {
                    head: format!(
                        "staged (stage 0) before={} after={}",
                        before_hash, after_hash
                    ),
                    dirty: "clean".to_string(),
                };
                self.record_op(
                    &op_name,
                    before,
                    OpOutcome::Success { after },
                    &repo_path,
                    cx,
                );
                // Re-detect so the staged file leaves the conflicted index set.
                self.conflict_detected_for = None;
                self.detect_conflict_mode(cx);
                self.push_toast(
                    ToastKind::Success,
                    SharedString::from(Msg::EditorSavedResolved.t()),
                    cx,
                );
            }
            Err(e) => {
                // Marker residue / write failure: hard block (ADR-0068).
                let err_msg = format!("{}", e);
                self.record_op(
                    &op_name,
                    before,
                    OpOutcome::Refused {
                        blockers: vec![err_msg],
                    },
                    &repo_path,
                    cx,
                );
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(Msg::EditorMarkerWarning.t()),
                    cx,
                );
            }
        }
    }

    /// Continue the in-progress operation (ADR-0068 routing — T-CONFLICT-FLOW-030/
    /// 032).  Gates through `plan_conflict_continue_route`, then:
    ///
    /// - **merge** → transition to the commit message panel pre-filled with the
    ///   merge message (`conflict_merge_pending = true`).  **No commit is
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
        let Some(mode) = self.conflict_mode_snapshot(cx) else {
            return;
        };

        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", "session unavailable")),
                    cx,
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
            Err(e) => {
                klog!("refused: {} blocked: {}", op_name, e);
                // Surface the specific (localized) blocking reason (ADR-0067).
                if let Some(first) = repo.continue_blockers(&mode.session, &mode.buffer).first() {
                    self.push_toast(ToastKind::Error, conflict_view::blocker_msg(first).t(), cx);
                } else {
                    self.push_toast(ToastKind::Error, SharedString::from(format!("{}", e)), cx);
                }
                self.record_op(
                    &op_name,
                    StateSummary {
                        head: format!("op={}", mode.session.op.slug()),
                        dirty: "blocked".to_string(),
                    },
                    OpOutcome::Refused {
                        blockers: vec![format!("{}", e)],
                    },
                    &repo_path,
                    cx,
                );
                cx.notify();
                return;
            }
        };

        match route {
            kagi_git::ContinueRoute::MergeCommitPanel { message } => {
                // Transition to the commit message panel pre-filled with the merge
                // message.  MERGE_HEAD stays present so the commit becomes a merge
                // commit.  No commit is created here (ADR-0068).
                //
                // Stage the resolutions into the index first: the per-file Save is
                // optional, so the index may still hold unmerged entries.  Without
                // this the commit panel shows nothing staged (Commit disabled) and
                // execute_merge_commit refuses the still-conflicted index.
                if let Err(e) = repo.stage_conflict_resolution(&mode.session, &mode.buffer) {
                    klog!("refused: {} stage failed: {}", op_name, e);
                    self.push_toast(
                        ToastKind::Error,
                        SharedString::from(format!("Could not stage resolution: {}", e)),
                        cx,
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
                self.conflict_merge_pending = true;
            }
            kagi_git::ContinueRoute::SequencerPlan(plan) => {
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
        let Some(mode) = self.conflict_mode_snapshot(cx) else {
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
                    cx,
                );
                return;
            }
        };
        let op_name = format!("{}-continue", mode.session.op.slug());

        match repo.execute_conflict_continue(&mode.session, &mode.buffer) {
            Ok(_outcome) => {
                klog!("executed: {}", op_name);
                let _ = kagi_git::ResolutionBuffer::clear(&repo_path);
                let after = StateSummary {
                    head: plan.predicted.head.clone(),
                    dirty: "staged".to_string(),
                };
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                    cx,
                );
                self.clear_conflict_continue_modal();
                self.reload(cx);
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                klog!("{} failed: {}", op_name, err_msg);
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                    cx,
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
        let Some(mode) = self.conflict_mode_snapshot(cx) else {
            return;
        };

        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", "session unavailable")),
                    cx,
                );
                return;
            }
        };

        let plan = match repo.plan_conflict_abort(&mode.session) {
            Ok(p) => p,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("abort plan error: {}", e)),
                    cx,
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
                    cx,
                );
                self.reload(cx);
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                klog!("{} failed: {}", op_name, err_msg);
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Failed { error: err_msg },
                    &repo_path,
                    cx,
                );
            }
        }
        cx.notify();
    }

    // ADR-0118: the two-stage Abort *arming* (first click) is entity-internal
    // (`ConflictView::abort_request_arm`); the *execute* (second click) defers to
    // `conflict_abort` here via `spawn_in`/`update_in`.

    /// Skip the current sequencer step (rebase / cherry-pick / revert) through
    /// the plan pipeline (T-042, ADR-0067): `plan_conflict_skip` → execute →
    /// oplog → re-detect.  Merge has no skip (the button is hidden for merge;
    /// the backend `plan_conflict_skip` also errors for merge as a guard).
    pub fn conflict_skip(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(mode) = self.conflict_mode_snapshot(cx) else {
            return;
        };

        let repo = match self.repo_session.as_ref() {
            Some(s) => s.backend(),
            None => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", "session unavailable")),
                    cx,
                );
                return;
            }
        };

        let plan = match repo.plan_conflict_skip(&mode.session) {
            Ok(p) => p,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("skip plan error: {}", e)),
                    cx,
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
                    cx,
                );
                self.reload(cx);
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                klog!("{} failed: {}", op_name, err_msg);
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Failed { error: err_msg },
                    &repo_path,
                    cx,
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
    pub fn conflict_open_external_tool(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(c) = self.conflict_mode_snapshot(cx) else {
            return;
        };
        let Some(file) = c.session.files.get(idx) else {
            return;
        };

        let template = match settings::read_setting("mergetool") {
            Some(t) if !t.trim().is_empty() => t,
            _ => {
                self.push_toast(ToastKind::Info, Msg::ConflictExternalToolUnset.t(), cx);
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
                cx,
            ),
            Err(e) => self.push_toast(
                ToastKind::Error,
                SharedString::from(format!("external tool failed: {}", e)),
                cx,
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
    pub fn conflict_copy_path(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(c) = self.conflict_mode_snapshot(cx) else {
            return;
        };
        let Some(file) = c.session.files.get(idx) else {
            return;
        };
        let abs = match self.repo_path.clone() {
            Some(p) => p.join(&file.path).to_string_lossy().into_owned(),
            None => file.path.to_string_lossy().into_owned(),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(abs.clone()));
        self.push_toast(ToastKind::Success, SharedString::from(abs), cx);
    }

    /// Copy the git command suggestion for the current operation + intent
    /// (ADR-0060 / T-052), e.g. `git merge --continue` / `git rebase --abort` /
    /// `git rebase --skip`.
    pub fn conflict_copy_git_command(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.conflict_mode_snapshot(cx) else {
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
        self.push_toast(ToastKind::Success, SharedString::from(cmd), cx);
    }
}
