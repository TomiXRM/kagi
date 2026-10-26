//! Sole runtime adapter for the first application family.
use crate::app::{self, Approved, Delivery};
use crate::ui::*;
use kagi_domain::plan_note::{PlanNote, WorktreeNote};

fn log_stash_event(
    event: kagi_git::backend::stash::StashEvent,
    name: &str,
    started: std::time::Instant,
    executed: &mut std::time::Instant,
) {
    use kagi_git::backend::stash::{StashAction, StashEvent};
    match event {
        StashEvent::Started => {
            if name != "stash-apply" {
                klog!("async: {} started", name);
            }
        }
        StashEvent::PlanBlocked => {
            let label = match name {
                "stash-pop" => "pop",
                "stash-drop" => "drop",
                other => other,
            };
            klog!("refused: {} plan has blockers, not executing", label);
        }
        StashEvent::VerifyFailed { error } => match name {
            "stash-apply" => klog!("verify: snapshot error: {}", error),
            "stash-push" => klog!(
                "async: stash-push timing stash={:.1}s verify={:.1}s",
                executed.duration_since(started).as_secs_f32(),
                executed.elapsed().as_secs_f32()
            ),
            _ => {}
        },
        StashEvent::Executed {
            action,
            oid,
            conflicts,
        } => {
            *executed = std::time::Instant::now();
            match action {
                StashAction::Push { message, .. } => klog!(
                    "executed: stash-push message={:?}",
                    message.unwrap_or_default()
                ),
                StashAction::Apply { index } => klog!("executed: stash-apply index={}", index),
                StashAction::Pop { index } => {
                    klog!("executed: stash-pop index={}", index);
                    if conflicts > 0 {
                        klog!(
                            "executed: stash-pop index={} — conflicts in {} file(s), stash kept",
                            index,
                            conflicts
                        );
                    }
                }
                StashAction::Drop { index } => klog!(
                    "executed: stash-drop index={} oid={}",
                    index,
                    oid.unwrap_or_default()
                ),
            }
        }
        StashEvent::Verified { dirty, count } => match name {
            "stash-push" => {
                if dirty {
                    klog!("verify: working tree NOT clean after stash-push");
                } else {
                    klog!("verified: working tree clean after stash-push");
                }
                klog!("verified: stash count={}", count);
                klog!(
                    "async: stash-push timing stash={:.1}s verify={:.1}s",
                    executed.duration_since(started).as_secs_f32(),
                    executed.elapsed().as_secs_f32()
                );
            }
            "stash-apply" => {
                if dirty {
                    klog!("verified: working tree dirty (stash applied)");
                } else {
                    klog!("verify: working tree NOT dirty after stash-apply");
                }
                klog!("verified: stash count={} (entry preserved)", count);
            }
            _ => {}
        },
    }
}

mod notices;

impl KagiApp {
    /// Preserve the `[kagi] footer:` contract while human-facing text can
    /// name a typed refusal, distinguish an Unknown outcome, or show a short
    /// failed-plan preview. The durable Failed receipt retains the full error.
    pub(crate) fn display_footer_message(
        op: &str,
        outcome: &OpOutcome,
        _contract: &SharedString,
        display_override: Option<String>,
    ) -> SharedString {
        if matches!(
            outcome,
            OpOutcome::Refused { .. } | OpOutcome::Failed { .. }
        ) {
            if let Some(display) =
                display_override.or_else(|| super::record::refused_display(op, outcome))
            {
                return i18n::op::user_text::operations::failure_preview(op, &display).into();
            }
        }
        super::outcome_text(op, outcome).into()
    }

    fn deliver_stash_result(
        &mut self,
        id: app::OperationId,
        owner: app::Attachment,
        report: kagi_git::backend::stash::StashReport,
        cx: &mut Context<Self>,
    ) {
        let entry = report.recording.entry();
        let name = report.action.name();
        let success = matches!(entry.outcome, OpOutcome::Success { .. });
        let partial = matches!(entry.outcome, OpOutcome::Partial { .. });
        // A stash conflict already has a full-screen recovery surface. Showing
        // the same Partial as an AppNotice would keep the one modal slot occupied
        // throughout Conflict Mode and strand the post-continue drop prompt.
        let conflict_partial = partial && !report.evidence.conflicts.is_empty();
        let summary = oplog_panel::outcome_summary_contract(&entry.outcome);
        if report.evidence.stop == Some(kagi_git::backend::stash::StashStopReason::Abandoned)
            && name != "stash-apply"
        {
            klog!("async: {} started", name);
        }
        if name != "stash-apply" && !report.evidence.plan_blocked {
            if success || conflict_partial {
                klog!("async: {} finished", name);
            } else {
                klog!("async: {} failed — {}", name, summary);
            }
        }
        if let Some(panel) = &self.op_log {
            panel.update(cx, |panel, cx| {
                panel.push(oplog_panel::OpLogPanel::entry_for_recording(
                    &report.recording,
                ));
                cx.notify();
            });
        }
        let footer = if !report.evidence.conflicts.is_empty() {
            format!(
                "{}: {}",
                i18n::op::user_text::operation_label(name),
                Msg::StashPopConflictedKept.t()
            )
        } else {
            super::outcome_text(name, &entry.outcome)
        };
        let recording_failed = matches!(
            report.recording,
            kagi_git::backend::recording::Recording::Failed { .. }
        );
        let current = self.active_session() == Some(owner.session)
            && self.app_sessions.visit(owner.session) == Some(owner.visit);
        let announce = !current || self.operation_result_toast_needed(false);
        if announce && !recording_failed {
            let _ = self.push_operation_result(
                entry,
                name,
                if success {
                    ToastKind::Success
                } else {
                    ToastKind::Error
                },
                footer.clone().into(),
                cx,
            );
        }
        if current && (announce || recording_failed) {
            self.status_footer = FooterStatus::Idle("".into());
        }
        if current && !announce && !recording_failed {
            self.status_footer = if success {
                FooterStatus::Success(crate::ui::operations::display_text(footer.clone().into()))
            } else if partial {
                FooterStatus::Idle(footer.clone().into())
            } else {
                FooterStatus::Failed(crate::ui::operations::display_text(footer.clone().into()))
            };
        }
        if !success && !conflict_partial && report.evidence.unknown {
            let mut notice = modals::AppNotice::from(format!("{}: {}", entry.repo, footer));
            notice.inspect = Some(id);
            self.app_notices.push_back(notice);
        }
        if let kagi_git::backend::recording::Recording::Failed { error, .. } = &report.recording {
            self.app_notices.push_back(
                super::record::recording_failure_notice(
                    &entry.repo,
                    &entry.op,
                    &report.recording.entry().outcome,
                    &error.to_string(),
                    None,
                )
                .into(),
            );
        }
    }
    /// Reserve and mirror in the same UI turn, before any writer dispatch.
    pub(crate) fn reserve_write(
        &mut self,
        name: &'static str,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> Option<app::WriteGuard> {
        let owner = self.active_session();
        self.reserve_write_for(name, name, path, owner, cx)
    }

    /// [`Self::reserve_write`] for a writer the operation queue must not
    /// treat as the tab's own predecessor: `owner: None` is background work
    /// (auto-fetch), which only blocks every tab's queue until it ends.
    pub(crate) fn reserve_write_for(
        &mut self,
        name: &'static str,
        refusal_name: &str,
        path: &std::path::Path,
        owner: Option<app::SessionId>,
        cx: &mut Context<Self>,
    ) -> Option<app::WriteGuard> {
        self.refresh_write_busy();
        self.sync_queue_before_admission(cx);
        // The lease answers for every other writer; the UI latch is what a
        // planning task in flight is refused by (ADR-0196 Wave 3).
        let admitted = if self.op_latched() {
            Err(app::AdmissionError::Busy)
        } else {
            self.app_sessions.write_lease(path)
        };
        match app::admit(&mut self.reads, admitted) {
            Ok(guard) => {
                self.clear_repository_busy_footer(cx);
                self.mark_write_busy(name, cx);
                // A guard writer has no receipt the `&&` chain can judge.
                self.queue_untracked_write(owner, cx);
                // #702: the guard names its op, so a retained lease parks an
                // entry that can say which write it is holding for.
                Some(guard.for_op(name))
            }
            Err(error) => {
                if name == "fetch"
                    && owner.is_none()
                    && error == app::AdmissionError::RepositoryBusy
                {
                    self.status_footer =
                        FooterStatus::Idle(Msg::AdviceConflictsRepositoryBusy.t().into());
                    klog!("auto-fetch: skipped (repository busy)");
                    cx.notify();
                    return None;
                }
                if name == "fetch" && owner.is_none() {
                    self.report_admission_refusal(error, cx);
                } else {
                    self.report_write_admission_refusal(refusal_name, path, error, cx);
                }
                None
            }
        }
    }

    /// Admission supersedes only the repository-contention claim, including
    /// a footer left in the previous language after a locale switch.
    pub(super) fn clear_repository_busy_footer(&mut self, cx: &mut Context<Self>) {
        let busy = match &self.status_footer {
            FooterStatus::Idle(message) | FooterStatus::Failed(message) => {
                i18n::op::user_text::operations::is_repository_busy_message(message)
            }
            _ => false,
        };
        if busy {
            self.status_footer = FooterStatus::Idle("".into());
            cx.notify();
        }
    }
    pub(crate) fn refresh_write_busy(&mut self) {
        let has_leases = self.app_sessions.has_leases();
        super::super::busy::settle_write_busy(&mut self.write_busy_op, has_leases);
        self.retire_hidden_busy_progress();
    }
    pub(crate) fn dispatch_job(&mut self, approved: Approved, cx: &mut Context<Self>) {
        let (name, label) = match &approved.prepared {
            app::Planned::Remove { .. } => ("remove-worktree", Msg::BusyRemoveWorktree),
            app::Planned::Stash { plan, .. } => (
                plan.action.name(),
                match plan.action {
                    app::StashAction::Drop { .. } => Msg::BusyStashDrop,
                    app::StashAction::Pop { .. } => Msg::BusyStashPop,
                    _ => Msg::BusyStash,
                },
            ),
            app::Planned::RemoteStash { .. } => ("remote-stash-drop", Msg::BusyStashDrop),
            app::Planned::RemotePull { .. } => ("pull", Msg::BusyPull),
            // Run-family writes dispatch through `finish_run`; `prepare` refuses
            // this one below and the refusal is presented like any other.
            app::Planned::Run(request) => (request.name, Msg::OpInProgress),
            // Same: the pull workflow dispatches through `finish_pull`.
            app::Planned::Pull(request) => (request.name, Msg::BusyPull),
            app::Planned::Conflict { plan, .. } => match plan.request().action() {
                kagi_domain::conflict_family::ConflictAction::Save => {
                    ("conflict-save", Msg::OpInProgress)
                }
                kagi_domain::conflict_family::ConflictAction::ResolveDirFile(
                    kagi_git::DirFileChoice::KeepDirectory,
                ) => ("conflict-dir-file:keep-directory", Msg::OpInProgress),
                kagi_domain::conflict_family::ConflictAction::ResolveDirFile(
                    kagi_git::DirFileChoice::KeepFile,
                ) => ("conflict-dir-file:keep-file", Msg::OpInProgress),
                kagi_domain::conflict_family::ConflictAction::Abort => {
                    ("conflict-abort", Msg::OpInProgress)
                }
            },
        };
        let refusal_owner = approved.prepared.owner();
        let conflict_plan = match &approved.prepared {
            app::Planned::Conflict { plan, .. } => Some(plan.clone()),
            _ => None,
        };
        let admitted = if self.op_latched() {
            Err(app::AdmissionError::Busy)
        } else {
            app::prepare(&mut self.app_sessions, approved)
        };
        let job = match app::admit(&mut self.reads, admitted) {
            Ok(job) => job,
            Err(error) => {
                if let app::OwnerAttachment::Local(owner) = refusal_owner {
                    let operation = conflict_plan
                        .as_ref()
                        .map_or(name, |plan| plan.operation_name());
                    self.report_write_admission_refusal(operation, &owner.path, error, cx);
                } else {
                    self.report_admission_refusal(error, cx);
                }
                return;
            }
        };
        self.mark_write_busy(name, cx);
        // Stash / remove / conflict jobs have no verify path the `&&` chain
        // could judge (ADR-0204 決定 4): untracked.
        let owner = self.active_session();
        self.queue_untracked_write(owner, cx);
        if name.starts_with("conflict-") {
            if let Some(conflict) = self.ui().conflict.clone() {
                conflict.update(cx, |view, cx| {
                    view.writer_busy = true;
                    cx.notify();
                });
            }
        }
        match name {
            "remove-worktree" => self.clear_remove_worktree_modal(),
            "stash-push" => self.clear_stash_push_modal(),
            "stash-apply" => self.clear_stash_apply_modal(),
            "stash-pop" => self.clear_pop_modal(),
            "stash-drop" => self.clear_stash_drop_modal(),
            "remote-stash-drop" => self.clear_stash_drop_modal(),
            "pull" => self.clear_pull_modal(),
            "conflict-save"
            | "conflict-dir-file:keep-directory"
            | "conflict-dir-file:keep-file" => {}
            "conflict-abort" => self.clear_conflict_abort_modal(),
            _ => unreachable!(),
        }
        self.status_footer = FooterStatus::Busy(crate::ui::operations::display_text(
            SharedString::from(label.t()),
        ));
        if name == "remove-worktree" {
            klog!("async: remove-worktree started");
        } else if name == "remote-stash-drop" {
            klog!("async: remote stash-drop started");
        } else if name == "pull" {
            klog!("async: remote pull started");
        }
        #[cfg(feature = "gui-e2e")]
        let hold = (name == "pull")
            .then(crate::ui::op_queue::take_run_hold)
            .flatten();
        let task = cx.background_spawn(async move {
            #[cfg(feature = "gui-e2e")]
            if let Some(hold) = hold {
                hold.await;
            }
            let started = std::time::Instant::now();
            let mut executed = started;
            job.run_with_events(|event| {
                use kagi_git::backend::remove::RemoveEvent;
                match event {
                    app::Event::Remove(RemoveEvent::ConfigTrusted) => {
                        klog!("worktree: trusted .kagi/worktree.toml (pre_remove)")
                    }
                    app::Event::Remove(RemoveEvent::ExecutionStarting) => {}
                    app::Event::Remove(RemoveEvent::ConfigRefused) => {
                        klog!("refused: remove-worktree config changed after plan, not executing")
                    }
                    app::Event::Stash(event) => {
                        log_stash_event(event, name, started, &mut executed);
                    }
                }
            })
        });
        cx.spawn(async move |this, cx| {
            let completion = task.await;
            let _ = this.update(cx, |app, cx| {
                let deliveries = app::apply(&mut app.app_sessions, completion);
                app.refresh_write_busy();
                if name.starts_with("conflict-") {
                    if let Some(conflict) = app.ui().conflict.clone() {
                        conflict.update(cx, |view, cx| {
                            view.writer_busy = false;
                            cx.notify();
                        });
                    }
                }
                for delivery in deliveries {
                    app.deliver_app_result(delivery, cx);
                }
                app.present_app_notice();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(crate) fn deliver_app_result(&mut self, delivery: Delivery, cx: &mut Context<Self>) {
        match delivery {
            Delivery::RemovedTarget(target) => {
                // #528: the worktree no longer exists, so neither should its
                // tab. `close_tab` keeps the existing dirty guard and only
                // re-activates a neighbour when this tab was the active one.
                // #482: found by `WorktreeId`, so a tab opened on the same path
                // after the plan is never the one that closes — and every tab
                // aliasing that worktree closes, not an arbitrary one.
                for session in self.app_sessions.sessions_for(&target.worktree) {
                    self.close_tab_by_session(session, cx);
                }
            }
            Delivery::Invalidate(target) => {
                // #482 stage 2: the mutation landed, so every read of this
                // worktree that predates it is stale — including one still in
                // flight, which is refused rather than allowed to overwrite the
                // reload below. The last good read stays on screen meanwhile.
                let owners = self.app_sessions.sessions_for(&target.worktree);
                for session in &owners {
                    self.reads.invalidate(*session);
                }
                if self.active_session().is_some_and(|a| owners.contains(&a)) {
                    self.reload(cx);
                }
            }
            Delivery::Completed {
                id,
                attachment,
                report,
                ..
            } => {
                let report = match report.evidence {
                    app::FamilyEvidence::Remove(report) => report,
                    app::FamilyEvidence::Stash(report) => {
                        self.deliver_stash_result(id, attachment, report, cx);
                        return;
                    }
                    app::FamilyEvidence::RemoteStash(_) | app::FamilyEvidence::RemotePull(_) => {
                        return
                    }
                    // Presented by the `finish_run` that admitted it; a
                    // completion abandoned by its window has no one to show.
                    app::FamilyEvidence::Run(_) | app::FamilyEvidence::Pull(_) => return,
                    app::FamilyEvidence::Conflict(report) => {
                        self.deliver_conflict_result(attachment, report, cx);
                        return;
                    }
                };
                let entry = report.recording.entry();
                let summary = oplog_panel::outcome_summary_contract(&entry.outcome);
                let success = matches!(entry.outcome, OpOutcome::Success { .. });
                let footer = match &entry.outcome {
                    OpOutcome::Success { after } => {
                        format!("remove-worktree: {} → {}", entry.before.head, after.head)
                    }
                    OpOutcome::Partial { error, .. } => {
                        format!("remove-worktree: partially applied — {}", error)
                    }
                    OpOutcome::Failed { error } => format!("remove-worktree: failed — {}", error),
                    OpOutcome::Refused { blockers } => {
                        format!("remove-worktree: refused — {}", blockers.join("; "))
                    }
                    OpOutcome::Unknown { evidence, .. } => {
                        format!("remove-worktree: result unknown — {}", evidence)
                    }
                };
                // A typed post-step blocker is the entire toast reason; the
                // Partial receipt and footer retain the full stage evidence.
                let concise_note = report.blocker.as_ref().filter(|note| {
                    matches!(entry.outcome, OpOutcome::Partial { .. })
                        || matches!(
                            note,
                            PlanNote::Worktree(WorktreeNote::RemoveIgnoredContentChanged)
                        )
                });
                let text = if let Some(note) = concise_note {
                    i18n::plan_note_text(note)
                } else if matches!(entry.outcome, OpOutcome::Refused { .. }) {
                    report.blocker.as_ref().map_or_else(
                        || super::outcome_text("remove-worktree", &entry.outcome),
                        |blocker| format!("{}: {}", entry.repo, i18n::plan_note_text(blocker)),
                    )
                } else {
                    format!(
                        "{}: {}",
                        entry.repo,
                        super::outcome_text("remove-worktree", &entry.outcome)
                    )
                };
                let recording_failed = matches!(
                    report.recording,
                    kagi_git::backend::remove::Recording::Failed { .. }
                );
                match &entry.outcome {
                    OpOutcome::Success { .. } => klog!(
                        "executed: remove-worktree {} (backups={})",
                        report.name,
                        report.progress.backups.len()
                    ),
                    OpOutcome::Partial { error, .. } => {
                        klog!("async: remove-worktree partial — {}", error)
                    }
                    OpOutcome::Refused { blockers } => {
                        klog!("async: remove-worktree failed — {}", blockers.join("; "))
                    }
                    _ => klog!("async: remove-worktree failed — {}", summary),
                }
                if let Some(panel) = self.op_log.clone() {
                    panel.update(cx, |panel, cx| {
                        panel.push(oplog_panel::OpLogPanel::entry_for_recording(
                            &report.recording,
                        ));
                        cx.notify();
                    });
                }
                if !recording_failed
                    && concise_note.is_none()
                    && report.blocker.is_some()
                    && matches!(report.recording.entry().outcome, OpOutcome::Refused { .. })
                {
                    self.app_notices.push_back(text.clone().into());
                }
                if !recording_failed {
                    self.push_toast(
                        if success {
                            ToastKind::Success
                        } else {
                            ToastKind::Error
                        },
                        text.clone(),
                        cx,
                    );
                }
                if self.active_session() == Some(attachment.session) {
                    klog!("footer: {}", footer);
                    self.status_footer = if recording_failed {
                        FooterStatus::Idle("".into())
                    } else {
                        let display = super::outcome_text(
                            "remove-worktree",
                            &report.recording.entry().outcome,
                        );
                        if success {
                            FooterStatus::Success(display.into())
                        } else {
                            FooterStatus::Failed(display.into())
                        }
                    };
                }
                if !success
                    && matches!(report.recording.entry().outcome, OpOutcome::Unknown { .. })
                    && !report.progress.termination_unknown
                {
                    let mut notice = modals::AppNotice::from(text);
                    notice.inspect = Some(id);
                    self.app_notices.push_back(notice);
                }
                if let kagi_git::backend::remove::Recording::Failed { error, .. } =
                    &report.recording
                {
                    self.app_notices.push_back(
                        super::record::recording_failure_notice(
                            attachment.path.display(),
                            "remove-worktree",
                            &report.recording.entry().outcome,
                            &error.to_string(),
                            None,
                        )
                        .into(),
                    );
                }
            }
            Delivery::RemoteCompleted {
                id,
                attachment,
                stamp,
                report,
            } => match report.evidence {
                app::FamilyEvidence::RemoteStash(report) => {
                    self.deliver_remote_stash_result(id, attachment, stamp, report, cx);
                }
                app::FamilyEvidence::RemotePull(report) => {
                    self.deliver_remote_pull_result(id, attachment, stamp, report, cx);
                }
                _ => unreachable!("remote completion belongs to a remote family"),
            },
        }
    }
    fn deliver_remote_stash_result(
        &mut self,
        id: app::OperationId,
        owner: crate::remote::stash::RemoteAttachment,
        stamp: app::OwnerStamp,
        report: crate::remote::stash::RemoteStashReport,
        cx: &mut Context<Self>,
    ) {
        let entry = report.recording.entry();
        let summary = if matches!(entry.outcome, OpOutcome::Unknown { .. }) {
            i18n::op::remote_completion_pending_contract(i18n::lang()).to_string()
        } else {
            oplog_panel::outcome_summary_contract(&entry.outcome)
        };
        let success = matches!(entry.outcome, OpOutcome::Success { .. });
        if success {
            klog!("async: remote stash-drop finished");
        } else {
            klog!("async: remote stash-drop failed — {}", summary);
        }
        if let Some(panel) = &self.op_log {
            panel.update(cx, |panel, cx| {
                panel.push(oplog_panel::OpLogPanel::entry_for_recording(
                    &report.recording,
                ));
                cx.notify();
            });
        }
        let active = self.remote_view.as_ref().is_some_and(|view| {
            view.host == owner.host
                && view.root == owner.root
                && self.active_session() == Some(owner.session)
                && self.app_sessions.visit(owner.session) == Some(stamp.visit)
        });
        let recording_failed = matches!(
            report.recording,
            kagi_git::backend::recording::Recording::Failed { .. }
        );
        // Explicit SSH operations use the existing long-running result lane:
        // their following snapshot owns Busy / Remote footer presentation.
        if !recording_failed {
            // #353: a refused remote drop names its diagnostic, not a count.
            let shown = super::outcome_text("stash-drop", &entry.outcome);
            let _ = self.push_operation_result(
                entry,
                "stash-drop",
                if success {
                    ToastKind::Success
                } else {
                    ToastKind::Error
                },
                shown.into(),
                cx,
            );
        }
        if active {
            self.status_footer = FooterStatus::Idle("".into());
            if success {
                #[cfg(feature = "gui-e2e")]
                let intercepted = crate::remote::stash::note_remote_stash_e2e_refresh();
                #[cfg(not(feature = "gui-e2e"))]
                let intercepted = false;
                if !intercepted {
                    self.refresh_remote_view(cx);
                }
            }
        }
        if !success && matches!(entry.outcome, OpOutcome::Unknown { .. }) {
            let mut notice = modals::AppNotice::from(format!(
                "{}: {}",
                entry.repo,
                super::outcome_text("stash-drop", &entry.outcome)
            ));
            notice.inspect = Some(id);
            self.app_notices.push_back(notice);
        }
        if let kagi_git::backend::recording::Recording::Failed { error, .. } = &report.recording {
            self.app_notices.push_back(
                super::record::recording_failure_notice(
                    &entry.repo,
                    &entry.op,
                    &report.recording.entry().outcome,
                    &error.to_string(),
                    None,
                )
                .into(),
            );
        }
    }
    fn deliver_remote_pull_result(
        &mut self,
        id: app::OperationId,
        owner: crate::remote::stash::RemoteAttachment,
        stamp: app::OwnerStamp,
        report: crate::remote::RemotePullReport,
        cx: &mut Context<Self>,
    ) {
        let path = std::path::PathBuf::from(format!("{}:{}", owner.host.label(), owner.root));
        self.notice_recording_failure("pull", &report.recording, &path, None);
        if matches!(
            report.recording.entry().outcome,
            OpOutcome::Unknown { .. } | OpOutcome::Partial { .. }
        ) {
            self.notice_reconcile_required(id, "pull", &path);
        }
        let active = self.remote_view.as_ref().is_some_and(|view| {
            view.host == owner.host
                && view.root == owner.root
                && self.active_session() == Some(owner.session)
                && self.app_sessions.visit(owner.session) == Some(stamp.visit)
        });
        if !active {
            klog!("op result dropped: tab switched during op");
        }
        let recording_failed = matches!(
            report.recording,
            kagi_git::backend::recording::Recording::Failed { .. }
        );
        let (notice, failure_message) = match &report.result {
            Ok(summary) => {
                if active {
                    klog!("async: remote pull finished — {summary}");
                }
                let notice = (!recording_failed
                    && matches!(report.recording.entry().outcome, OpOutcome::Success { .. }))
                .then(|| {
                    (
                        ToastKind::Success,
                        i18n::op::user_text::operations::pull_summary(summary).into(),
                    )
                });
                (notice, None)
            }
            Err(error) => {
                let err_msg = error.to_string();
                if active {
                    klog!("async: remote pull failed — {err_msg}");
                }
                (None, Some(err_msg))
            }
        };
        let entry = oplog_panel::OpLogPanel::entry_for_recording(&report.recording);
        // Keep the final SSH result independent of its ordinary refresh footer.
        let _ = self.record_op_result(
            entry,
            cx,
            false,
            None,
            notice,
            true,
            active,
            None,
            recording_failed,
            None,
            None,
        );
        if active {
            self.refresh_remote_view(cx);
        }
        if let Some(err_msg) = failure_message {
            self.enqueue_run_outcome_notice(
                app::OperationId(report.recording.entry().id),
                &report.recording,
                Some(&err_msg),
                None,
            );
        }
    }
    /// Whether this window may close (or the app quit) now: no write holds a
    /// lease and no clone is running. A clone has no repository session, so
    /// it is its own term — closing mid-clone would leave `gh` / `git`
    /// writing the folder with no verify and no receipt; the close waits
    /// until `finish_clone` has the recorded result (#930 review).
    pub fn may_close_host(&self) -> bool {
        self.app_sessions.may_close_host() && self.home_github.cloning.is_none()
    }
    pub(crate) fn hold_host_operation(&mut self, cx: &mut Context<Self>) -> bool {
        if self.may_close_host() {
            return false;
        }
        let already_presented = self
            .app_notice()
            .is_some_and(|notice| notice.host_close_guard)
            || self
                .app_notices
                .iter()
                .any(|notice| notice.host_close_guard);
        if !already_presented {
            let mut notice = modals::AppNotice::from(Msg::OpInProgress.t().to_string());
            notice.host_close_guard = true;
            self.app_notices.push_back(notice);
        }
        self.present_app_notice();
        cx.notify();
        true
    }
    /// #510: a `plan_*` that failed before it could open a modal must still
    /// reach the user — footer plus the shared app-notice modal, never stderr
    /// alone. The `[kagi]` plan-error line the caller already emitted is a test
    /// contract and stays exactly as it was.
    pub(crate) fn report_plan_failure(&mut self, op: i18n::Op, error: impl std::fmt::Display) {
        let message = i18n::op_plan_failed(op, error);
        self.status_footer = FooterStatus::Failed(crate::ui::operations::display_text(
            SharedString::from(message.clone()),
        ));
        self.app_notices.push_back(message.into());
        self.present_app_notice();
    }
    pub(crate) fn report_unknown_notice(&mut self, repo: &Path, evidence: impl Into<String>) {
        self.app_notices.push_back(modals::AppNotice::from(format!(
            "{}: {}",
            repo.display(),
            i18n::op::user_text::diagnostic(&evidence.into())
        )));
        self.present_app_notice();
    }
    pub(crate) fn present_oplog_write_failure(
        &mut self,
        entry: &OpLogEntry,
        error: impl std::fmt::Display,
        stopped_notice: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let message = super::record::recording_failure_notice(
            &entry.repo,
            &entry.op,
            &entry.outcome,
            &error.to_string(),
            stopped_notice,
        );
        self.app_notices.push_back(message.into());
        self.present_app_notice();
        cx.notify();
    }
    pub(crate) fn present_app_notice(&mut self) {
        // Only this transient refusal expires. Receipts and reconcile notices
        // retain their normal owner/action, even when the final writer exits.
        if self.may_close_host() {
            if self
                .app_notice()
                .is_some_and(|notice| notice.host_close_guard)
            {
                self.clear_app_notice();
            }
            self.app_notices.retain(|notice| !notice.host_close_guard);
        }
        if self.has_active_modal() {
            return;
        }
        if let Some(message) = self.app_notices.pop_front() {
            self.set_app_notice(message);
        }
    }
    pub(crate) fn poll_app_jobs(&mut self, cx: &mut Context<Self>) {
        self.refresh_write_busy();
        // A guarded write whose process group is unaccounted for parked its
        // requirement from wherever it ran; this is where it becomes something
        // the user can open (#702 review 6). Same notice as `finish_run`'s.
        for (id, op, path) in self.app_sessions.drain_unaccounted() {
            self.notice_reconcile_required(id, op, &path);
        }
        for delivery in self.app_sessions.drain_abandoned() {
            self.deliver_app_result(delivery, cx);
        }
        self.refresh_write_busy();
        // The reload-time attempt may be deferred by an active modal. Retry
        // before queued notices are presented so the fresh drop confirmation
        // is not stranded after its conflict has been continued.
        self.present_stash_followup(cx);
        self.retire_busy_control_focus(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_human_message_does_not_replace_the_contract_footer() {
        let contract: SharedString =
            "rebase-skip: partially applied — process state unavailable".into();
        let outcome = OpOutcome::Unknown {
            after: StateSummary {
                head: "unknown".into(),
                dirty: "unknown".into(),
            },
            evidence: "termination unconfirmed — do not retry".into(),
        };

        let display = KagiApp::display_footer_message("rebase-skip", &outcome, &contract, None);

        assert_eq!(
            contract.as_ref(),
            "rebase-skip: partially applied — process state unavailable"
        );
        let OpOutcome::Unknown { evidence, .. } = &outcome else {
            unreachable!()
        };
        assert!(
            !display.contains(evidence),
            "raw evidence remains in the receipt"
        );
        assert!(display.contains("retry") || display.contains("再試行"));
        assert_ne!(display, contract);
        assert!(!display.contains("rebase-skip"));
    }
}
