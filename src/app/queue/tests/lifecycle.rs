//! Lifecycle edges of the anchor, the modal slot and an admission in flight.
use super::*;

#[test]
fn a_settled_write_whose_lease_outlives_it_never_anchors_a_new_chain() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let w = stamp(1, 50);
    q.apply(QueueEvent::WriteStarted(w));
    // Unknown: the receipt arrives once, the lease stays until reconciled.
    settle(
        &mut q,
        w,
        &SemanticOutcome::Unknown {
            after: summary(),
            evidence: "child not reaped".into(),
        },
        ExecutionEvidence::Unverified,
        true,
        true,
    );
    let x = enqueue(&mut q, 1);
    assert_eq!(q.gate(session(1)), ChainGate::Armed { anchor: None });
    assert_eq!(
        state(&q, 1),
        IntentState::Waiting {
            reason: WaitReason::WriteRunning
        }
    );
    q.apply(QueueEvent::ReconcileAcknowledged(session(1)));
    assert_eq!(
        q.apply(QueueEvent::LeaseReleased(Some(w))),
        vec![QueueEffect::StartPlan(x)]
    );
}

#[test]
fn removing_a_planning_head_leaves_a_modal_it_never_opened() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let a = enqueue(&mut q, 1);
    assert_eq!(state(&q, 1), IntentState::Planning);
    // The user opened an unrelated confirmation meanwhile.
    q.apply(QueueEvent::ModalSlotBusy);
    q.apply(QueueEvent::PlanError(a));
    q.apply(QueueEvent::PlanSlotFreed(Some(a)));
    enqueue(&mut q, 1);
    assert_eq!(
        state(&q, 1),
        IntentState::Waiting {
            reason: WaitReason::NeedsConfirmation
        },
        "the slot is still the user's"
    );
}

/// The head through Approve: `Admitting`, no lease yet.
fn admitting(q: &mut IntentQueue, n: u64) -> IntentId {
    let id = enqueue(q, n);
    q.apply(QueueEvent::PlanCompleted(id));
    q.apply(QueueEvent::Approve(id));
    assert_eq!(state(q, n), IntentState::Admitting);
    id
}

#[test]
fn cancel_all_keeps_an_admission_in_flight() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let a = admitting(&mut q, 1);
    enqueue(&mut q, 1);
    q.apply(QueueEvent::CancelAll(session(1)));
    assert_eq!(q.intents(session(1)).unwrap().len(), 1);
    assert_eq!(state(&q, 1), IntentState::Admitting);
    assert_eq!(q.cancelled(session(1)).unwrap().len(), 1);
    let w = stamp(1, 60);
    q.apply(QueueEvent::Admission {
        id: a,
        result: Ok(w),
    });
    assert_eq!(state(&q, 1), IntentState::Running { stamp: w });
}

#[test]
fn detach_during_admission_keeps_a_late_write_tracked() {
    for admitted in [true, false] {
        let mut q = IntentQueue::new();
        q.apply(QueueEvent::OwnerReturned(session(1)));
        let a = admitting(&mut q, 1);
        q.apply(QueueEvent::OwnerReturned(session(2)));
        let x = enqueue(&mut q, 2);
        let effects = q.apply(QueueEvent::OwnerDetached(session(1)));
        assert!(effects.contains(&QueueEffect::CloseConfirm(a)));
        assert_eq!(
            state(&q, 2),
            IntentState::Waiting {
                reason: WaitReason::PlanSlotBusy
            },
            "the admission still owns the pipeline"
        );
        let w = stamp(1, 61);
        if admitted {
            q.apply(QueueEvent::Admission {
                id: a,
                result: Ok(w),
            });
            assert_eq!(
                state(&q, 2),
                IntentState::Waiting {
                    reason: WaitReason::WriteRunning
                }
            );
            settle(
                &mut q,
                w,
                &success(),
                ExecutionEvidence::Verified,
                true,
                false,
            );
            assert_eq!(
                q.apply(QueueEvent::LeaseReleased(Some(w))),
                vec![QueueEffect::StartPlan(x)]
            );
        } else {
            let effects = q.apply(QueueEvent::Admission {
                id: a,
                result: Err(AdmissionError::Busy),
            });
            assert!(effects.contains(&QueueEffect::Cancelled(a, CancelReason::OwnerGone)));
            assert!(effects.contains(&QueueEffect::StartPlan(x)));
        }
        assert!(q.intents(session(1)).is_none());
        assert!(
            q.cancelled(session(1)).is_none(),
            "nothing listed for a closed tab"
        );
    }
}
