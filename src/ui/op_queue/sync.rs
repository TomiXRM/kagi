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

    /// Only a new explicit intent reopens a dismissed recovery notice, never a tick.
    pub(super) fn notice_queued_reconcile(&mut self, id: IntentId, cx: &mut Context<Self>) {
        if !self
            .op_queue
            .queue
            .intent(id)
            .is_some_and(|intent| self.op_queue.queue.is_reconciling(intent.owner))
        {
            return;
        }
        let Some(requirement) = self.app_sessions.blocking_reconcile() else {
            return;
        };
        let already_presented = self
            .app_notice()
            .into_iter()
            .chain(self.app_notices.iter())
            .any(|notice| {
                notice.inspect == Some(requirement)
                    || notice
                        .acknowledge
                        .as_ref()
                        .is_some_and(|read| read.operation() == requirement)
            });
        if !already_presented {
            self.report_admission_refusal(AdmissionError::NeedsReconcile, cx);
        }
    }

    /// One cancellable retry per waiting head. No plan or approval is renewed
    /// until a background, zero-wait repository probe succeeds.
    pub(super) fn sync_repository_retries(&mut self, cx: &mut Context<Self>) {
        use crate::app::WaitReason;
        let queue = &self.op_queue.queue;
        self.op_queue.repository_retries.retain(|id, _| {
            queue.intent(*id).is_some_and(|intent| {
                intent.state
                    == (IntentState::Waiting {
                        reason: WaitReason::RepositoryBusy,
                    })
            })
        });
        self.op_queue
            .repository_busy_notified
            .retain(|id| queue.intent(*id).is_some());
        for intent in queue.repository_waiters() {
            let id = intent.id;
            if self.op_queue.repository_retries.contains_key(&id) {
                continue;
            }
            let Some(owner) = self.app_sessions.attachment(intent.owner) else {
                continue;
            };
            let retry = cx.spawn(async move |this, acx| {
                let mut interval = TICK;
                loop {
                    acx.background_executor().timer(interval).await;
                    let path = owner.path.clone();
                    let available = acx
                        .background_spawn(async move {
                            match kagi_git::Backend::check_conflict_write_admission(&path) {
                                Ok(()) => true,
                                Err(error) => error.blocker().is_none(),
                            }
                        })
                        .await;
                    let keep_waiting = this
                        .update(acx, |app, cx| {
                            #[cfg(feature = "gui-e2e")]
                            {
                                app.op_queue.repository_probe_count += 1;
                            }
                            let current = app.op_queue.queue.intent(id).is_some_and(|intent| {
                                intent.owner == owner.session
                                    && intent.state
                                        == (IntentState::Waiting {
                                            reason: WaitReason::RepositoryBusy,
                                        })
                            }) && app.app_sessions.is_attached(owner.session);
                            if !current {
                                return false;
                            }
                            if available {
                                // Retire this task before reentrant admission can put
                                // the same intent back into Waiting and arm its successor.
                                if let Some(retry) = app.op_queue.repository_retries.remove(&id) {
                                    retry.detach();
                                }
                                #[cfg(feature = "gui-e2e")]
                                super::before_repository_resume_for_e2e();
                                #[cfg(feature = "gui-e2e")]
                                {
                                    app.op_queue.repository_resume_count += 1;
                                }
                                app.drive_queue(QueueEvent::RepositoryAvailable(id), cx);
                                return false;
                            }
                            true
                        })
                        .unwrap_or(false);
                    if !keep_waiting {
                        break;
                    }
                    interval = (interval * 2).min(std::time::Duration::from_secs(2));
                }
            });
            self.op_queue.repository_retries.insert(id, retry);
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
                WaitReason::RepositoryBusy => {
                    return crate::ui::i18n::Msg::AdviceConflictsRepositoryBusy
                        .t()
                        .into();
                }
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
