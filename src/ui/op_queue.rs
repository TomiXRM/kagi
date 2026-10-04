//! Wiring of the operation queue (#355 stage 3a, ADR-0204).
//!
//! [`crate::app::IntentQueue`] is a pure reducer. This module is the one place
//! that feeds it events and carries out its effects: everything reaches it
//! through [`KagiApp::drive_queue`]. Observations the reducer needs but no
//! single call site owns — the tab on screen, the modal slot, a revalidating
//! read, the remote latch, a plan job outside the queue, a released lease —
//! are compared with what the queue was last told in [`KagiApp::sync_queue`],
//! which runs on every enqueue, on every completed run-family write, and on a
//! 250 ms ticker while any tab has intents.
//!
//! Only the checkout family is wired in stage 3a.
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gpui::{Context, SharedString};
use kagi_ui_core::i18n::{queue_text, QueueText};

use super::modals::{CheckoutPlanModal, CheckoutPlanTarget};
use super::{i18n, KagiApp};
use crate::app::{
    AdmissionError, CheckoutIntent, EnqueueError, ExecutionEvidence, IntentId, IntentQueue,
    IntentRequest, IntentState, OperationId, OwnerStamp, QueueEffect, QueueEvent, SessionId,
    Settlement,
};

const TICK: Duration = Duration::from_millis(250);

/// The window's queue and what it was last told.
#[derive(Default)]
pub(crate) struct QueueWiring {
    pub(crate) queue: IntentQueue,
    active: Option<SessionId>,
    modal: bool,
    revalidating: Option<SessionId>,
    remote: bool,
    /// The tab that started the lease-less remote pull (set at its latch).
    remote_owner: Option<SessionId>,
    planning: bool,
    /// The tracked write the queue was told about, until its release.
    write: Option<OwnerStamp>,
    /// An untracked write (guard writer, clone) the queue was told about.
    untracked: bool,
    /// Set just before `finish_run`, consumed at its admission.
    next_run: Option<NextRun>,
    /// Verify results of tracked writes still running.
    runs: HashMap<OwnerStamp, Arc<AtomicBool>>,
    /// Replanned intents not yet shown or admitted.
    plans: HashMap<IntentId, CheckoutPlanModal>,
    /// The head whose confirmation is on screen.
    confirming: Option<IntentId>,
    /// The user dismissed that confirmation (`cancel_modal` has no context).
    rejected: Option<IntentId>,
    logged_cancels: HashSet<IntentId>,
    ticker_alive: bool,
}

struct NextRun {
    intent: Option<IntentId>,
    verified: Arc<AtomicBool>,
}

/// Domain words stay English in both languages (ADR-0048).
pub(crate) fn intent_label(request: &IntentRequest) -> String {
    match request {
        IntentRequest::Checkout {
            target: CheckoutIntent::Branch(branch),
        } => format!("checkout {branch}"),
        IntentRequest::Checkout {
            target: CheckoutIntent::Commit(commit),
        } => format!("checkout {}", commit.short()),
        IntentRequest::Commit { message } => {
            let subject: String = message
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(40)
                .collect();
            format!("commit {subject}")
        }
        IntentRequest::Merge { source, into } => format!("merge {source} → {into}"),
    }
}

impl KagiApp {
    /// Feed one event to the queue and carry out every effect it returns.
    pub(crate) fn drive_queue(&mut self, event: QueueEvent<'_>, cx: &mut Context<Self>) {
        let effects = self.op_queue.queue.apply(event);
        self.run_queue_effects(effects, cx);
    }

    /// The active tab has intents waiting: a new one goes behind them, even
    /// when nothing is running, or it would overtake them.
    pub(crate) fn active_tab_has_queue(&self) -> bool {
        self.active_session()
            .is_some_and(|session| !self.op_queue.queue.auto_fetch_allowed(session))
    }

    pub(crate) fn enqueue_checkout(
        &mut self,
        target: &CheckoutPlanTarget,
        cx: &mut Context<Self>,
    ) -> bool {
        let target = match target {
            CheckoutPlanTarget::Branch(branch) => CheckoutIntent::Branch(branch.clone()),
            CheckoutPlanTarget::Commit(commit) => CheckoutIntent::Commit(commit.clone()),
        };
        self.enqueue_intent(IntentRequest::Checkout { target }, cx)
    }

    fn enqueue_intent(&mut self, request: IntentRequest, cx: &mut Context<Self>) -> bool {
        let Some(owner) = self
            .active_session()
            .and_then(|session| self.app_sessions.attachment(session))
        else {
            return false;
        };
        // The queue must see the write, the tab and the modal as they are now.
        self.sync_queue(cx);
        let label = intent_label(&request);
        let effects = self
            .op_queue
            .queue
            .apply(QueueEvent::Enqueue { owner, request });
        let rejected = effects.iter().find_map(|effect| match effect {
            QueueEffect::Rejected(error) => Some(*error),
            _ => None,
        });
        if let Some(error) = rejected {
            klog!("queue: rejected {} ({:?})", label, error);
        }
        self.run_queue_effects(effects, cx);
        rejected.is_none()
    }

    fn run_queue_effects(&mut self, effects: Vec<QueueEffect>, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                QueueEffect::Enqueued(id) => {
                    let label = self.queued_label(id);
                    klog!("queue: enqueued {}", label);
                    self.push_toast(
                        super::ToastKind::Info,
                        format!("{}: {label}", queue_text(QueueText::Queued)),
                        cx,
                    );
                }
                QueueEffect::Rejected(EnqueueError::CapacityRejected) => {
                    self.push_toast(super::ToastKind::Error, queue_text(QueueText::Full), cx);
                }
                // The entry point keeps its old refusal (`OpInProgress`).
                QueueEffect::Rejected(_) => {}
                // Logged with every other cancellation below.
                QueueEffect::Cancelled(..) => {}
                QueueEffect::Settled(id) => klog!("queue: settled {}", id.0),
                QueueEffect::StartPlan(id) => self.plan_queued(id, cx),
                QueueEffect::OpenConfirm(id) => self.confirm_or_run_queued(id, cx),
                QueueEffect::BeginAdmission(id) => self.admit_queued(id, cx),
                QueueEffect::CloseConfirm(id) => {
                    if self.plan_modal().is_some_and(|m| m.queued == Some(id)) {
                        self.clear_plan_modal();
                    }
                    if self.op_queue.confirming == Some(id) {
                        self.op_queue.confirming = None;
                    }
                }
                QueueEffect::InvalidatePlan(id) => {
                    self.op_queue.plans.remove(&id);
                }
            }
        }
        self.log_new_cancellations();
        self.ensure_queue_ticker(cx);
        cx.notify();
    }

    fn queued_label(&self, id: IntentId) -> String {
        self.op_queue
            .queue
            .intent(id)
            .map_or_else(|| format!("#{}", id.0), |i| intent_label(&i.request))
    }

    fn log_new_cancellations(&mut self) {
        let mut seen = HashSet::new();
        for session in self.op_queue.queue.sessions() {
            for intent in self.op_queue.queue.cancelled(session).into_iter().flatten() {
                seen.insert(intent.id);
                if !self.op_queue.logged_cancels.contains(&intent.id) {
                    if let IntentState::Cancelled { reason } = intent.state {
                        klog!(
                            "queue: cancelled {} ({:?})",
                            intent_label(&intent.request),
                            reason
                        );
                    }
                }
            }
        }
        // Dismissed entries leave the log set with the list.
        self.op_queue.logged_cancels = seen;
    }

    /// Live replan at the head (ADR-0204 決定 4). Checkout plans synchronously
    /// against the tab's session, so the plan slot is freed in the same turn.
    fn plan_queued(&mut self, id: IntentId, cx: &mut Context<Self>) {
        let Some(intent) = self.op_queue.queue.intent(id).cloned() else {
            return;
        };
        // Same `SessionId`, same `WorktreeId`, on screen — or cancel. Never
        // re-aim at another target.
        let same_owner = self.active_session() == Some(intent.owner)
            && self
                .app_sessions
                .attachment(intent.owner)
                .is_some_and(|owner| owner.worktree.as_ref() == Some(&intent.worktree));
        let label = intent_label(&intent.request);
        if !same_owner {
            klog!("queue: owner changed for {}", label);
            self.drive_queue(QueueEvent::IdentityChanged(id), cx);
            self.drive_queue(QueueEvent::PlanSlotFreed(Some(id)), cx);
            return;
        }
        let IntentRequest::Checkout { target } = &intent.request else {
            // Stage 3a has no entry point for the other families.
            self.drive_queue(QueueEvent::PlanError(id), cx);
            self.drive_queue(QueueEvent::PlanSlotFreed(Some(id)), cx);
            return;
        };
        let (target, planned) = {
            let Some(repo) = self.ui().repo_session.as_ref().map(|s| s.backend()) else {
                self.report_plan_failure(
                    i18n::Op::Checkout,
                    super::modal_plan::SESSION_UNAVAILABLE,
                );
                self.drive_queue(QueueEvent::PlanError(id), cx);
                self.drive_queue(QueueEvent::PlanSlotFreed(Some(id)), cx);
                return;
            };
            match target {
                CheckoutIntent::Branch(branch) => (
                    CheckoutPlanTarget::Branch(branch.clone()),
                    repo.plan_checkout(branch),
                ),
                CheckoutIntent::Commit(commit) => (
                    CheckoutPlanTarget::Commit(commit.clone()),
                    repo.plan_checkout_commit(commit),
                ),
            }
        };
        match planned {
            Ok(plan) => {
                klog!(
                    "queue: plan {} blockers={} warnings={}",
                    label,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.op_queue.plans.insert(
                    id,
                    CheckoutPlanModal {
                        target,
                        stash_first: false,
                        plan: Arc::new(plan),
                        error: None,
                        queued: Some(id),
                    },
                );
                self.drive_queue(QueueEvent::PlanCompleted(id), cx);
            }
            Err(error) => {
                klog!("queue: plan error {}: {}", label, error);
                self.report_plan_failure(i18n::Op::Checkout, error);
                self.drive_queue(QueueEvent::PlanError(id), cx);
                self.drive_queue(QueueEvent::PlanSlotFreed(Some(id)), cx);
            }
        }
    }

    /// The family's own confirmation rule (ADR-0204 決定 4, amended): a
    /// checkout whose plan has neither blockers nor warnings runs without a
    /// modal, exactly as a double click does; anything else asks.
    fn confirm_or_run_queued(&mut self, id: IntentId, cx: &mut Context<Self>) {
        let Some(modal) = self.op_queue.plans.get(&id).cloned() else {
            return;
        };
        // Never take the slot from a modal the queue has not heard of yet.
        if self.has_active_modal() {
            self.sync_queue(cx);
            self.drive_queue(QueueEvent::ConfirmWithdrawn(id), cx);
            return;
        }
        let label = self.queued_label(id);
        if modal.plan.blockers.is_empty() && modal.plan.warnings.is_empty() {
            klog!("queue: run {} (clean plan)", label);
            self.drive_queue(QueueEvent::Approve(id), cx);
        } else {
            klog!("queue: confirm {}", label);
            self.op_queue.plans.remove(&id);
            self.op_queue.confirming = Some(id);
            self.set_plan_modal(modal);
        }
    }

    /// The user confirmed a queued checkout's modal.
    pub(crate) fn confirm_queued_checkout(
        &mut self,
        id: IntentId,
        modal: CheckoutPlanModal,
        cx: &mut Context<Self>,
    ) {
        self.op_queue.confirming = None;
        if !modal.plan.blockers.is_empty() {
            // Records the refusal and closes the modal, as for any checkout.
            self.run_checkout(modal, Some(id), cx);
            self.drive_queue(QueueEvent::PlanError(id), cx);
            return;
        }
        self.clear_plan_modal();
        self.op_queue.plans.insert(id, modal);
        self.drive_queue(QueueEvent::Approve(id), cx);
    }

    /// `cancel_modal` runs without a context: remember the answer for the next
    /// sync, which turns it into `Reject`.
    pub(crate) fn note_queued_confirm_dismissed(&mut self) {
        if let Some(id) = self.plan_modal().and_then(|m| m.queued) {
            self.op_queue.rejected = Some(id);
        }
    }

    fn admit_queued(&mut self, id: IntentId, cx: &mut Context<Self>) {
        let Some(modal) = self.op_queue.plans.remove(&id) else {
            self.drive_queue(
                QueueEvent::Admission {
                    id,
                    result: Err(AdmissionError::StaleApproval),
                },
                cx,
            );
            return;
        };
        self.run_checkout(modal, Some(id), cx);
        // `run_checkout` returned before admission (no repository path).
        if self
            .op_queue
            .queue
            .intent(id)
            .is_some_and(|intent| intent.state == IntentState::Admitting)
        {
            self.op_queue.next_run = None;
            self.drive_queue(
                QueueEvent::Admission {
                    id,
                    result: Err(AdmissionError::StaleApproval),
                },
                cx,
            );
        }
    }

    /// Called just before `finish_run` by a family with a verify path. The
    /// returned flag is set by the job when the postcondition holds.
    pub(crate) fn prepare_queue_run(&mut self, intent: Option<IntentId>) -> Arc<AtomicBool> {
        let verified = Arc::new(AtomicBool::new(false));
        self.op_queue.next_run = Some(NextRun {
            intent,
            verified: verified.clone(),
        });
        verified
    }

    /// `finish_run` refused admission.
    pub(crate) fn queue_admission_refused(
        &mut self,
        error: &AdmissionError,
        cx: &mut Context<Self>,
    ) {
        if let Some(NextRun {
            intent: Some(id), ..
        }) = self.op_queue.next_run.take()
        {
            self.drive_queue(
                QueueEvent::Admission {
                    id,
                    result: Err(error.clone()),
                },
                cx,
            );
        }
    }

    /// `finish_run` admitted a write. One with a verify path is a possible
    /// chain anchor; every other run-family write is untracked.
    pub(crate) fn queue_write_admitted(&mut self, stamp: OwnerStamp, cx: &mut Context<Self>) {
        match self.op_queue.next_run.take() {
            Some(NextRun { intent, verified }) => {
                self.op_queue.runs.insert(stamp, verified);
                self.op_queue.write = Some(stamp);
                match intent {
                    Some(id) => self.drive_queue(
                        QueueEvent::Admission {
                            id,
                            result: Ok(stamp),
                        },
                        cx,
                    ),
                    None => self.drive_queue(QueueEvent::WriteStarted(stamp), cx),
                }
            }
            None => self.queue_untracked_write(Some(stamp.session), cx),
        }
    }

    /// The lease-less remote pull latched from the tab now on screen: its
    /// own queue must refuse new intents, others only wait (ADR-0204 決定 3).
    pub(crate) fn note_remote_write_owner(&mut self) {
        self.op_queue.remote_owner = self.active_session();
    }

    /// A write the queue cannot judge started. `owner: None` is background
    /// work (auto-fetch) that is no one's predecessor.
    pub(crate) fn queue_untracked_write(
        &mut self,
        owner: Option<SessionId>,
        cx: &mut Context<Self>,
    ) {
        self.op_queue.untracked = true;
        self.drive_queue(QueueEvent::UntrackedWriteStarted { owner }, cx);
    }

    /// A run-family write settled through `apply`: hand its receipt to the
    /// chain, whatever tab is on screen.
    pub(crate) fn queue_write_settled(
        &mut self,
        stamp: OwnerStamp,
        id: OperationId,
        report: &kagi_git::backend::recording::RunReport,
        cx: &mut Context<Self>,
    ) {
        if let Some(verified) = self.op_queue.runs.remove(&stamp) {
            let evidence = if verified.load(Ordering::SeqCst) {
                ExecutionEvidence::Verified
            } else {
                ExecutionEvidence::Unverified
            };
            let reconcile_required = self.app_sessions.needs_reconcile(id);
            self.drive_queue(
                QueueEvent::AnchorSettled {
                    stamp,
                    receipt: Settlement {
                        outcome: &report.recording.entry().outcome,
                        evidence,
                        recording: &report.recording,
                        reconcile_required,
                    },
                },
                cx,
            );
        }
    }

    fn queue_revalidating(&self, session: SessionId) -> bool {
        self.ui
            .get(&session)
            .is_some_and(|ui| ui.panes_revalidating())
            || self
                .app_sessions
                .worktree_of(session)
                .is_some_and(|worktree| self.app_sessions.is_stale(worktree))
    }

    /// Before a new writer is admitted: release in the queue whatever ended
    /// while it was idle, so the new write is not mistaken for the old one's
    /// owner (#1018 review). A queued write admitted by this very sync owns
    /// the slot itself; the caller's prepared run is kept for its own turn.
    pub(crate) fn sync_queue_before_admission(&mut self, cx: &mut Context<Self>) {
        let prepared = self.op_queue.next_run.take();
        self.sync_queue(cx);
        self.op_queue.next_run = prepared;
    }

    /// Tell the queue what changed since it was last told. The release of a
    /// write is reported last, so the arbitration it triggers already sees
    /// the tab, the read and the modal slot as they are.
    pub(crate) fn sync_queue(&mut self, cx: &mut Context<Self>) {
        let active = self.active_session();
        // The returning tab's read state first: an `OwnerReturned` that lands
        // before its `RevalidationStarted` would replan against a stale read.
        let revalidating = active.filter(|session| self.queue_revalidating(*session));
        if revalidating != self.op_queue.revalidating {
            let previous = self.op_queue.revalidating;
            self.op_queue.revalidating = revalidating;
            if let Some(session) = revalidating {
                self.drive_queue(QueueEvent::RevalidationStarted(session), cx);
            }
            if let Some(session) = previous.filter(|p| Some(*p) != revalidating) {
                self.drive_queue(QueueEvent::RevalidationDone(session), cx);
            }
        }
        if active != self.op_queue.active {
            let previous = self.op_queue.active;
            self.op_queue.active = active;
            match (active, previous) {
                (Some(session), _) => self.drive_queue(QueueEvent::OwnerReturned(session), cx),
                (None, Some(session)) => self.drive_queue(QueueEvent::OwnerDeparted(session), cx),
                (None, None) => {}
            }
        }
        for session in self.op_queue.queue.sessions() {
            if !self.app_sessions.is_attached(session) {
                self.drive_queue(QueueEvent::OwnerDetached(session), cx);
            }
        }
        let remote = self.remote_write.is_some();
        if remote != self.op_queue.remote {
            self.op_queue.remote = remote;
            match (remote, self.op_queue.remote_owner.or(active)) {
                (true, Some(owner)) => {
                    self.drive_queue(QueueEvent::RemoteLatched(owner), cx);
                }
                // No tab owns the screen: nothing can be queued, and the
                // latch is observed again once a tab is on screen.
                (true, None) => self.op_queue.remote = false,
                (false, _) => {
                    self.op_queue.remote_owner = None;
                    self.drive_queue(QueueEvent::RemoteLatchReleased, cx);
                }
            }
        }
        let planning = self.planning.is_some();
        if planning != self.op_queue.planning {
            self.op_queue.planning = planning;
            let event = if planning {
                QueueEvent::PlanSlotTaken
            } else {
                QueueEvent::PlanSlotFreed(None)
            };
            self.drive_queue(event, cx);
        }
        let modal = self.has_active_modal();
        if modal != self.op_queue.modal {
            self.op_queue.modal = modal;
            let event = if modal {
                QueueEvent::ModalSlotBusy
            } else {
                QueueEvent::ModalSlotFree
            };
            self.drive_queue(event, cx);
        }
        if let Some(id) = self.op_queue.confirming {
            if !self.plan_modal().is_some_and(|m| m.queued == Some(id)) {
                self.op_queue.confirming = None;
                let event = if self.op_queue.rejected.take() == Some(id) {
                    klog!("queue: declined {}", self.queued_label(id));
                    QueueEvent::Reject(id)
                } else {
                    QueueEvent::ConfirmWithdrawn(id)
                };
                self.drive_queue(event, cx);
            }
        }
        // A reconcile the queue waits on is cleared only by its acknowledgement
        // (ADR-0204 決定 5). The entries carry no session, so the queue waits
        // until none is left anywhere.
        if self.app_sessions.blocking_reconcile().is_none() {
            for session in self.op_queue.queue.reconciling_sessions() {
                self.drive_queue(QueueEvent::ReconcileAcknowledged(session), cx);
            }
        }
        let cloning = self.home_github.cloning.is_some();
        if cloning && !self.op_queue.untracked && self.op_queue.write.is_none() {
            self.queue_untracked_write(None, cx);
        }
        if !cloning
            && !self.app_sessions.has_leases()
            && (self.op_queue.write.is_some() || self.op_queue.untracked)
        {
            let stamp = self.op_queue.write.take();
            self.op_queue.untracked = false;
            self.drive_queue(QueueEvent::LeaseReleased(stamp), cx);
        }
    }

    fn ensure_queue_ticker(&mut self, cx: &mut Context<Self>) {
        if self.op_queue.ticker_alive || self.op_queue.queue.is_empty() {
            return;
        }
        self.op_queue.ticker_alive = true;
        cx.spawn(async move |this, acx| loop {
            acx.background_executor().timer(TICK).await;
            let alive = this
                .update(acx, |app, cx| {
                    app.sync_queue(cx);
                    // Redraw for the running row's elapsed seconds.
                    cx.notify();
                    if app.op_queue.queue.is_empty() {
                        app.op_queue.ticker_alive = false;
                        return false;
                    }
                    true
                })
                .unwrap_or(false);
            if !alive {
                break;
            }
        })
        .detach();
    }

    /// Strip actions.
    pub(crate) fn queue_remove(&mut self, id: IntentId, cx: &mut Context<Self>) {
        let head = self.op_queue.queue.intent(id).map(|intent| intent.state);
        let event = match head {
            Some(IntentState::Planning | IntentState::AwaitingConfirm) => {
                QueueEvent::CancelHead(id)
            }
            Some(IntentState::Queued | IntentState::Waiting { .. }) => QueueEvent::RemoveOne(id),
            _ => return,
        };
        klog!("queue: remove {}", self.queued_label(id));
        self.drive_queue(event, cx);
    }

    pub(crate) fn queue_cancel_all(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.active_session() {
            klog!("queue: cancel all");
            self.drive_queue(QueueEvent::CancelAll(session), cx);
        }
    }

    pub(crate) fn queue_dismiss_cancelled(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.active_session() {
            self.drive_queue(QueueEvent::DismissCancelled(session), cx);
        }
    }

    /// A label for the strip's state column.
    pub(crate) fn queue_state_text(state: IntentState) -> SharedString {
        use crate::app::WaitReason;
        let key = match state {
            IntentState::Queued => QueueText::Count,
            IntentState::Waiting { reason } => match reason {
                WaitReason::WriteRunning => QueueText::WaitWrite,
                WaitReason::PlanSlotBusy => QueueText::WaitPlan,
                WaitReason::NeedsConfirmation => QueueText::WaitConfirm,
                WaitReason::NeedsReconcile => QueueText::WaitReconcile,
                WaitReason::RemoteLatched => QueueText::WaitPull,
            },
            IntentState::Planning => QueueText::Planning,
            IntentState::AwaitingConfirm => QueueText::Confirming,
            IntentState::Admitting => QueueText::Starting,
            IntentState::Running { .. } => QueueText::Running,
            IntentState::Settled | IntentState::Cancelled { .. } => QueueText::Cancelled,
        };
        SharedString::from(queue_text(key))
    }

    pub(crate) fn queue_cancel_text(reason: crate::app::CancelReason) -> &'static str {
        use crate::app::CancelReason as R;
        queue_text(match reason {
            R::UserRemoved => QueueText::ReasonRemoved,
            R::UserRejected => QueueText::ReasonRejected,
            R::PlanError => QueueText::ReasonPlanError,
            R::ChainTripped { .. } => QueueText::ReasonChain,
            R::OwnerGone => QueueText::ReasonOwnerGone,
            R::IdentityChanged => QueueText::ReasonIdentity,
            R::StaleApproval => QueueText::ReasonStale,
            R::CapacityRejected => QueueText::Full,
        })
    }
}

#[cfg(feature = "gui-e2e")]
thread_local! {
    static RUN_HOLD: std::cell::RefCell<Option<gpui::Task<()>>> =
        const { std::cell::RefCell::new(None) };
    static RUN_PANIC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(feature = "gui-e2e")]
pub(super) fn take_run_panic() -> bool {
    RUN_PANIC.with(|flag| flag.replace(false))
}

#[cfg(feature = "gui-e2e")]
pub(super) fn take_run_hold() -> Option<gpui::Task<()>> {
    RUN_HOLD.with(|slot| slot.borrow_mut().take())
}

#[cfg(feature = "gui-e2e")]
impl KagiApp {
    /// Hold the next admitted run-family write (checkout, commit, …) before
    /// its backend work: the lease is held, nothing has been written.
    pub fn hold_next_run_for_e2e(hold: gpui::Task<()>) {
        RUN_HOLD.with(|slot| assert!(slot.borrow_mut().replace(hold).is_none()));
    }

    /// The next admitted run-family job dies before running: its completion
    /// is the abandonment's `Unknown`, which parks a reconcile entry.
    pub fn panic_next_run_for_e2e() {
        RUN_PANIC.with(|flag| flag.set(true));
    }

    /// Report the current window state to the queue now, as the ticker would,
    /// before any read started by the caller can land.
    pub fn sync_queue_for_e2e(&mut self, cx: &mut Context<Self>) {
        self.sync_queue(cx);
    }
}
