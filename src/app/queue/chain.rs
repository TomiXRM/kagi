//! The chain and arbitration rules [`IntentQueue::apply`] composes: who is
//! the head, how a head is cancelled, how a receipt trips or advances the
//! chain, and which head may move (ADR-0204 決定 1 / 3 / 5).
use super::*;

impl IntentQueue {
    pub(super) fn head(&self, id: IntentId) -> Option<SessionId> {
        self.per_session
            .iter()
            .find_map(|(session, q)| (q.front().is_some_and(|i| i.id == id)).then_some(*session))
    }
    pub(super) fn clear_empty(&mut self, session: SessionId) {
        if self
            .per_session
            .get(&session)
            .is_none_or(VecDeque::is_empty)
        {
            self.per_session.remove(&session);
            self.gates.insert(session, ChainGate::default());
        }
    }
    pub(super) fn record_cancel(&mut self, mut intent: QueuedIntent, reason: CancelReason) {
        intent.state = IntentState::Cancelled { reason };
        let list = self.cancelled.entry(intent.owner).or_default();
        if list.len() == MAX_CANCELLED_INTENTS {
            list.pop_front();
        }
        list.push_back(intent);
    }
    pub(super) fn cancel_head(
        &mut self,
        session: SessionId,
        reason: CancelReason,
        effects: &mut Vec<QueueEffect>,
    ) {
        let Some(head) = self
            .per_session
            .get_mut(&session)
            .and_then(VecDeque::pop_front)
        else {
            return;
        };
        let id = head.id;
        match head.state {
            IntentState::Planning | IntentState::AwaitingConfirm => {
                effects.push(QueueEffect::CloseConfirm(id));
                effects.push(QueueEffect::InvalidatePlan(id));
                // Planning's job still owns the latch. Only PlanSlotFreed clears it.
                // A Planning head has no modal yet: the slot belongs to whoever
                // holds it, and only the confirmation the queue opened is released.
                if head.state == IntentState::AwaitingConfirm {
                    self.plan_slot_busy = false;
                    self.planning_job = None;
                    self.modal_busy = false;
                }
            }
            IntentState::Admitting => {
                effects.push(QueueEffect::CloseConfirm(id));
                effects.push(QueueEffect::InvalidatePlan(id));
                self.modal_busy = false;
            }
            IntentState::Queued | IntentState::Waiting { .. } => {}
            IntentState::Running { .. } | IntentState::Settled | IntentState::Cancelled { .. } => {
                self.per_session
                    .entry(session)
                    .or_default()
                    .push_front(head);
                return;
            }
        }
        self.record_cancel(head, reason);
        if reason == CancelReason::UserRemoved {
            // The user took this one out (RemoveOne on the head): not a
            // failure, so the rest move up behind the same anchor (決定 3).
            if matches!(
                self.gate(session),
                ChainGate::Armed { anchor: Some(ChainAnchor::QueuedHead(anchor)) } if anchor == id
            ) {
                let next = self
                    .per_session
                    .get(&session)
                    .and_then(VecDeque::front)
                    .map(|i| ChainAnchor::QueuedHead(i.id));
                self.gates
                    .insert(session, ChainGate::Armed { anchor: next });
            }
            self.clear_empty(session);
        } else {
            self.trip(session, ChainAnchor::QueuedHead(id));
        }
    }
    pub(super) fn trip(&mut self, session: SessionId, by: ChainAnchor) {
        self.gates.insert(session, ChainGate::Tripped { by });
        if let Some(mut queue) = self.per_session.remove(&session) {
            while let Some(intent) = queue.pop_front() {
                self.record_cancel(intent, CancelReason::ChainTripped { by });
            }
        }
        self.clear_empty(session);
    }
    pub(super) fn settle(
        &mut self,
        stamp: OwnerStamp,
        receipt: Settlement<'_>,
    ) -> Option<IntentId> {
        let session = stamp.session;
        let queued = self
            .per_session
            .get(&session)
            .and_then(VecDeque::front)
            .and_then(|head| match head.state {
                IntentState::Running { stamp: owner } if owner == stamp => Some(head.id),
                _ => None,
            });
        let anchored = matches!(self.gate(session), ChainGate::Armed { anchor: Some(ChainAnchor::ActiveWrite(owner)) } if owner == stamp);
        if !anchored && queued.is_none() {
            return None;
        } // wrong owner or duplicate receipt
        if let Some(id) = queued {
            let mut head = self
                .per_session
                .get_mut(&session)
                .expect("head exists")
                .pop_front()
                .expect("head exists");
            head.state = IntentState::Settled;
            if receipt.permits_successor() {
                let anchor = self
                    .per_session
                    .get(&session)
                    .and_then(VecDeque::front)
                    .map(|next| ChainAnchor::QueuedHead(next.id));
                self.gates.insert(session, ChainGate::Armed { anchor });
            } else {
                self.trip(session, ChainAnchor::QueuedHead(id));
            }
        } else if receipt.permits_successor() {
            let anchor = self
                .per_session
                .get(&session)
                .and_then(VecDeque::front)
                .map(|next| ChainAnchor::QueuedHead(next.id));
            self.gates.insert(session, ChainGate::Armed { anchor });
        } else {
            if self
                .per_session
                .get(&session)
                .is_some_and(|q| !q.is_empty())
            {
                self.trip(session, ChainAnchor::ActiveWrite(stamp));
            }
        }
        if receipt.reconcile_required {
            self.reconciling.insert(session);
        }
        self.clear_empty(session);
        queued
    }
    pub(super) fn arbitrate(&mut self, effects: &mut Vec<QueueEffect>) {
        let mut candidates: Vec<_> = self
            .per_session
            .iter()
            .filter_map(|(session, queue)| queue.front().map(|head| (head.enqueued, *session)))
            .collect();
        candidates.sort_unstable_by_key(|(order, _)| *order);
        let occupied = self.pipeline_count() != 0;
        let mut chosen = false;
        for (_, session) in candidates {
            let gate = self.gate(session);
            let head = self
                .per_session
                .get_mut(&session)
                .and_then(VecDeque::front_mut)
                .expect("candidate exists");
            if !matches!(
                head.state,
                IntentState::Queued | IntentState::Waiting { .. }
            ) {
                continue;
            }
            let reason = if self.input_focused {
                Some(WaitReason::NeedsConfirmation)
            } else if self.write.is_some() || self.write_busy {
                Some(WaitReason::WriteRunning)
            } else if self.plan_slot_busy || occupied || chosen {
                Some(WaitReason::PlanSlotBusy)
            } else if self.reconciling.contains(&session) {
                Some(WaitReason::NeedsReconcile)
            } else if self.active != Some(session)
                || self.modal_busy
                || self.revalidating.contains(&session)
            {
                Some(WaitReason::NeedsConfirmation)
            } else if matches!(
                gate,
                ChainGate::Armed {
                    anchor: Some(ChainAnchor::ActiveWrite(_))
                }
            ) {
                Some(WaitReason::WriteRunning)
            } else {
                None
            };
            match reason {
                Some(reason) => head.state = IntentState::Waiting { reason },
                None => {
                    head.state = IntentState::Planning;
                    self.plan_slot_busy = true;
                    self.planning_job = Some(head.id);
                    chosen = true;
                    effects.push(QueueEffect::StartPlan(head.id));
                }
            }
        }
    }
}
