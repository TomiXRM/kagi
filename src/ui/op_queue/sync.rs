use super::*;

impl KagiApp {
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
        if self.op_queue.observed_input_focused != self.op_queue.input_focused {
            self.op_queue.input_focused = self.op_queue.observed_input_focused;
            let event = if self.op_queue.input_focused {
                QueueEvent::InputFocused
            } else {
                QueueEvent::InputBlurred
            };
            self.drive_queue(event, cx);
        }
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
            if !self.plan_modal().is_some_and(|m| m.queued == Some(id))
                && !self.queued_commit_modal().is_some_and(|m| m.queued == id)
                && !self.merge_modal().is_some_and(|m| m.queued == Some(id))
            {
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

    pub(super) fn ensure_queue_ticker(&mut self, cx: &mut Context<Self>) {
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
                WaitReason::Typing => QueueText::WaitTyping,
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
            R::MergeStarted => QueueText::ReasonMergeStarted,
            R::StaleApproval => QueueText::ReasonStale,
            R::CapacityRejected => QueueText::Full,
        })
    }
}
