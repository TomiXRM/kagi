//! The reducer events stage 3's wiring needs (ADR-0204, #355 stage 3a).
use super::*;

#[test]
fn owners_untracked_write_rejects_its_enqueue_but_another_tab_waits() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(2)));
    q.apply(QueueEvent::UntrackedWriteStarted {
        owner: Some(session(1)),
    });
    assert_eq!(
        q.apply(QueueEvent::Enqueue {
            owner: owner(1),
            request: request(),
        }),
        vec![QueueEffect::Rejected(EnqueueError::UntrackedWrite)]
    );
    assert!(q.intents(session(1)).is_none());
    let other = enqueue(&mut q, 2);
    assert_eq!(q.gate(session(2)), ChainGate::Armed { anchor: None });
    assert_eq!(
        state(&q, 2),
        IntentState::Waiting {
            reason: WaitReason::WriteRunning
        }
    );
    assert_eq!(
        q.apply(QueueEvent::LeaseReleased(None)),
        vec![QueueEffect::StartPlan(other)]
    );
    // Released: the first tab may queue again.
    assert!(matches!(
        q.apply(QueueEvent::Enqueue {
            owner: owner(1),
            request: request(),
        })[0],
        QueueEffect::Enqueued(_)
    ));
}

#[test]
fn untracked_release_does_not_forget_a_tracked_write() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(2)));
    q.apply(QueueEvent::WriteStarted(stamp(1, 40)));
    let id = enqueue(&mut q, 2);
    assert_eq!(q.apply(QueueEvent::LeaseReleased(None)), vec![]);
    assert_eq!(
        state(&q, 2),
        IntentState::Waiting {
            reason: WaitReason::WriteRunning
        }
    );
    assert_eq!(
        q.apply(QueueEvent::LeaseReleased(Some(stamp(1, 40)))),
        vec![QueueEffect::StartPlan(id)]
    );
}

#[test]
fn a_plan_outside_the_queue_holds_the_head_until_its_slot_frees() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    q.apply(QueueEvent::PlanSlotTaken);
    let id = enqueue(&mut q, 1);
    assert_eq!(
        state(&q, 1),
        IntentState::Waiting {
            reason: WaitReason::PlanSlotBusy
        }
    );
    assert_eq!(
        q.apply(QueueEvent::PlanSlotFreed(None)),
        vec![QueueEffect::StartPlan(id)]
    );
}

#[test]
fn a_withdrawn_confirmation_is_replanned_not_cancelled() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let id = enqueue(&mut q, 1);
    assert_eq!(
        q.apply(QueueEvent::PlanCompleted(id)),
        vec![QueueEffect::OpenConfirm(id)]
    );
    // A reload swept the confirmation: the UI observes the empty slot first.
    assert_eq!(q.apply(QueueEvent::ModalSlotFree), vec![]);
    assert_eq!(
        q.apply(QueueEvent::ConfirmWithdrawn(id)),
        vec![QueueEffect::InvalidatePlan(id), QueueEffect::StartPlan(id)]
    );
    assert_eq!(state(&q, 1), IntentState::Planning);
    assert!(q.cancelled(session(1)).is_none());
    assert_eq!(q.gate(session(1)), ChainGate::Armed { anchor: None });
}

#[test]
fn a_confirmation_replaced_by_another_modal_waits_for_the_slot() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let id = enqueue(&mut q, 1);
    q.apply(QueueEvent::PlanCompleted(id));
    q.apply(QueueEvent::ModalSlotBusy);
    assert_eq!(
        q.apply(QueueEvent::ConfirmWithdrawn(id)),
        vec![QueueEffect::InvalidatePlan(id)]
    );
    assert_eq!(
        state(&q, 1),
        IntentState::Waiting {
            reason: WaitReason::NeedsConfirmation
        }
    );
    assert_eq!(
        q.apply(QueueEvent::ModalSlotFree),
        vec![QueueEffect::StartPlan(id)]
    );
}

#[test]
fn a_withdrawal_never_touches_an_approved_head() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let id = enqueue(&mut q, 1);
    q.apply(QueueEvent::PlanCompleted(id));
    q.apply(QueueEvent::Approve(id));
    assert_eq!(q.apply(QueueEvent::ConfirmWithdrawn(id)), vec![]);
    assert_eq!(state(&q, 1), IntentState::Admitting);
}

#[test]
fn removing_the_confirming_head_keeps_its_successors() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let head = enqueue(&mut q, 1);
    let next = enqueue(&mut q, 1);
    q.apply(QueueEvent::PlanCompleted(head));
    q.apply(QueueEvent::ModalSlotBusy);
    let effects = q.apply(QueueEvent::CancelHead(head));
    assert!(effects.contains(&QueueEffect::CloseConfirm(head)));
    q.apply(QueueEvent::ModalSlotFree);
    assert_eq!(q.intents(session(1)).unwrap().front().unwrap().id, next);
    assert_eq!(state(&q, 1), IntentState::Planning, "the next one moves up");
    assert_eq!(
        q.cancelled(session(1)).unwrap().len(),
        1,
        "only the removed head is listed"
    );
    // Declining a confirmation is a failure of the chain, not a removal.
    q.apply(QueueEvent::PlanCompleted(next));
    let last = enqueue(&mut q, 1);
    q.apply(QueueEvent::Reject(next));
    assert!(q.intents(session(1)).is_none());
    assert_eq!(
        q.cancelled(session(1)).unwrap().back().unwrap().state,
        IntentState::Cancelled {
            reason: CancelReason::ChainTripped {
                by: ChainAnchor::QueuedHead(next)
            }
        }
    );
    assert_eq!(q.cancelled(session(1)).unwrap().back().unwrap().id, last);
}

#[test]
fn cancel_all_takes_the_confirming_head_and_every_waiting_intent() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let head = enqueue(&mut q, 1);
    enqueue(&mut q, 1);
    enqueue(&mut q, 1);
    q.apply(QueueEvent::PlanCompleted(head));
    q.apply(QueueEvent::CancelAll(session(1)));
    assert!(q.intents(session(1)).is_none());
    let listed = q.cancelled(session(1)).unwrap();
    assert_eq!(listed.len(), 3);
    assert!(listed.iter().all(|i| i.state
        == IntentState::Cancelled {
            reason: CancelReason::UserRemoved
        }));
}

#[test]
fn a_reconcile_wait_names_its_session_until_acknowledged() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let id = enqueue(&mut q, 1);
    q.apply(QueueEvent::PlanCompleted(id));
    q.apply(QueueEvent::Approve(id));
    q.apply(QueueEvent::Admission {
        id,
        result: Err(crate::app::AdmissionError::NeedsReconcile),
    });
    assert_eq!(q.reconciling_sessions(), vec![session(1)]);
    assert_eq!(
        state(&q, 1),
        IntentState::Waiting {
            reason: WaitReason::NeedsReconcile
        }
    );
    assert_eq!(
        q.apply(QueueEvent::ReconcileAcknowledged(session(1))),
        vec![QueueEffect::StartPlan(id)]
    );
    assert!(q.reconciling_sessions().is_empty());
}
