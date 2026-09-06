//! Sole runtime adapter for the first application family.
use crate::app::{self, Approved, Delivery, LegacyBusy};
use crate::ui::*;

impl KagiApp {
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
        let job = match app::prepare_remove(
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
        self.busy_op = Some("remove-worktree");
        self.clear_remove_worktree_modal();
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyRemoveWorktree.t()));
        klog!("async: remove-worktree started");
        let task = cx.background_spawn(async move {
            job.run_with_events(|event| {
                use kagi_git::backend::remove::RemoveEvent;
                match event {
                    RemoveEvent::ConfigTrusted => {
                        klog!("worktree: trusted .kagi/worktree.toml (pre_remove)")
                    }
                    RemoveEvent::ExecutionStarting => {}
                    RemoveEvent::ConfigRefused => {
                        klog!("refused: remove-worktree config changed after plan, not executing")
                    }
                }
            })
        });
        cx.spawn(async move |this, cx| {
            let completion = task.await;
            let _ = this.update(cx, |app, cx| {
                let deliveries = app::apply(&mut app.app_sessions, completion);
                if !app.app_sessions.has_leases() && app.busy_op == Some("remove-worktree") {
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
            Delivery::RemovedTarget(path) => {
                self.tab_cache.remove(&path);
            }
            Delivery::Invalidate(path) => {
                self.tab_cache.remove(&path);
                if self.repo_path.as_ref() == Some(&path) {
                    self.reload(cx);
                }
            }
            Delivery::Completed {
                id,
                attachment,
                report,
            } => {
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
                if self.repo_path.as_ref() == Some(&attachment.path)
                    && self.switch_generation == attachment.generation
                {
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
