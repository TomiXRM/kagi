//! Sole runtime adapter for the first application family.
use crate::app::{self, Approved, Delivery, LegacyBusy};
use crate::ui::*;

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

impl KagiApp {
    fn deliver_stash_result(
        &mut self,
        id: app::OperationId,
        owner: app::Attachment,
        report: kagi_git::backend::stash::StashReport,
        cx: &mut Context<Self>,
    ) {
        let entry = report.recording.entry().clone();
        let name = report.action.name();
        let success = matches!(entry.outcome, OpOutcome::Success { .. });
        let partial = matches!(entry.outcome, OpOutcome::Partial { .. });
        // A stash conflict already has a full-screen recovery surface. Showing
        // the same Partial as an AppNotice would keep the one modal slot occupied
        // throughout Conflict Mode and strand the post-continue drop prompt.
        let conflict_partial = partial && !report.evidence.conflicts.is_empty();
        let summary = oplog_panel::outcome_summary(&entry.outcome);
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
                panel.push(entry.clone());
                cx.notify();
            });
        }
        let footer = if let Some(error) = &report.evidence.preflight_error {
            i18n::op_failed(i18n::Op::Preflight, error)
        } else if !report.evidence.conflicts.is_empty() {
            format!("{}: {}", name, Msg::StashPopConflictedKept.t())
        } else {
            format!("{}: {}", name, summary)
        };
        let recording_failed = matches!(
            report.recording,
            kagi_git::backend::recording::Recording::Failed { .. }
        );
        self.push_toast(
            if success && !recording_failed {
                ToastKind::Success
            } else {
                ToastKind::Error
            },
            format!("{}: {}", entry.repo, footer),
            cx,
        );
        if self.active_session() == Some(owner.session) {
            self.status_footer = if success && !recording_failed {
                FooterStatus::Success(footer.clone().into())
            } else if partial {
                FooterStatus::Idle(footer.clone().into())
            } else {
                FooterStatus::Failed(footer.clone().into())
            };
        }
        if !success && !conflict_partial {
            let mut notice = modals::AppNotice::from(format!("{}: {}", entry.repo, footer));
            if report.evidence.unknown {
                notice.inspect = Some(id);
            }
            self.app_notices.push_back(notice);
        }
        if let kagi_git::backend::recording::Recording::Failed { error, .. } = report.recording {
            self.app_notices
                .push_back(format!("{}: recording failed: {}", entry.repo, error).into());
        }
    }
    /// Reserve and mirror in the same UI turn, before any writer dispatch.
    pub(crate) fn reserve_write(
        &mut self,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> Option<app::WriteGuard> {
        self.refresh_write_busy();
        match self
            .app_sessions
            .write_lease(path, LegacyBusy(self.busy_op.is_some()))
        {
            Ok(guard) => {
                self.busy_op = Some("app-writer");
                Some(guard)
            }
            Err(error) => {
                let message = if error == app::AdmissionError::Busy {
                    Msg::OpInProgress.t().to_string()
                } else {
                    error.to_string()
                };
                self.status_footer = FooterStatus::Failed(message.clone().into());
                self.push_toast(ToastKind::Error, message.clone(), cx);
                self.app_notices.push_back(message.into());
                cx.notify();
                None
            }
        }
    }
    pub(crate) fn refresh_write_busy(&mut self) {
        if self.busy_op == Some("app-writer") && !self.app_sessions.has_leases() {
            self.busy_op = None;
        }
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
        };
        let job = match app::prepare(
            &mut self.app_sessions,
            approved,
            LegacyBusy(self.busy_op.is_some()),
        ) {
            Ok(job) => job,
            Err(error) => {
                self.app_notices.push_back(error.to_string().into());
                cx.notify();
                return;
            }
        };
        let busy = if name == "remove-worktree" {
            name
        } else {
            "app-writer"
        };
        self.busy_op = Some(busy);
        match name {
            "remove-worktree" => self.clear_remove_worktree_modal(),
            "stash-push" => self.clear_stash_push_modal(),
            "stash-apply" => self.clear_stash_apply_modal(),
            "stash-pop" => self.clear_pop_modal(),
            "stash-drop" => self.clear_stash_drop_modal(),
            _ => unreachable!(),
        }
        self.status_footer = FooterStatus::Busy(SharedString::from(label.t()));
        if name == "remove-worktree" {
            klog!("async: remove-worktree started");
        }
        let task = cx.background_spawn(async move {
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
                if !app.app_sessions.has_leases() && app.busy_op == Some(busy) {
                    app.busy_op = None;
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
    fn deliver_app_result(&mut self, delivery: Delivery, cx: &mut Context<Self>) {
        match delivery {
            Delivery::RemovedTarget(target) => {
                self.tab_cache.remove(&target.path);
                // #528: the worktree no longer exists, so neither should its
                // tab. `close_tab` keeps the existing dirty guard and only
                // re-activates a neighbour when this tab was the active one.
                // #482 stage 1: found by frozen `WorktreeId`, so a tab opened on
                // the same path after the plan is never the one that closes.
                if let Some(session) = self.app_sessions.session_for(&target.worktree) {
                    self.close_tab_by_session(session, cx);
                }
            }
            Delivery::Invalidate(target) => {
                self.tab_cache.remove(&target.path);
                if self.app_sessions.session_for(&target.worktree) == self.active_session() {
                    self.reload(cx);
                }
            }
            Delivery::Completed {
                id,
                attachment,
                report,
            } => {
                let report = match report.evidence {
                    app::FamilyEvidence::Remove(report) => report,
                    app::FamilyEvidence::Stash(report) => {
                        self.deliver_stash_result(id, attachment, report, cx);
                        return;
                    }
                };
                let entry = report.recording.entry().clone();
                let summary = oplog_panel::outcome_summary(&entry.outcome);
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
                let text = if matches!(entry.outcome, OpOutcome::Refused { .. }) {
                    footer.clone()
                } else {
                    format!("{}: {}", entry.repo, summary)
                };
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
                        panel.push(entry);
                        cx.notify();
                    });
                }
                self.push_toast(
                    if success {
                        ToastKind::Success
                    } else {
                        ToastKind::Error
                    },
                    text.clone(),
                    cx,
                );
                if self.active_session() == Some(attachment.session) {
                    klog!("footer: {}", footer);
                    self.status_footer = if success {
                        FooterStatus::Success(footer.into())
                    } else {
                        FooterStatus::Failed(footer.into())
                    };
                }
                if !success {
                    let mut notice = modals::AppNotice::from(text);
                    if matches!(report.recording.entry().outcome, OpOutcome::Unknown { .. })
                        && !report.progress.termination_unknown
                    {
                        notice.inspect = Some(id);
                    }
                    self.app_notices.push_back(notice);
                }
                if let kagi_git::backend::remove::Recording::Failed { error, .. } = report.recording
                {
                    self.app_notices.push_back(
                        format!("{}: recording failed: {}", attachment.path.display(), error)
                            .into(),
                    );
                }
            }
        }
    }
    pub(crate) fn hold_host_close(&mut self, cx: &mut Context<Self>) -> bool {
        if self.app_sessions.may_close_host() {
            return false;
        }
        self.app_notices
            .push_back(Msg::OpInProgress.t().to_string().into());
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
        self.status_footer = FooterStatus::Failed(SharedString::from(message.clone()));
        self.app_notices.push_back(message.into());
        self.present_app_notice();
    }
    pub(crate) fn present_app_notice(&mut self) {
        if self.has_active_modal() {
            return;
        }
        if let Some(message) = self.app_notices.pop_front() {
            self.set_app_notice(message);
        }
    }
    pub(crate) fn poll_app_jobs(&mut self, cx: &mut Context<Self>) {
        self.refresh_write_busy();
        for delivery in self.app_sessions.drain_abandoned() {
            self.deliver_app_result(delivery, cx);
        }
        if !self.app_sessions.has_leases() && self.busy_op == Some("remove-worktree") {
            self.busy_op = None;
        }
        // The reload-time attempt may be deferred by an active modal. Retry
        // before queued notices are presented so the fresh drop confirmation
        // is not stranded after its conflict has been continued.
        self.present_stash_followup(cx);
    }
    pub(crate) fn confirm_app_notice(&mut self, cx: &mut Context<Self>) {
        let Some(notice) = self.app_notice().cloned() else {
            return;
        };
        self.clear_app_notice();
        if let Some(read) = notice.acknowledge {
            if let Err(error) = app::acknowledge(&mut self.app_sessions, read) {
                self.app_notices.push_back(error.to_string().into());
            }
        } else if let Some(id) = notice.inspect {
            match app::prepare_reconcile(&self.app_sessions, id) {
                Ok(job) => {
                    let task = cx.background_spawn(async move { job.run() });
                    cx.spawn(async move |this, cx| {
                        let result = task.await;
                        let _ = this.update(cx, |app, cx| {
                            match result {
                                Ok(read) => {
                                    app.app_notices.push_back(modals::AppNotice {
                                        message: format!(
                                            "{}\n{}",
                                            Msg::AppReconcileAcknowledge.t(),
                                            read.observation
                                        ),
                                        inspect: None,
                                        acknowledge: Some(read),
                                    });
                                }
                                Err(error) => {
                                    app.app_notices.push_back(modals::AppNotice {
                                        message: error,
                                        inspect: Some(id),
                                        acknowledge: None,
                                    });
                                }
                            }
                            cx.notify();
                        });
                    })
                    .detach();
                }
                Err(error) => self.app_notices.push_back(error.into()),
            }
        }
        cx.notify();
    }
}
