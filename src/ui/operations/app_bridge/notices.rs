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
}
