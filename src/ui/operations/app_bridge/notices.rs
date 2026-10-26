//! The app-notice surface: what a settlement or a refusal leaves the user to
//! act on.
//!
//! Split from `app_bridge.rs` on that boundary. The rule these share is #702's:
//! a scope kagi has closed must always come with the way to open it — a parked
//! reconcile requirement is queued at settlement, a refusal names the entry
//! that is blocking, and a read that cannot be acknowledged yet comes back as
//! another look rather than a button that will be refused.
use super::*;

impl KagiApp {
    /// The way into a reconcile requirement settlement just parked.
    ///
    /// Queued from the **settle** half, beside the recording-failure notice: an
    /// unacknowledged requirement refuses every later write in that scope, and
    /// a completion whose tab the user left would otherwise leave an entry
    /// nobody can open (#702 re-review). Every family that settles through
    /// `apply` calls this.
    pub(crate) fn notice_reconcile_required(
        &mut self,
        id: app::OperationId,
        op: &str,
        repo: &std::path::Path,
    ) {
        if !self.app_sessions.needs_reconcile(id) {
            return;
        }
        self.app_notices.push_back(modals::AppNotice {
            message: format!(
                "{}: {}",
                repo.display(),
                i18n::op::user_text::operation_label(op)
            ),
            inspect: Some(id),
            acknowledge: None,
            release_armed: false,
            host_close_guard: false,
        });
    }

    /// Repository admission is a probe, not a receipt owner. Record a refused
    /// user attempt here, under its requested operation, before presenting it.
    pub(crate) fn report_write_admission_refusal(
        &mut self,
        operation: &str,
        path: &Path,
        error: app::AdmissionError,
        cx: &mut Context<Self>,
    ) {
        let entry = OpLogEntry::new(
            operation,
            path.display().to_string(),
            StateSummary {
                head: "unobserved".into(),
                dirty: "unobserved".into(),
            },
            OpOutcome::Refused {
                blockers: vec![error.to_string()],
            },
        )
        .with_worktree(Some(path.display().to_string()))
        .with_nothing_moved();
        let recording = kagi_git::backend::recording::finalize(entry);
        self.notice_recording_failure(operation, &recording, path, None);
        if matches!(
            recording,
            kagi_git::backend::recording::Recording::Failed { .. }
        ) {
            self.present_recorded(&recording, true, cx);
            self.present_app_notice();
            cx.notify();
            return;
        }
        self.insert_recorded_row(&recording, cx);
        if error == app::AdmissionError::RepositoryBusy {
            let reason = Msg::AdviceConflictsRepositoryBusy.t();
            self.status_footer =
                FooterStatus::Failed(crate::ui::operations::display_text(reason.into()));
            self.push_operation_result(
                recording.entry(),
                operation,
                ToastKind::Error,
                i18n::op::user_text::operations::failure_preview(operation, reason).into(),
                cx,
            );
            cx.notify();
        } else {
            self.report_admission_refusal(error, cx);
        }
    }

    /// A live writer refuses contextually; reconciliation still offers Inspect.
    pub(crate) fn report_admission_refusal(
        &mut self,
        error: app::AdmissionError,
        cx: &mut Context<Self>,
    ) {
        if error == app::AdmissionError::RepositoryBusy {
            let message = Msg::AdviceConflictsRepositoryBusy.t().to_string();
            self.status_footer =
                FooterStatus::Failed(crate::ui::operations::display_text(message.clone().into()));
            self.push_toast(ToastKind::Error, message, cx);
            cx.notify();
            return;
        }
        let blocking = self.app_sessions.blocking_reconcile();
        if error == app::AdmissionError::Busy && blocking.is_none() {
            self.status_footer = FooterStatus::Idle(Msg::OpInProgress.t().into());
            cx.notify();
            return;
        }
        let message = if error == app::AdmissionError::Busy {
            Msg::OpInProgress.t().to_string()
        } else {
            i18n::op::user_text::diagnostic(&error.to_string()).into_owned()
        };
        self.status_footer =
            FooterStatus::Failed(crate::ui::operations::display_text(message.clone().into()));
        self.push_toast(ToastKind::Error, message.clone(), cx);
        // A refusal made while a requirement is parked names it, so the user
        // can open it from the refusal instead of being told "no" with no way
        // to say yes (#702 re-review). `NeedsReconcile` is the direct case; a
        // `Busy` whose lease is held *by* an unproven termination is the same
        // wall, and `blocking_reconcile` is `None` when there is no such entry.
        let mut notice = modals::AppNotice::from(message);
        notice.inspect = blocking;
        self.app_notices.push_back(notice);
        self.present_app_notice();
        cx.notify();
    }

    pub fn confirm_app_notice(&mut self, cx: &mut Context<Self>) {
        let Some(mut notice) = self.app_notice().cloned() else {
            return;
        };
        self.clear_app_notice();
        if let Some(read) = notice.acknowledge.take() {
            if read.can_acknowledge_unobserved() {
                if notice.release_armed {
                    self.start_unobservable_release(read, cx);
                } else {
                    notice.release_armed = true;
                    notice.acknowledge = Some(read);
                    self.set_app_notice(notice);
                }
            } else {
                let id = read.operation();
                if let Err(error) = app::acknowledge(&mut self.app_sessions, read) {
                    let mut notice = modals::AppNotice::from(error.to_string());
                    notice.inspect = Some(id);
                    self.app_notices.push_back(notice);
                }
            }
        } else if let Some(id) = notice.inspect {
            self.inspect_reconcile(id, cx);
        }
        cx.notify();
    }

    fn inspect_reconcile(&mut self, id: app::OperationId, cx: &mut Context<Self>) {
        let job = match app::prepare_reconcile(&self.app_sessions, id) {
            Ok(job) => job,
            Err(error) => {
                self.app_notices.push_back(error.into());
                return;
            }
        };
        let task = cx.background_spawn(async move { job.run() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |app, cx| {
                let mut notice = match result {
                    Ok(read) if read.settled() || read.can_acknowledge_unobserved() => {
                        let warning = if read.can_acknowledge_unobserved() {
                            Msg::AppReconcileUnobservable
                        } else {
                            Msg::AppReconcileAcknowledge
                        };
                        let mut notice = modals::AppNotice::from(format!(
                            "{}\n{}",
                            warning.t(),
                            i18n::op::user_text::diagnostic(&read.observation)
                        ));
                        notice.acknowledge = Some(read);
                        notice
                    }
                    Ok(read) => modals::AppNotice::from(
                        i18n::op::user_text::diagnostic(&read.observation).into_owned(),
                    ),
                    Err(error) => modals::AppNotice::from(
                        i18n::op::user_text::diagnostic(&error).into_owned(),
                    ),
                };
                if notice.acknowledge.is_none() {
                    notice.inspect = Some(id);
                }
                app.app_notices.push_back(notice);
                cx.notify();
            });
        })
        .detach();
    }

    fn start_unobservable_release(&mut self, read: app::ReconcileRead, cx: &mut Context<Self>) {
        let id = read.operation();
        let job = match app::prepare_unobservable_release(&self.app_sessions, read) {
            Ok(job) => job,
            Err(error) => {
                self.report_admission_refusal(error, cx);
                return;
            }
        };
        let task = cx.background_spawn(async move { job.run() });
        cx.spawn(async move |this, cx| {
            let report = task.await;
            let _ = this.update(cx, |app, cx| {
                let recording = report.recording();
                app.notice_recording_failure(
                    "reconcile-release-unobservable",
                    recording,
                    Path::new(&recording.entry().repo),
                    None,
                );
                // Transform failed receipts without duplicating the recording-failure
                // notice with present_recorded's footer and toast.
                app.insert_recorded_row(recording, cx);
                if let Err(error) = app::acknowledge_unobserved(&mut app.app_sessions, report) {
                    let mut notice = modals::AppNotice::from(error);
                    notice.inspect = Some(id);
                    app.app_notices.push_back(notice);
                }
                app.refresh_write_busy();
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn cancel_app_notice(&mut self) {
        // Acknowledge the presentation, not the reconcile requirement. The
        // oplog retains durable detail; a fresh admission refusal can offer
        // Inspect again. Dropping this payload also discards armed approval.
        self.clear_app_notice();
    }
}
