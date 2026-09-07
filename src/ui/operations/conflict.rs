//! Conflict-resolution operations for `KagiApp` (issue #13 Phase 4, P1).
//!
//! Extracted verbatim from `ui/mod.rs`: the conflict editor and the
//! conflict-session operations (`conflict_*`, `confirm/cancel_conflict_continue`).
//! Behaviour is unchanged. Per Rust visibility a descendant module can access
//! the private fields/methods of `KagiApp`, so no visibility was widened.

#![allow(clippy::too_many_arguments)]

use crate::{app, ui::*};

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
    pub(crate) fn conflict_mode_snapshot(
        &self,
        cx: &Context<Self>,
    ) -> Option<conflict_view::ConflictMode> {
        self.conflict.as_ref().and_then(|e| e.read(cx).mode.clone())
    }

    pub(crate) fn accept_conflict_intent(
        &mut self,
        intent: conflict_view::FrozenConflictIntent,
        cx: &mut Context<Self>,
    ) {
        let (token, owner, revision) = match &intent {
            conflict_view::FrozenConflictIntent::Request { token, request } => {
                (*token, &request.owner, request.request.revision())
            }
            conflict_view::FrozenConflictIntent::SaveRefusal {
                token,
                owner,
                revision,
                ..
            } => (*token, owner, revision),
        };
        let current_view = self.conflict.as_ref().map(|view| view.read(cx));
        let current_token = current_view.as_ref().map(|view| view.intent_token);
        let current_revision = current_view
            .as_ref()
            .and_then(|view| view.mode.as_ref().map(|mode| &mode.revision));
        let current_owner = self
            .active_session()
            .and_then(|session| self.app_sessions.attachment(session));
        if !conflict_intent_matches(
            current_token,
            current_revision,
            current_owner.as_ref(),
            token,
            owner,
            revision,
        ) {
            self.push_toast(ToastKind::Error, "stale conflict action was ignored", cx);
            return;
        }
        match intent {
            conflict_view::FrozenConflictIntent::Request { request, .. } => {
                self.start_conflict_request(request, cx)
            }
            conflict_view::FrozenConflictIntent::SaveRefusal {
                owner,
                operation,
                path,
                error,
                ..
            } => {
                let policy = crate::ui::blocking_ops::execution_policy();
                let recording = kagi_git::Backend::record_conflict_save_refusal(
                    &owner.path,
                    policy,
                    &operation,
                    &path,
                    &error,
                );
                self.present_conflict_recording(owner.session, recording, cx);
                self.present_app_notice();
            }
        }
    }

    fn start_conflict_request(&mut self, request: app::ConflictAppRequest, cx: &mut Context<Self>) {
        let policy = crate::ui::blocking_ops::execution_policy();
        let owner = request.owner.session;
        let job = app::plan_conflict(&mut self.app_sessions, request, policy);
        if !app::apply_plan(&mut self.app_sessions, job.run()) {
            return;
        }
        let token = match self.app_sessions.plan_state() {
            app::PlanState::Ready { token, .. } => token.clone(),
            app::PlanState::Error {
                error, recording, ..
            } => {
                let error = error.clone();
                let recording = recording.clone();
                if let Some(recording) = recording {
                    self.present_conflict_recording(owner, recording, cx);
                } else {
                    self.push_toast(ToastKind::Error, error.clone(), cx);
                    self.app_notices.push_back(error.clone().into());
                }
                self.present_app_notice();
                return;
            }
            _ => return,
        };
        match app::approve(&mut self.app_sessions, token, app::Policy::Conflict(policy)) {
            Ok(approved) => self.dispatch_job(approved, cx),
            Err(error) => {
                self.push_toast(ToastKind::Error, error.to_string(), cx);
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
        if self.reject_if_busy(cx) {
            return;
        }
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
                    SharedString::from(i18n::op_failed(i18n::Op::RepoOpen, "session unavailable")),
                    cx,
                );
                return;
            }
        };

        let op_name = format!("{}-continue", mode.session.op.slug());
        if matches!(mode.session.op, kagi_git::ConflictOp::StashConflict) {
            if let Some(owner) = self.active_session() {
                self.app_sessions.observe_stash_conflict(
                    owner,
                    &repo.stash_conflict_identity().unwrap_or_default(),
                );
            }
        }
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
                self.record_op_persist(
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
                let Some(guard) = self.reserve_write("conflict-continue", &repo_path, cx) else {
                    return;
                };
                let result = self
                    .repo_session
                    .as_ref()
                    .expect("repo session existed while planning merge continue")
                    .backend()
                    .stage_conflict_resolution(&mode.session, &mode.buffer);
                let unknown = app::settle_conflict_write(
                    guard,
                    &result,
                    StateSummary {
                        head: format!("op={}", mode.session.op.slug()),
                        dirty: "resolving".to_string(),
                    },
                );
                self.refresh_write_busy();
                if let Err(e) = result {
                    klog!("refused: {} stage failed: {}", op_name, e);
                    let error = format!("Could not stage resolution: {}", e);
                    if let Some(outcome) = unknown {
                        self.record_op_persist(
                            &op_name,
                            StateSummary {
                                head: format!("op={}", mode.session.op.slug()),
                                dirty: "resolving".to_string(),
                            },
                            outcome,
                            &repo_path,
                            cx,
                        );
                    } else {
                        self.push_toast(ToastKind::Error, SharedString::from(error), cx);
                    }
                    cx.notify();
                    return;
                }
                eprintln!(
                    "[kagi] {}: routing to commit message panel (merge)",
                    op_name
                );
                self.open_commit_panel(window, cx);
                // ADR-0118: seed the merge message into the entity's input + state.
                // `open_commit_panel` runs on the parent (this method is the parent,
                // deferred from the ConflictView Continue listener — correction #6),
                // so updating the freshly-created CommitPanelView here is safe.
                if let Some(entity) = self.commit_panel.clone() {
                    let (title_input, body_input) = {
                        let v = entity.read(cx);
                        (v.title_input.clone(), v.body_input.clone())
                    };
                    let (title, body) = kagi_git::split_title_body(&message);
                    if let Some(input) = title_input {
                        input.update(cx, |state, cx| state.set_value(title, window, cx));
                    }
                    if let Some(input) = body_input {
                        input.update(cx, |state, cx| state.set_value(body, window, cx));
                    }
                    entity.update(cx, |v, _| v.state.commit_msg = message.clone());
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
            kagi_git::ContinueRoute::StashComplete => {
                // #309: a stash conflict is not a commit — "continue" just stages
                // the resolved paths (execute_conflict_continue collapses the
                // unmerged entries to stage 0). No merge commit, no `--continue`.
                let Some(guard) = self.reserve_write("conflict-continue", &repo_path, cx) else {
                    return;
                };
                let result = self
                    .repo_session
                    .as_ref()
                    .expect("repo session existed while planning conflict continue")
                    .backend()
                    .execute_conflict_continue(&mode.session, &mode.buffer);
                let unknown = app::settle_conflict_write(
                    guard,
                    &result,
                    StateSummary {
                        head: format!("op={}", mode.session.op.slug()),
                        dirty: "resolving".to_string(),
                    },
                );
                self.refresh_write_busy();
                match result {
                    Ok(result) => {
                        klog!("executed: {}", op_name);
                        let _ = kagi_git::ResolutionBuffer::clear(&repo_path);
                        self.record_op_persist(
                            &op_name,
                            StateSummary {
                                head: format!("op={}", mode.session.op.slug()),
                                dirty: "resolving".to_string(),
                            },
                            OpOutcome::Success {
                                after: result.after.clone(),
                            },
                            &repo_path,
                            cx,
                        );
                        // Consume this owner's proven OID once, after reload's
                        // modal clear. A fresh unique-OID plan requires new approval.
                        if let Some(owner) = self.active_session() {
                            self.app_sessions.continue_stash_conflict(owner);
                        }
                        // Conflicts are gone → re-detect clears Conflict Mode.
                        self.reload(cx);
                    }
                    Err(e) => {
                        let err_msg = format!("{}", e);
                        let is_unknown = unknown.is_some();
                        klog!("{} failed: {}", op_name, err_msg);
                        let outcome = unknown.unwrap_or_else(|| OpOutcome::Failed {
                            error: err_msg.clone(),
                        });
                        self.record_op_persist(
                            &op_name,
                            StateSummary {
                                head: format!("op={}", mode.session.op.slug()),
                                dirty: "resolving".to_string(),
                            },
                            outcome,
                            &repo_path,
                            cx,
                        );
                        if !is_unknown {
                            self.push_toast(ToastKind::Error, SharedString::from(err_msg), cx);
                        }
                    }
                }
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

        if self.repo_session.is_none() {
            self.push_toast(
                ToastKind::Error,
                SharedString::from(i18n::op_failed(i18n::Op::RepoOpen, "session unavailable")),
                cx,
            );
            return;
        }
        let op_name = format!("{}-continue", mode.session.op.slug());

        let Some(guard) = self.reserve_write("conflict-continue", &repo_path, cx) else {
            return;
        };
        let result = self
            .repo_session
            .as_ref()
            .expect("repo session existed while planning conflict continue")
            .backend()
            .execute_conflict_continue(&mode.session, &mode.buffer);
        let unknown = app::settle_conflict_write(guard, &result, plan.current.clone());
        self.refresh_write_busy();
        match result {
            Ok(result) => {
                klog!("executed: {}", op_name);
                let _ = kagi_git::ResolutionBuffer::clear(&repo_path);
                // #296: record the REAL measured post-continue state, not the
                // plan's predicted head — a partial / new-conflict continuation
                // must not be logged as a clean success.
                let after = result.after.clone();
                self.record_op_persist(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                    cx,
                );
                self.clear_conflict_continue_modal();
                self.reload(cx);
                self.conflict_detected_for = None;
                self.detect_conflict_mode(cx);
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                let unknown_evidence = unknown.as_ref().and_then(|outcome| match outcome {
                    OpOutcome::Unknown { evidence, .. } => Some(evidence.clone()),
                    _ => None,
                });
                klog!("{} failed: {}", op_name, err_msg);
                let outcome = unknown.unwrap_or_else(|| OpOutcome::Failed {
                    error: err_msg.clone(),
                });
                self.record_op_persist(&op_name, plan.current.clone(), outcome, &repo_path, cx);
                if let Some(modal) = self.conflict_continue_modal_mut() {
                    modal.error = Some(SharedString::from(
                        unknown_evidence.clone().unwrap_or(err_msg),
                    ));
                }
                if let Some(evidence) = unknown_evidence {
                    self.report_unknown_notice(&repo_path, evidence);
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
        if self.reject_if_busy(cx) {
            return;
        }
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
                    SharedString::from(i18n::op_failed(i18n::Op::RepoOpen, "session unavailable")),
                    cx,
                );
                return;
            }
        };

        if matches!(mode.session.op, kagi_git::ConflictOp::StashConflict) {
            if let Some(owner) = self.active_session() {
                self.app_sessions.observe_stash_conflict(
                    owner,
                    &repo.stash_conflict_identity().unwrap_or_default(),
                );
            }
        }
        let plan = match repo.plan_conflict_abort(&mode.session) {
            Ok(p) => p,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(i18n::op_plan_failed(i18n::Op::Abort, e)),
                    cx,
                );
                return;
            }
        };
        let op_name = format!("{}-abort", mode.session.op.slug());
        let Some(guard) = self.reserve_write("conflict-abort", &repo_path, cx) else {
            return;
        };

        // #309: a stash conflict has no ORIG_HEAD / sequencer state — abort
        // restores HEAD for the conflicted paths and keeps the stash, rather than
        // moving refs back via ORIG_HEAD (execute_conflict_abort's path).
        let abort_result = if matches!(mode.session.op, kagi_git::ConflictOp::StashConflict) {
            self.repo_session
                .as_ref()
                .expect("repo session existed while planning conflict abort")
                .backend()
                .execute_stash_conflict_abort(&mode.session, &mode.buffer)
        } else {
            self.repo_session
                .as_ref()
                .expect("repo session existed while planning conflict abort")
                .backend()
                .execute_conflict_abort(&mode.session, &mode.buffer)
        };
        let unknown = app::settle_conflict_write(guard, &abort_result, plan.current.clone());
        self.refresh_write_busy();
        match abort_result {
            Ok(_outcome) => {
                if let Some(owner) = self.active_session() {
                    self.app_sessions.clear_stash_conflict(owner);
                }
                klog!("executed: {}", op_name);
                let after = StateSummary {
                    head: plan.predicted.head.clone(),
                    dirty: "clean".to_string(),
                };
                self.record_op_persist(
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
                let is_unknown = unknown.is_some();
                klog!("{} failed: {}", op_name, err_msg);
                let outcome = unknown.unwrap_or_else(|| OpOutcome::Failed {
                    error: err_msg.clone(),
                });
                self.record_op_persist(&op_name, plan.current.clone(), outcome, &repo_path, cx);
                // Without this the failure only reached the oplog: no toast, no
                // modal, no reload — so the UI kept rendering the pre-failure
                // conflict state and the user believed the abort/skip had
                // happened. CLAUDE.md: errors surface via the oplog *and* the UI.
                if !is_unknown {
                    self.push_toast(ToastKind::Error, SharedString::from(err_msg), cx);
                }
            }
        }
        cx.notify();
    }

    // ADR-0118: the two-stage Abort *arming* (first click) is entity-internal
    // (`ConflictView::abort_request_arm`); the *execute* (second click) defers to
    // `conflict_abort` here via `spawn_in`/`update_in`.

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
                SharedString::from(i18n::op_failed(i18n::Op::ExternalTool, e)),
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

fn conflict_intent_matches(
    current_token: Option<u64>,
    current_revision: Option<&kagi_domain::conflict_family::ConflictRevision>,
    current_owner: Option<&app::Attachment>,
    frozen_token: u64,
    frozen_owner: &app::Attachment,
    frozen_revision: &kagi_domain::conflict_family::ConflictRevision,
) -> bool {
    current_token == Some(frozen_token)
        && current_revision == Some(frozen_revision)
        && current_owner == Some(frozen_owner)
}

#[cfg(test)]
mod intent_tests {
    use super::*;
    use std::path::PathBuf;

    fn owner(tab: u64) -> app::Attachment {
        app::Attachment {
            session: app::SessionId {
                tab: app::TabId(tab),
                incarnation: tab,
            },
            path: PathBuf::from(format!("repo-{tab}")),
            worktree: None,
            visit: 0,
        }
    }

    #[test]
    fn old_view_intent_is_rejected_after_next_step_or_tab_switch() {
        let frozen_owner = owner(1);
        let other_owner = owner(2);
        let old_revision =
            kagi_domain::conflict_family::ConflictRevision::from_fingerprint("old".to_string());
        let next_revision =
            kagi_domain::conflict_family::ConflictRevision::from_fingerprint("next".to_string());

        assert!(conflict_intent_matches(
            Some(7),
            Some(&old_revision),
            Some(&frozen_owner),
            7,
            &frozen_owner,
            &old_revision,
        ));
        assert!(!conflict_intent_matches(
            Some(8),
            Some(&old_revision),
            Some(&frozen_owner),
            7,
            &frozen_owner,
            &old_revision,
        ));
        assert!(!conflict_intent_matches(
            Some(7),
            Some(&next_revision),
            Some(&frozen_owner),
            7,
            &frozen_owner,
            &old_revision,
        ));
        assert!(!conflict_intent_matches(
            Some(7),
            Some(&old_revision),
            Some(&other_owner),
            7,
            &frozen_owner,
            &old_revision,
        ));
    }
}

// Conflict-detection outcome types + the detect/apply halves, moved from
// `src/ui/mod.rs` (T-HOTSPOT-UIMOD-001). Behaviour-preserving relocation.
/// T-PERF-RENDER-001: the `Send` result of the read-only conflict-detection I/O
/// (`KagiApp::detect_conflict_payload`), applied to `KagiApp` on the UI thread by
/// `apply_conflict_detect`.  Splitting the I/O out of the state mutation lets the
/// same detection run either synchronously (`reload`) or off the UI thread
/// (`detect_conflict_mode_async`) without changing the emitted `[kagi]` lines.
pub(crate) enum ConflictDetectOutcome {
    /// `Backend::open` failed — leave `merge_commit_ready` untouched, clear mode.
    OpenFailed,
    /// No conflict session — clear Conflict Mode (emits `conflict-mode: cleared`
    /// only when a mode was previously open).
    Cleared,
    /// A merge with MERGE_HEAD but no unmerged entries — resolved, ready to commit.
    MergeResolvedReady,
    /// An active conflict/merge with files to resolve.  Boxed: the session +
    /// resolution buffer are large, and this variant is the rare case.
    Detected(Box<ConflictDetected>),
}

/// Payload of [`ConflictDetectOutcome::Detected`] — the assembled conflict state
/// the UI-thread apply moves into `self.conflict`.
pub(crate) struct ConflictDetected {
    stash_identity: Vec<String>,
    session: kagi_git::conflicts::ConflictSession,
    observation: kagi_domain::conflict_family::ConflictObservation,
    buffer: kagi_git::resolution::ResolutionBuffer,
    current_branch: String,
    selected_file: Option<usize>,
    editing_file: Option<usize>,
    /// Selected content file whose hunks were materialized; set as the open
    /// editor file on apply.
    editing_path: Option<PathBuf>,
}

impl KagiApp {
    /// Read-only conflict detection: opens the repo, detects the session, builds
    /// the resolution buffer, recomputes per-file status, auto-selects a file, and
    /// materializes zdiff3 markers for the selected content file.  This is the
    /// **entire I/O half** of conflict detection — pure inputs/outputs (no `self`)
    /// so it runs either synchronously (`detect_conflict_mode`) or on a background
    /// thread (`detect_conflict_mode_async`).  `current_branch` and the `prev_*`
    /// preservation indices are captured by the caller from `self`.
    pub(crate) fn detect_conflict_payload(
        repo_path: &Path,
        prev_selected_path: Option<PathBuf>,
        prev_editing_path: Option<PathBuf>,
        current_branch: String,
    ) -> ConflictDetectOutcome {
        let repo = match crate::ui::blocking_ops::open_backend(repo_path) {
            Ok(r) => r,
            Err(_) => return ConflictDetectOutcome::OpenFailed,
        };

        let snapshot = match repo.conflict_snapshot() {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => return ConflictDetectOutcome::Cleared,
            Err(_) => return ConflictDetectOutcome::OpenFailed,
        };
        let session = snapshot.session;

        // A merge with MERGE_HEAD present but no remaining unmerged index entries
        // is not a conflict to resolve — it is a resolved merge ready to commit.
        if matches!(session.op, kagi_git::ConflictOp::Merge { .. }) && session.files.is_empty() {
            return ConflictDetectOutcome::MergeResolvedReady;
        }

        // Build / reload the resolution buffer.  A previously-autosaved buffer
        // (e.g. from before a restart) is preferred so partial work survives;
        // otherwise materialize a fresh buffer from the index conflicts.
        // #297: index-authoritative buffer with autosaved drafts overlaid. The
        // old `load().or_else(from_repo)` let an autosave (which does not persist
        // the index-derived `raw` metadata) short-circuit `from_repo`, so a
        // binary/symlink "take side" failed with "that side does not exist".
        let mut buffer = repo
            .resolution_buffer_from_repo_with_autosave()
            .unwrap_or_else(|_| kagi_git::ResolutionBuffer::new(repo_path));

        // Recompute per-file status from the buffer (detection seeds Unresolved).
        let mut session = session;
        let residue = buffer.files_with_marker_residue();
        for f in &mut session.files {
            if buffer.has_resolution(&f.path) {
                f.status = if residue.contains(&f.path) {
                    kagi_git::ConflictStatus::NeedsReview
                } else {
                    kagi_git::ConflictStatus::Resolved
                };
            } else {
                f.status = kagi_git::ConflictStatus::Unresolved;
            }
        }

        // Preserve the previously-selected file across re-detections by PATH
        // (issue #285): a per-file Save re-sorts / renumbers `files`, so the old
        // index would silently point at a different file. Fall back to the first
        // unresolved file (KDiff3-style "land on work to do"), then index 0.
        let selected_file =
            kagi_git::resolve_selected_file(&session.files, prev_selected_path.as_deref());

        // W33: preserve the dashboard editing file across re-detection — by PATH,
        // for the same reason (issue #285). Dropped if that file is gone.
        let editing_file = prev_editing_path
            .as_deref()
            .and_then(|p| session.files.iter().position(|f| f.path == p));

        // The center A/B editor renders from the hunk model, which needs the repo
        // to materialize zdiff3 markers.  With auto-selection the user never
        // clicked, so build the hunk model for the selected content file here.
        let mut editing_path = None;
        if let Some(idx) = selected_file {
            if let Some(f) = session.files.get(idx) {
                if f.kind == kagi_git::ConflictKind::Content {
                    let path = f.path.clone();
                    if let Some(markers) = repo.materialized_markers(&buffer, &path) {
                        buffer.ensure_hunks(&path, &markers);
                    }
                    editing_path = Some(path);
                }
            }
        }

        ConflictDetectOutcome::Detected(Box::new(ConflictDetected {
            stash_identity: if matches!(session.op, kagi_git::ConflictOp::StashConflict) {
                repo.stash_conflict_identity().unwrap_or_default()
            } else {
                vec![]
            },
            session,
            observation: snapshot.observation,
            buffer,
            current_branch,
            selected_file,
            editing_file,
            editing_path,
        }))
    }

    /// Foreground half of conflict detection: apply a [`ConflictDetectOutcome`]
    /// computed by [`detect_conflict_payload`] to `self`, emitting the same
    /// `[kagi]` contract lines in the same order as the original synchronous
    /// implementation. ADR-0118: this is the single point that builds / updates /
    /// drops the `Entity<ConflictView>` — `Detected` updates an existing entity in
    /// place (preserving its splits / editor inputs / before-text) or creates a
    /// new one; `Cleared` / `MergeResolvedReady` / `OpenFailed` drop it. The
    /// "was a conflict open?" (Cleared) and editor-close (Detected) checks read
    /// the entity here because they must reflect the current UI state at apply
    /// time. Needs `cx` (entity create / read / update).
    pub(crate) fn apply_conflict_detect(
        &mut self,
        outcome: ConflictDetectOutcome,
        cx: &mut Context<Self>,
    ) {
        let conflict_observation = match &outcome {
            ConflictDetectOutcome::Detected(detected) => Some(detected.observation.clone()),
            _ => None,
        };
        if let Some(owner) = self.active_session() {
            let identity = match &outcome {
                ConflictDetectOutcome::Detected(d) => d.stash_identity.as_slice(),
                _ => &[],
            };
            if matches!(outcome, ConflictDetectOutcome::OpenFailed) {
                self.app_sessions.clear_stash_conflict(owner);
            } else {
                self.app_sessions.observe_stash_conflict(owner, identity);
            }
            self.app_sessions
                .observe_conflict(owner, conflict_observation);
        }
        match outcome {
            ConflictDetectOutcome::OpenFailed => {
                // Mirrors the original early-return on `Backend::open` failure,
                // which happened before `merge_commit_ready` was reset — so that
                // flag is intentionally left untouched here.
                self.conflict = None;
            }
            ConflictDetectOutcome::Cleared => {
                self.merge_commit_ready = false;
                if self
                    .conflict
                    .as_ref()
                    .is_some_and(|e| e.read(cx).mode.is_some())
                {
                    klog!("conflict-mode: cleared");
                }
                // Drop the entity (clears mode + editing + splits + before-text;
                // the accepted Stage-1 reset delta on re-entry).
                self.conflict = None;
            }
            ConflictDetectOutcome::MergeResolvedReady => {
                self.merge_commit_ready = false;
                klog!("conflict-mode: merge resolved — ready to commit");
                self.merge_commit_ready = true;
                self.conflict = None;
            }
            ConflictDetectOutcome::Detected(detected) => {
                let ConflictDetected {
                    stash_identity: _,
                    session,
                    observation,
                    buffer,
                    current_branch,
                    selected_file,
                    editing_file,
                    editing_path,
                } = *detected;
                self.merge_commit_ready = false;
                eprintln!(
                    "[kagi] conflict-mode: {} {} file(s)",
                    session.op.slug(),
                    session.files.len()
                );

                let mode = conflict_view::ConflictMode {
                    revision: observation.revision,
                    session,
                    buffer,
                    current_branch,
                    selected_file,
                    editing_file,
                    abort_armed: false,
                    skip_armed: false,
                };
                let files = mode.session.files.clone();

                match self.conflict.clone() {
                    // Re-detect: update the existing entity in place so its splits
                    // / editor inputs / before-text / scroll survive the reload.
                    Some(entity) => {
                        entity.update(cx, |v, _| {
                            let prev_editing = v.editing.clone();
                            // W32: close the editor if the edited file is no longer
                            // conflicted (reads the entity's current `editing`).
                            if let Some(editing) = v.editing.clone() {
                                if !files.iter().any(|f| f.path == editing) {
                                    v.editing = None;
                                }
                            }
                            v.mode = Some(mode);
                            if let Some(path) = editing_path {
                                v.editing = Some(path);
                            }
                            // Issue #285: the editor file just changed, so the
                            // stored hunk index belongs to the old file — reset it
                            // (the new file may have fewer hunks).
                            if v.editing != prev_editing {
                                v.selected_hunk = 0;
                            }
                        });
                    }
                    // Fresh conflict: build the entity, capturing the repo path +
                    // a weak back-ref for its deferred parent callbacks.
                    None => {
                        let weak_app = cx.weak_entity();
                        let repo_path = self.repo_path.clone().unwrap_or_default();
                        let Some(owner) = self
                            .active_session()
                            .and_then(|session| self.app_sessions.attachment(session))
                        else {
                            self.conflict = None;
                            return;
                        };
                        let entity = cx.new(|_| {
                            let mut v =
                                conflict_view::ConflictView::new(weak_app, repo_path, owner);
                            v.mode = Some(mode);
                            v.editing = editing_path;
                            v
                        });
                        self.conflict = Some(entity);
                    }
                }
            }
        }
    }
}
