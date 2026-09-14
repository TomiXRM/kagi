//! Conflict-resolution operations for `KagiApp` (issue #13 Phase 4, P1).
//!
//! Extracted verbatim from `ui/mod.rs`: the conflict editor and the
//! conflict-session operations (`conflict_*`, `confirm/cancel_conflict_continue`).
//! Behaviour is unchanged. Per Rust visibility a descendant module can access
//! the private fields/methods of `KagiApp`, so no visibility was widened.

#![allow(clippy::too_many_arguments)]

use crate::{app, ui::*};

impl KagiApp {
    /// A deferred conflict action (button → next tick via `spawn_in`) must be
    /// dropped when its frozen owner is no longer the exact attachment on
    /// screen: it would otherwise plan or execute against whatever conflict the
    /// now-active tab has (ADR-0197 決定 5 / #722 P1-b). Callbacks capture the
    /// owner at the click and pass it here rather than re-resolving from active.
    ///
    /// It is also refused while the owner's panes await their activation read
    /// (#722 P2): the pane on screen still describes the pre-switch repository.
    pub(crate) fn conflict_action_owner_on_screen(&self, owner: &crate::app::Attachment) -> bool {
        self.active_session() == Some(owner.session)
            && self.app_sessions.attachment(owner.session).as_ref() == Some(owner)
            && !self.ui().panes_revalidating
    }

    /// Marshal a toast from a retained conflict pane, but only while its frozen
    /// owner is the tab on screen (a background pane's toast must not surface).
    pub(crate) fn push_conflict_owner_toast(
        &mut self,
        owner: &crate::app::Attachment,
        kind: ToastKind,
        msg: String,
        cx: &mut Context<Self>,
    ) {
        if self.conflict_action_owner_on_screen(owner) {
            self.push_toast(kind, SharedString::from(msg), cx);
        }
    }
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
        self.ui()
            .conflict
            .as_ref()
            .and_then(|e| e.read(cx).mode.clone())
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
        let current_view = self.ui().conflict.as_ref().map(|view| view.read(cx));
        let current_token = current_view.as_ref().map(|view| view.intent_token);
        let current_revision = current_view
            .as_ref()
            .and_then(|view| view.mode.as_ref().map(|mode| &mode.revision));
        let current_owner = self
            .active_session()
            .and_then(|session| self.app_sessions.attachment(session));
        if self.ui().panes_revalidating
            || !conflict_intent_matches(
                current_token,
                current_revision,
                current_owner.as_ref(),
                token,
                owner,
                revision,
            )
        {
            self.push_toast(ToastKind::Error, "stale conflict action was ignored", cx);
            return;
        }
        match intent {
            conflict_view::FrozenConflictIntent::Request { request, .. } => {
                let _ = self.start_conflict_request(request, cx);
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
                self.present_conflict_action(
                    owner.session,
                    kagi_domain::conflict_family::ConflictAction::Save,
                    recording,
                    cx,
                );
                self.present_app_notice();
            }
        }
    }

    /// Plan → approve → dispatch one conflict-family request. `false` = the
    /// request never became a job (a refused plan, a stale approval); its
    /// recording has already been presented.
    pub(crate) fn start_conflict_request(
        &mut self,
        request: app::ConflictAppRequest,
        cx: &mut Context<Self>,
    ) -> bool {
        let policy = crate::ui::blocking_ops::execution_policy();
        let owner = request.owner.session;
        let action = request.request.action();
        let job = app::plan_conflict(&mut self.app_sessions, request, policy);
        if !app::apply_plan(&mut self.app_sessions, job.run()) {
            return false;
        }
        let token = match self.app_sessions.plan_state() {
            app::PlanState::Ready { token, .. } => token.clone(),
            app::PlanState::Error {
                error, recording, ..
            } => {
                let error = error.clone();
                let recording = recording.clone();
                if let Some(recording) = recording {
                    self.present_conflict_action(owner, action, recording, cx);
                } else {
                    self.push_toast(ToastKind::Error, error.clone(), cx);
                    self.app_notices.push_back(error.clone().into());
                }
                self.present_app_notice();
                return false;
            }
            _ => return false,
        };
        match app::approve(&mut self.app_sessions, token, app::Policy::Conflict(policy)) {
            Ok(approved) => {
                self.dispatch_job(approved, cx);
                true
            }
            Err(error) => {
                self.push_toast(ToastKind::Error, error.to_string(), cx);
                false
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
    pub fn conflict_continue(
        &mut self,
        owner: crate::app::Attachment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.conflict_action_owner_on_screen(&owner) {
            return;
        }
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

        let repo_session = self.ui().repo_session.clone();
        let repo = match repo_session.as_ref() {
            Some(session) => session.backend(),
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
                    .ui()
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
                if let Some(entity) = self.ui().commit_panel.clone() {
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
                if let Some(ui) = self.ui_mut() {
                    ui.conflict_merge_pending = true;
                }
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
                    .ui()
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

        if self.ui().repo_session.is_none() {
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
            .ui()
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
                if let Some(ui) = self.ui_mut() {
                    ui.conflict_detected = false;
                }
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
