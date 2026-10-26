//! Conflict-resolution operations for `KagiApp` (issue #13 Phase 4, P1).
//!
//! Extracted verbatim from `ui/mod.rs`: the conflict editor and the
//! conflict-session operations (`conflict_*`, `confirm/cancel_conflict_continue`).
//! Behaviour is unchanged. Per Rust visibility a descendant module can access
//! the private fields/methods of `KagiApp`, so no visibility was widened.

#![allow(clippy::too_many_arguments)]

use crate::{app, ui::*};
pub(super) fn conflict_unfinished_recording(
    op: &str,
    repo: &std::path::Path,
    before: StateSummary,
    error: &kagi_git::GitError,
) -> kagi_git::backend::recording::Recording {
    let outcome = kagi_git::backend::conflict_continue::conflict_completion_error(error, &before);
    kagi_git::backend::recording::finalize(
        kagi_git::oplog::OpLogEntry::new(op, repo.display().to_string(), before, outcome)
            .with_worktree(Some(repo.display().to_string())),
    )
}

#[cfg(feature = "gui-e2e")]
static PANIC_NEXT_CONTINUE_MERGE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "gui-e2e")]
static PANIC_NEXT_CONTINUE_STASH: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "gui-e2e")]
static PANIC_NEXT_CONTINUE_CONFIRM: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "gui-e2e")]
thread_local! {
    static CONTINUE_MERGE_HOLD: std::cell::RefCell<Option<gpui::Task<()>>> =
        const { std::cell::RefCell::new(None) };
    static CONTINUE_CONFIRM_HOLD: std::cell::RefCell<Option<gpui::Task<()>>> =
        const { std::cell::RefCell::new(None) };
    static CONTINUE_COMPLETION_HOLD: std::cell::RefCell<
        Option<(gpui::Task<()>, std::sync::Arc<std::sync::atomic::AtomicBool>)>
    > = const { std::cell::RefCell::new(None) };
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    pub fn panic_next_continue_merge_for_e2e() {
        PANIC_NEXT_CONTINUE_MERGE.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    pub fn panic_next_continue_stash_for_e2e() {
        PANIC_NEXT_CONTINUE_STASH.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    pub fn panic_next_continue_confirm_for_e2e() {
        PANIC_NEXT_CONTINUE_CONFIRM.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    pub fn hold_next_continue_merge_for_e2e(hold: gpui::Task<()>) {
        CONTINUE_MERGE_HOLD.with(|slot| assert!(slot.borrow_mut().replace(hold).is_none()));
    }
    pub fn hold_next_continue_confirm_for_e2e(hold: gpui::Task<()>) {
        CONTINUE_CONFIRM_HOLD.with(|slot| assert!(slot.borrow_mut().replace(hold).is_none()));
    }
    pub fn clear_continue_confirm_hold_for_e2e() {
        CONTINUE_CONFIRM_HOLD.with(|slot| {
            drop(slot.borrow_mut().take());
        });
    }
    /// Pause after real background execution, before foreground recording/reload.
    /// The returned flag signals that the completion has reached this boundary.
    pub fn hold_next_continue_completion_for_e2e(
        hold: gpui::Task<()>,
    ) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        let reached = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        CONTINUE_COMPLETION_HOLD.with(|slot| {
            assert!(slot.borrow_mut().replace((hold, reached.clone())).is_none());
        });
        reached
    }
    pub fn clear_continue_completion_hold_for_e2e() {
        CONTINUE_COMPLETION_HOLD.with(|slot| {
            drop(slot.borrow_mut().take());
        });
    }
}

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
        self.pane_mutation_admitted(owner.session)
            && self.app_sessions.attachment(owner.session).as_ref() == Some(owner)
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

    /// Complete the recovery barrier before any plan approval or repository lock.
    /// The retained pane and attachment must still own the acknowledged generation.
    pub(crate) fn flush_conflict_drafts(
        &mut self,
        owner: crate::app::Attachment,
        action: &'static str,
        cx: &mut Context<Self>,
        resume: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
    ) {
        // Restore failures own the retained pane and its existing diagnostic.
        // Admission must precede even capture/flush, not only the later writer.
        if !self.pane_mutation_admitted(owner.session) {
            match action {
                "continue" => self.clear_conflict_continue_modal(),
                "abort" => self.clear_conflict_abort_modal(),
                _ => {}
            }
            cx.notify();
            return;
        }
        let Some(pane) = self
            .ui
            .get(&owner.session)
            .and_then(|ui| ui.conflict.clone())
        else {
            resume(self, cx);
            return;
        };
        if pane.read(cx).draft_flushes > 0 {
            return;
        }
        let task = pane.update(cx, |view, cx| view.flush_drafts(cx));
        cx.spawn(async move |this, cx| {
            let saved = task.await;
            let _ = this.update(cx, |app, cx| {
                if !app.conflict_action_owner_on_screen(&owner)
                    || app
                        .ui
                        .get(&owner.session)
                        .and_then(|ui| ui.conflict.as_ref())
                        != Some(&pane)
                {
                    return;
                }
                if saved {
                    resume(app, cx);
                } else {
                    if pane.read(cx).draft_persist_failed.is_none() {
                        app.push_toast(ToastKind::Error, "stale conflict action was ignored", cx);
                        cx.notify();
                        return;
                    }
                    if action == "abort" {
                        if let Some(operation) = app.view().operation.clone() {
                            app.refuse_conflict_abort_for_failed_draft(&owner, &operation, cx);
                            app.clear_conflict_abort_modal();
                            cx.notify();
                            return;
                        }
                    }
                    let blocker = kagi_domain::plan_note::PlanNote::Conflicts(
                        kagi_domain::plan_note::ConflictsNote::DraftPreservationFailed,
                    );
                    let message = super::super::i18n::plan_note_text(&blocker);
                    let entry = Self::refused_entry(
                        &format!(
                            "{}-{action}",
                            app.view()
                                .operation
                                .as_ref()
                                .map(|op| op.kind().slug())
                                .unwrap_or("conflict")
                        ),
                        StateSummary {
                            head: "conflict".into(),
                            dirty: "unsaved decisions".into(),
                        },
                        &[blocker],
                        &owner.path,
                    );
                    let recording = kagi_git::backend::recording::finalize(entry);
                    app.present_operation_completion(&recording, true, false, Some(message), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn accept_conflict_intent(
        &mut self,
        intent: conflict_view::FrozenConflictIntent,
        cx: &mut Context<Self>,
    ) {
        let owner = match &intent {
            conflict_view::FrozenConflictIntent::Request { request, .. } => request.owner.clone(),
            conflict_view::FrozenConflictIntent::SaveRefusal { owner, .. } => owner.clone(),
        };
        self.flush_conflict_drafts(owner, "save", cx, move |app, cx| {
            app.accept_flushed_conflict_intent(intent, cx);
        });
    }

    fn accept_flushed_conflict_intent(
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
        if !self.pane_mutation_admitted(owner.session)
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
        let save_path = match &intent {
            conflict_view::FrozenConflictIntent::Request { request, .. } => {
                match &request.request {
                    kagi_domain::conflict_family::ConflictRequest::Save { path, .. } => {
                        Some(path.clone())
                    }
                    _ => None,
                }
            }
            conflict_view::FrozenConflictIntent::SaveRefusal { path, .. } => Some(path.clone()),
        };
        if let Some(path) = save_path {
            if let Some(view) = self.ui().conflict.as_ref() {
                view.update(cx, |view, _| view.save_attempt = Some(path));
            }
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
                    None,
                    cx,
                );
                self.present_app_notice();
            }
        }
    }

    /// Prepare one conflict-family request. Ordinary Save retains its explicit
    /// Save approval; deletion and directory/file choices require the frozen
    /// plan sheet's separate confirmation before dispatch.
    pub(crate) fn start_conflict_request(
        &mut self,
        request: app::ConflictAppRequest,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.pane_mutation_admitted(request.owner.session) {
            return false;
        }
        use kagi_domain::conflict_family::{ConflictDraft, ConflictRequest};
        let needs_confirmation = matches!(
            &request.request,
            ConflictRequest::Save {
                draft: ConflictDraft::Delete,
                ..
            } | ConflictRequest::ResolveDirFile { .. }
        );
        let attachment = request.owner.clone();
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
                error,
                recording,
                blocker,
                ..
            } => {
                let error = error.clone();
                let recording = recording.clone();
                let blocker = blocker.clone();
                if let Some(recording) = recording {
                    self.present_conflict_action(owner, action, recording, blocker.as_ref(), cx);
                } else {
                    self.push_toast(ToastKind::Error, error.clone(), cx);
                    self.app_notices.push_back(error.clone().into());
                }
                self.present_app_notice();
                return false;
            }
            _ => return false,
        };
        if needs_confirmation {
            let app::PlanState::Ready {
                prepared: app::Planned::Conflict { plan, .. },
                ..
            } = self.app_sessions.plan_state()
            else {
                return false;
            };
            let changes = plan.path_changes();
            use crate::ui::modals::conflict_resolution::ResolutionKind;
            use kagi_domain::resolution::DirFileChoice;
            let (kind, path) = match plan.request() {
                ConflictRequest::Save {
                    path,
                    draft: ConflictDraft::Delete,
                    ..
                } => (ResolutionKind::Remove, path.clone()),
                ConflictRequest::ResolveDirFile { path, choice, .. } => (
                    match choice {
                        DirFileChoice::KeepDirectory => ResolutionKind::KeepDirectory,
                        DirFileChoice::KeepFile => ResolutionKind::KeepFile,
                    },
                    path.clone(),
                ),
                _ => return false,
            };
            let revision = plan.request().revision().clone();
            let Some((intent_token, continuation)) = self.ui().conflict.as_ref().and_then(|pane| {
                let pane = pane.read(cx);
                let mode = pane.mode.as_ref()?;
                let continuation = if mode.session.files.len() == 1
                    && matches!(mode.session.op, kagi_git::ConflictOp::Merge { .. })
                {
                    Msg::ConflictToCommit
                } else {
                    Msg::ConflictReviewContinue
                };
                Some((pane.intent_token, continuation))
            }) else {
                return false;
            };
            self.set_conflict_resolution_modal(
                crate::ui::modals::conflict_resolution::ConflictResolutionModal {
                    token,
                    owner: attachment,
                    revision,
                    intent_token,
                    changes,
                    kind,
                    path,
                    continuation,
                },
            );
            cx.notify();
            return true;
        }
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
        let handle = window.window_handle();
        self.flush_conflict_drafts(owner.clone(), "continue", cx, move |app, cx| {
            let token = app
                .ui()
                .conflict
                .as_ref()
                .map(|pane| pane.read(cx).intent_token);
            let this = cx.entity().downgrade();
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = this.update(cx, |app, cx| {
                        let current = app
                            .ui()
                            .conflict
                            .as_ref()
                            .map(|pane| pane.read(cx).intent_token);
                        if current != token || app.failed_conflict_draft_for(owner.session, cx) {
                            return;
                        }
                        app.conflict_continue_flushed(owner, window, cx)
                    });
                });
            });
        });
    }

    fn conflict_continue_flushed(
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
                    SharedString::from(i18n::op::op_failed_contract(
                        i18n::Op::RepoOpen,
                        "session unavailable",
                    )),
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
            &mode.revision,
        ) {
            Ok(r) => r,
            Err(e) => {
                klog!("refused: {} blocked: {}", op_name, e);
                // Preserve the typed localized blocker in the single result
                // owner; an append failure takes priority over this preview.
                let notice = repo
                    .continue_blockers(&mode.session, &mode.buffer)
                    .ok()
                    .and_then(|blockers| blockers.into_iter().next())
                    .map(|first| {
                        (
                            ToastKind::Error,
                            conflict_view::blocker_msg(&first).t().into(),
                        )
                    });
                // Refused at planning: nothing ran, so "recorded, nothing
                // moved" — not a missing record that blocks a restore (#891).
                let entry = kagi_git::oplog::OpLogEntry::new(
                    &op_name,
                    repo_path.display().to_string(),
                    StateSummary {
                        head: format!("op={}", mode.session.op.slug()),
                        dirty: "blocked".to_string(),
                    },
                    OpOutcome::Refused {
                        blockers: vec![format!("{}", e)],
                    },
                )
                .with_ref_moves(Some(Vec::new()));
                self.record_op_with_notice(entry, cx, true, None, notice, false);
                cx.notify();
                return;
            }
        };

        match route {
            kagi_git::ContinueRoute::MergeCommitPanel {
                message,
                comment_char,
            } => {
                // Transition to the commit message panel pre-filled with the merge
                // message.  MERGE_HEAD stays present so the commit becomes a merge
                // commit.  No commit is created here (ADR-0068).
                //
                // Stage the resolutions into the index first: the per-file Save is
                // optional, so the index may still hold unmerged entries.  Without
                // this the commit panel shows nothing staged (Commit disabled) and
                // execute_merge_commit refuses the still-conflicted index.
                let Some(guard) = self.reserve_write_for(
                    "conflict-continue",
                    &op_name,
                    &repo_path,
                    self.active_session(),
                    cx,
                ) else {
                    return;
                };
                let before = StateSummary {
                    head: format!("op={}", mode.session.op.slug()),
                    dirty: "resolving".to_string(),
                };
                let session_op = mode.session.op.slug().to_string();
                let expected_revision = mode.revision;
                let session = mode.session;
                let buffer = mode.buffer;
                let owner = owner.session;
                let visit = self.app_sessions.visit(owner);
                let abandonment = guard.abandonment();
                let supervision = abandonment.supervision();
                let bg_path = repo_path.clone();
                #[cfg(feature = "gui-e2e")]
                let hold = CONTINUE_MERGE_HOLD.with(|slot| slot.borrow_mut().take());
                let task = cx.background_spawn(async move {
                    let _supervised = kagi_git::proc::supervisor::enter(supervision);
                    #[cfg(feature = "gui-e2e")]
                    if let Some(hold) = hold {
                        hold.await;
                    }
                    #[cfg(feature = "gui-e2e")]
                    if PANIC_NEXT_CONTINUE_MERGE.swap(false, std::sync::atomic::Ordering::SeqCst) {
                        let _ =
                            std::panic::catch_unwind(|| panic!("injected merge continue panic"));
                        return None;
                    }
                    let report = kagi_git::Backend::run_recorded_conflict_handoff(
                        &bg_path,
                        &session,
                        &buffer,
                        &expected_revision,
                        before.clone(),
                    );
                    app::settle_conflict_write(guard, &report.result, before);
                    Some((report.result, report.recording))
                });
                cx.spawn_in(window, async move |this, acx| {
                    let (result, recording) =
                        task.fallible().await.flatten().unwrap_or_else(|| {
                            let error = abandonment.into_unknown();
                            let recording = conflict_unfinished_recording(
                                &op_name,
                                &repo_path,
                                StateSummary {
                                    head: format!("op={}", session_op),
                                    dirty: "resolving".into(),
                                },
                                &error,
                            );
                            (Err(error), recording)
                        });
                    let _ = this.update_in(acx, |app, window, cx| {
                        app.refresh_write_busy();
                        let current = app.active_session() == Some(owner)
                            && app.app_sessions.visit(owner) == visit;
                        if let Err(e) = &result {
                            klog!("refused: {} stage failed: {}", op_name, e);
                        }
                        // A successful handoff opens the composer, not another
                        // result surface. Its receipt still belongs in history.
                        app.present_operation_completion(
                            &recording,
                            current && result.is_err(),
                            false,
                            None,
                            cx,
                        );
                        if result.is_ok() {
                            klog!("{}: routing to commit message panel (merge)", op_name);
                        }
                        for (id, op, path) in app.app_sessions.drain_unaccounted() {
                            app.notice_reconcile_required(id, op, &path);
                        }
                        app.present_app_notice();
                        if !current {
                            cx.notify();
                            return;
                        }
                        if result.is_err() {
                            cx.notify();
                            return;
                        }
                        app.open_commit_panel(window, cx);
                        if let Some(entity) = app.ui().commit_panel.clone() {
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
                            entity.update(cx, |v, _| {
                                v.state.commit_msg = message.clone();
                                v.merge_comment_char = comment_char;
                            });
                        }
                        if let Some(conflict) = app.ui().conflict.clone() {
                            conflict.update(cx, |view, _| view.retire_raw_preview());
                        }
                        if let Some(ui) = app.ui_mut() {
                            ui.conflict_merge_pending = true;
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            kagi_git::ContinueRoute::SequencerPlan(plan) => {
                // Confirmation modal before advancing the sequencer.
                klog!("{}: opening continue confirmation (sequencer)", op_name);
                self.set_conflict_continue_modal(ConflictContinuePlanModal {
                    plan: std::sync::Arc::new(*plan),
                    revision: mode.revision,
                    operation_kind: mode.session.op.kind(),
                    error: None,
                });
            }
            kagi_git::ContinueRoute::StashPlan(plan) => {
                self.set_conflict_continue_modal(ConflictContinuePlanModal {
                    plan: std::sync::Arc::new(*plan),
                    revision: mode.revision,
                    operation_kind: mode.session.op.kind(),
                    error: None,
                });
            }
        }
        cx.notify();
    }

    /// Confirm the sequencer `<op> --continue` plan (T-CONFLICT-FLOW-032): run
    /// `execute_conflict_continue` (which stages the resolution and advances the
    /// sequencer), record the oplog, and reload. Keep the revision-bound autosave
    /// available for recovery even if the UI never adopts the next observation.
    pub fn confirm_conflict_continue(&mut self, cx: &mut Context<Self>) {
        let Some(owner) = self
            .active_session()
            .and_then(|id| self.app_sessions.attachment(id))
        else {
            return;
        };
        self.flush_conflict_drafts(owner, "continue", cx, |app, cx| {
            app.confirm_conflict_continue_flushed(cx)
        });
    }

    fn confirm_conflict_continue_flushed(&mut self, cx: &mut Context<Self>) {
        if !self
            .active_session()
            .is_some_and(|owner| self.pane_mutation_admitted(owner))
        {
            self.clear_conflict_continue_modal();
            cx.notify();
            return;
        }
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
                SharedString::from(i18n::op::op_failed_contract(
                    i18n::Op::RepoOpen,
                    "session unavailable",
                )),
                cx,
            );
            return;
        }
        let op_name = format!("{}-continue", modal.operation_kind.slug());

        let Some(guard) = self.reserve_write_for(
            "conflict-continue",
            &op_name,
            &repo_path,
            self.active_session(),
            cx,
        ) else {
            return;
        };
        let owner = self.active_session();
        let visit = owner.and_then(|id| self.app_sessions.visit(id));
        let session = mode.session;
        let is_stash = matches!(session.op, kagi_git::ConflictOp::StashConflict);
        let buffer = mode.buffer;
        let expected_revision = modal.revision;
        let before = plan.current.clone();
        let modal_plan = plan.clone();
        let abandonment = guard.abandonment();
        let supervision = abandonment.supervision();
        let bg_path = repo_path.clone();
        let bg_before = before.clone();
        #[cfg(feature = "gui-e2e")]
        let hold = CONTINUE_CONFIRM_HOLD.with(|slot| slot.borrow_mut().take());
        #[cfg(feature = "gui-e2e")]
        let completion_hold = CONTINUE_COMPLETION_HOLD.with(|slot| slot.borrow_mut().take());
        let task = cx.background_spawn(async move {
            let _supervised = kagi_git::proc::supervisor::enter(supervision);
            #[cfg(feature = "gui-e2e")]
            if let Some(hold) = hold {
                hold.await;
            }
            #[cfg(feature = "gui-e2e")]
            if PANIC_NEXT_CONTINUE_CONFIRM.swap(false, std::sync::atomic::Ordering::SeqCst) {
                let _ = std::panic::catch_unwind(|| panic!("injected sequencer continue panic"));
                return None;
            }
            #[cfg(feature = "gui-e2e")]
            if is_stash
                && PANIC_NEXT_CONTINUE_STASH.swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                let _ = std::panic::catch_unwind(|| panic!("injected stash continue panic"));
                return None;
            }
            let report = kagi_git::Backend::run_recorded_conflict_continue(
                &bg_path,
                &session,
                &buffer,
                &expected_revision,
                bg_before.clone(),
            );
            app::settle_conflict_write(guard, &report.result, bg_before);
            Some((report.result, report.recording))
        });
        cx.spawn(async move |this, acx| {
            let (result, recording) = task.fallible().await.flatten().unwrap_or_else(|| {
                let error = abandonment.into_unknown();
                let recording = conflict_unfinished_recording(&op_name, &repo_path, before, &error);
                (Err(error), recording)
            });
            #[cfg(feature = "gui-e2e")]
            if let Some((hold, reached)) = completion_hold {
                reached.store(true, std::sync::atomic::Ordering::Release);
                hold.await;
            }
            let _ = this.update(acx, |app, cx| {
                app.refresh_write_busy();
                let current = owner.is_some_and(|id| {
                    app.active_session() == Some(id) && app.app_sessions.visit(id) == visit
                });
                let modal_matches = current
                    && app
                        .conflict_continue_modal()
                        .is_some_and(|modal| std::sync::Arc::ptr_eq(&modal.plan, &modal_plan));
                match result {
                    Ok(result) => {
                        klog!("executed: {}", op_name);
                        let stopped = !is_stash
                            && matches!(&result.outcome, kagi_git::ContinueOutcome::Staged);
                        app.present_operation_completion(&recording, current, stopped, None, cx);
                        if modal_matches {
                            app.clear_conflict_continue_modal();
                        }
                        if current {
                            if is_stash {
                                if let Some(owner) = owner {
                                    app.app_sessions.continue_stash_conflict(owner);
                                }
                            }
                            app.reload(cx);
                            if let Some(ui) = app.ui_mut() {
                                ui.conflict_detected = false;
                            }
                            app.detect_conflict_mode(cx);
                        }
                    }
                    Err(e) => {
                        let err_msg = format!("{}", e);
                        let unknown_evidence = match &recording.entry().outcome {
                            OpOutcome::Unknown { evidence, .. } => Some(evidence.clone()),
                            _ => None,
                        };
                        klog!("{} failed: {}", op_name, err_msg);
                        app.present_operation_completion(&recording, current, false, None, cx);
                        if modal_matches {
                            // This admitted request is terminal. Retire only
                            // its frozen confirmation so its result/required
                            // inspection can use the existing presentation slot.
                            app.clear_conflict_continue_modal();
                            if let Some(evidence) = unknown_evidence {
                                app.report_unknown_notice(&repo_path, evidence);
                            }
                        }
                    }
                }
                for (id, op, path) in app.app_sessions.drain_unaccounted() {
                    app.notice_reconcile_required(id, op, &path);
                }
                app.present_app_notice();
                cx.notify();
            });
        })
        .detach();
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
                SharedString::from(i18n::op::op_failed_contract(i18n::Op::ExternalTool, e)),
                cx,
            ),
        }
    }

    /// Open the integrated terminal at the repository root (ADR-0060 / T-051).
    pub fn conflict_open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_settings_for_command(window, cx);
        self.bottom_panel_open = true;
        self.bottom_tab = BottomTab::Terminal;
        self.activate_terminal(window, cx);
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
