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
            message: format!("{}: {op}", repo.display()),
            inspect: Some(id),
            acknowledge: None,
            release_armed: false,
        });
    }

    /// An admission the application layer refused: footer, toast and the
    /// shared app-notice modal, never stderr alone.
    pub(crate) fn report_admission_refusal(
        &mut self,
        error: app::AdmissionError,
        cx: &mut Context<Self>,
    ) {
        let message = if error == app::AdmissionError::Busy {
            Msg::OpInProgress.t().to_string()
        } else {
            error.to_string()
        };
        self.status_footer = FooterStatus::Failed(message.clone().into());
        self.push_toast(ToastKind::Error, message.clone(), cx);
        // A refusal made while a requirement is parked names it, so the user
        // can open it from the refusal instead of being told "no" with no way
        // to say yes (#702 re-review). `NeedsReconcile` is the direct case; a
        // `Busy` whose lease is held *by* an unproven termination is the same
        // wall, and `blocking_reconcile` is `None` when there is no such entry.
        let mut notice = modals::AppNotice::from(message);
        notice.inspect = self.app_sessions.blocking_reconcile();
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
                            read.observation
                        ));
                        notice.acknowledge = Some(read);
                        notice
                    }
                    Ok(read) => modals::AppNotice::from(read.observation),
                    Err(error) => modals::AppNotice::from(error),
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
                let entry = report.recording().entry().clone();
                app.notice_recording_failure(
                    "reconcile-release-unobservable",
                    report.recording(),
                    Path::new(&entry.repo),
                );
                if let Some(panel) = &app.op_log {
                    panel.update(cx, |panel, cx| {
                        panel.push(entry);
                        cx.notify();
                    });
                }
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
        // Dismissal preserves the recovery entry, never its armed approval.
        if let Some(mut notice) = self
            .app_notice()
            .filter(|notice| notice.inspect.is_some() || notice.acknowledge.is_some())
            .cloned()
        {
            notice.release_armed = false;
            self.app_notices.push_back(notice);
        }
        self.clear_app_notice();
    }
}
