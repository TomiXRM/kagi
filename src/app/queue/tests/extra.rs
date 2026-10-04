use super::*;

#[test]
fn remove_one_does_not_cancel_the_suffix_or_the_other_session() {
    let mut q = IntentQueue::new();
    let first = enqueue(&mut q, 1);
    let next = enqueue(&mut q, 1);
    let other = enqueue(&mut q, 2);
    q.apply(QueueEvent::RemoveOne(first));
    assert_eq!(q.intents(session(1)).unwrap().front().unwrap().id, next);
    assert_eq!(q.intents(session(2)).unwrap().front().unwrap().id, other);
    assert_eq!(q.cancelled(session(1)).unwrap().len(), 1);
    assert_eq!(
        q.gate(session(1)),
        ChainGate::Armed {
            anchor: Some(ChainAnchor::QueuedHead(next))
        }
    );
}

#[test]
fn auto_fetch_is_neither_anchor_nor_chain_failure() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    q.apply(QueueEvent::UntrackedWriteStarted { owner: None });
    let id = enqueue(&mut q, 1);
    assert_eq!(q.gate(session(1)), ChainGate::Armed { anchor: None });
    assert!(!q.auto_fetch_allowed(session(1)));
    assert_eq!(
        state(&q, 1),
        IntentState::Waiting {
            reason: WaitReason::WriteRunning
        }
    );
    assert_eq!(
        q.apply(QueueEvent::LeaseReleased(None)),
        vec![QueueEffect::StartPlan(id)]
    );
    assert!(q.cancelled(session(1)).is_none());
}

#[test]
fn stale_admission_cancels_chain_and_closes_its_confirmation() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let first = enqueue(&mut q, 1);
    let second = enqueue(&mut q, 1);
    q.apply(QueueEvent::PlanCompleted(first));
    q.apply(QueueEvent::Approve(first));
    assert_eq!(
        q.apply(QueueEvent::Admission {
            id: first,
            result: Err(crate::app::AdmissionError::StaleApproval)
        }),
        vec![
            QueueEffect::CloseConfirm(first),
            QueueEffect::InvalidatePlan(first)
        ]
    );
    assert_eq!(
        q.cancelled(session(1)).unwrap()[0].state,
        IntentState::Cancelled {
            reason: CancelReason::StaleApproval
        }
    );
    assert_eq!(q.cancelled(session(1)).unwrap()[1].id, second);
    assert!(q.intents(session(1)).is_none());
}

#[test]
fn capacity_counts_sixteen_pending_even_with_a_running_head() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let running = start(&mut q, 1, 42);
    assert_eq!(q.queued_count(session(1)), 0);
    for expected in 1..=MAX_QUEUED_INTENTS {
        let id = enqueue(&mut q, 1);
        assert_eq!(q.queued_count(session(1)), expected);
        assert_eq!(q.intents(session(1)).unwrap().back().unwrap().id, id);
    }
    assert_eq!(q.intents(session(1)).unwrap().front().unwrap().id, running);
    assert_eq!(
        q.apply(QueueEvent::Enqueue {
            owner: owner(1),
            request: request()
        }),
        vec![QueueEffect::Rejected(EnqueueError::CapacityRejected)]
    );
    assert_eq!(q.queued_count(session(1)), MAX_QUEUED_INTENTS);
}

#[test]
fn dismissing_old_cancellations_keeps_a_new_chains_active_anchor() {
    let mut q = IntentQueue::new();
    let removed = enqueue(&mut q, 1);
    q.apply(QueueEvent::RemoveOne(removed));
    assert_eq!(q.cancelled(session(1)).unwrap().len(), 1);
    q.apply(QueueEvent::WriteStarted(stamp(1, 91)));
    let head = enqueue(&mut q, 1);
    let tail = enqueue(&mut q, 1);
    q.apply(QueueEvent::DismissCancelled(session(1)));
    assert_eq!(
        q.gate(session(1)),
        ChainGate::Armed {
            anchor: Some(ChainAnchor::ActiveWrite(stamp(1, 91)))
        }
    );
    settle(
        &mut q,
        stamp(1, 91),
        &SemanticOutcome::Failed {
            error: "failed".into(),
        },
        ExecutionEvidence::Verified,
        true,
        false,
    );
    assert_eq!(
        q.cancelled(session(1))
            .unwrap()
            .iter()
            .map(|i| i.id)
            .collect::<Vec<_>>(),
        vec![head, tail]
    );
}
