use super::*;
use crate::app::{AdmissionError, OperationId, RepoId, TabId};
use kagi_domain::plan::StateSummary;
use kagi_git::oplog::OpLogEntry;
use std::path::PathBuf;

fn session(n: u64) -> SessionId {
    SessionId {
        tab: TabId(n),
        incarnation: n,
    }
}
fn owner(n: u64) -> Attachment {
    Attachment {
        session: session(n),
        visit: 0,
        path: PathBuf::from("/repo"),
        worktree: Some(WorktreeId {
            repo: RepoId(PathBuf::from("/repo/.git")),
            git_dir: PathBuf::from("/repo/.git"),
        }),
    }
}
fn stamp(n: u64, operation: u64) -> OwnerStamp {
    OwnerStamp {
        session: session(n),
        visit: 0,
        operation: OperationId(operation),
    }
}
fn request() -> IntentRequest {
    IntentRequest::Checkout {
        target: "feature".into(),
    }
}
fn enqueue(queue: &mut IntentQueue, n: u64) -> IntentId {
    let effects = queue.apply(QueueEvent::Enqueue {
        owner: owner(n),
        request: request(),
    });
    match effects[0] {
        QueueEffect::Enqueued(id) => id,
        other => panic!("not enqueued: {other:?}"),
    }
}
fn state(queue: &IntentQueue, n: u64) -> IntentState {
    queue.intents(session(n)).unwrap().front().unwrap().state
}
fn summary() -> StateSummary {
    StateSummary {
        head: "main".into(),
        dirty: "clean".into(),
    }
}
fn entry() -> OpLogEntry {
    OpLogEntry::new(
        "checkout",
        "/repo",
        summary(),
        SemanticOutcome::Success { after: summary() },
    )
}
fn settle(
    queue: &mut IntentQueue,
    stamp: OwnerStamp,
    outcome: &SemanticOutcome,
    evidence: ExecutionEvidence,
    recorded: bool,
    reconcile: bool,
) -> Vec<QueueEffect> {
    let recording = if recorded {
        Recording::Appended {
            path: "/log".into(),
            entry: entry(),
        }
    } else {
        Recording::Failed {
            attempted: entry(),
            error: "disk full".into(),
        }
    };
    queue.apply(QueueEvent::AnchorSettled {
        stamp,
        receipt: Settlement {
            outcome,
            evidence,
            recording: &recording,
            reconcile_required: reconcile,
        },
    })
}
fn success() -> SemanticOutcome {
    SemanticOutcome::Success { after: summary() }
}
fn start(queue: &mut IntentQueue, n: u64, operation: u64) -> IntentId {
    let id = enqueue(queue, n);
    assert_eq!(state(queue, n), IntentState::Planning);
    assert_eq!(
        queue.apply(QueueEvent::PlanCompleted(id)),
        vec![QueueEffect::OpenConfirm(id)]
    );
    assert_eq!(state(queue, n), IntentState::AwaitingConfirm);
    assert_eq!(
        queue.apply(QueueEvent::Approve(id)),
        vec![QueueEffect::BeginAdmission(id)]
    );
    queue.apply(QueueEvent::Admission {
        id,
        result: Ok(stamp(n, operation)),
    });
    assert_eq!(
        state(queue, n),
        IntentState::Running {
            stamp: stamp(n, operation)
        }
    );
    id
}

#[test]
fn q1_running_anchor_settles_then_successors_execute_in_order() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    q.apply(QueueEvent::WriteStarted(stamp(1, 70)));
    let first = enqueue(&mut q, 1);
    let second = enqueue(&mut q, 1);
    assert_eq!(
        q.gate(session(1)),
        ChainGate::Armed {
            anchor: Some(ChainAnchor::ActiveWrite(stamp(1, 70)))
        }
    );
    assert_eq!(
        state(&q, 1),
        IntentState::Waiting {
            reason: WaitReason::WriteRunning
        }
    );
    settle(
        &mut q,
        stamp(1, 70),
        &success(),
        ExecutionEvidence::Verified,
        true,
        false,
    );
    assert_eq!(
        q.apply(QueueEvent::LeaseReleased(stamp(1, 70))),
        vec![QueueEffect::StartPlan(first)]
    );
    assert_eq!(
        q.apply(QueueEvent::PlanCompleted(first)),
        vec![QueueEffect::OpenConfirm(first)]
    );
    q.apply(QueueEvent::Approve(first));
    q.apply(QueueEvent::Admission {
        id: first,
        result: Ok(stamp(1, 71)),
    });
    assert_eq!(
        settle(
            &mut q,
            stamp(1, 71),
            &success(),
            ExecutionEvidence::Verified,
            true,
            false,
        ),
        vec![QueueEffect::Settled(first)]
    );
    assert_eq!(
        q.apply(QueueEvent::LeaseReleased(stamp(1, 71))),
        vec![QueueEffect::StartPlan(second)]
    );
}

#[test]
fn q2_each_non_success_trips_and_lists_entire_tail() {
    for outcome in [
        SemanticOutcome::Refused {
            blockers: vec!["blocked".into()],
        },
        SemanticOutcome::Failed {
            error: "failed".into(),
        },
        SemanticOutcome::Partial {
            after: summary(),
            error: "partial".into(),
        },
        SemanticOutcome::Unknown {
            after: summary(),
            evidence: "unknown".into(),
        },
    ] {
        let mut q = IntentQueue::new();
        q.apply(QueueEvent::OwnerReturned(session(1)));
        let first = start(&mut q, 1, 10);
        let second = enqueue(&mut q, 1);
        let third = enqueue(&mut q, 1);
        settle(
            &mut q,
            stamp(1, 10),
            &outcome,
            ExecutionEvidence::Verified,
            true,
            false,
        );
        let cancelled = q.cancelled(session(1)).unwrap();
        assert_eq!(
            cancelled
                .iter()
                .map(|i| (i.id, i.state))
                .collect::<Vec<_>>(),
            vec![
                (
                    second,
                    IntentState::Cancelled {
                        reason: CancelReason::ChainTripped {
                            by: ChainAnchor::QueuedHead(first)
                        }
                    }
                ),
                (
                    third,
                    IntentState::Cancelled {
                        reason: CancelReason::ChainTripped {
                            by: ChainAnchor::QueuedHead(first)
                        }
                    }
                ),
            ]
        );
        assert!(q.intents(session(1)).is_none());
        assert_eq!(q.gate(session(1)), ChainGate::default());
    }
}

#[test]
fn q2b_all_four_receipt_conditions_are_required() {
    for (evidence, recorded, reconcile) in [
        (ExecutionEvidence::Verified, false, false),
        (ExecutionEvidence::Unverified, true, false),
        (ExecutionEvidence::NotApplicable, true, false),
        (ExecutionEvidence::Verified, true, true),
    ] {
        let mut q = IntentQueue::new();
        q.apply(QueueEvent::OwnerReturned(session(1)));
        let first = start(&mut q, 1, 10);
        let second = enqueue(&mut q, 1);
        settle(
            &mut q,
            stamp(1, 10),
            &success(),
            evidence,
            recorded,
            reconcile,
        );
        assert_eq!(
            q.cancelled(session(1)).unwrap().back().unwrap().state,
            IntentState::Cancelled {
                reason: CancelReason::ChainTripped {
                    by: ChainAnchor::QueuedHead(first)
                }
            }
        );
        assert_eq!(q.cancelled(session(1)).unwrap().back().unwrap().id, second);
    }
}

#[test]
fn q4_every_wait_has_an_exhaustively_named_release_and_rechecks_other_heads() {
    let reasons = [
        WaitReason::WriteRunning,
        WaitReason::PlanSlotBusy,
        WaitReason::NeedsConfirmation,
        WaitReason::NeedsReconcile,
        WaitReason::RemoteLatched,
    ];
    let releases = [
        ReleaseEvent::LeaseReleased,
        ReleaseEvent::PlanSlotFreed,
        ReleaseEvent::OwnerReturned,
        ReleaseEvent::ReconcileAcknowledged,
        ReleaseEvent::RemoteLatchReleased,
    ];
    for (reason, release) in reasons.into_iter().zip(releases) {
        assert!(reason.released_by(release));
        let mut q = IntentQueue::new();
        q.apply(QueueEvent::OwnerReturned(session(1)));
        match reason {
            WaitReason::WriteRunning => {
                q.apply(QueueEvent::WriteStarted(stamp(2, 99)));
            }
            WaitReason::PlanSlotBusy => {
                // A single-shot family's plan job outside the queue.
                q.apply(QueueEvent::PlanSlotTaken);
            }
            WaitReason::NeedsConfirmation => {
                q.apply(QueueEvent::OwnerDeparted(session(1)));
            }
            WaitReason::NeedsReconcile => {
                q.reconciling.insert(session(1));
            }
            WaitReason::RemoteLatched => {
                // Another tab's lease-less pull: this tab may still queue.
                q.apply(QueueEvent::RemoteLatched(session(3)));
            }
        }
        let a = enqueue(&mut q, 1);
        let other = enqueue(&mut q, 2);
        assert_eq!(state(&q, 1), IntentState::Waiting { reason });
        let effects = match release {
            ReleaseEvent::LeaseReleased => q.apply(QueueEvent::LeaseReleased(stamp(2, 99))),
            ReleaseEvent::PlanSlotFreed => q.apply(QueueEvent::PlanSlotFreed(None)),
            ReleaseEvent::OwnerReturned => q.apply(QueueEvent::OwnerReturned(session(1))),
            ReleaseEvent::ReconcileAcknowledged => {
                q.apply(QueueEvent::ReconcileAcknowledged(session(1)))
            }
            ReleaseEvent::RemoteLatchReleased => q.apply(QueueEvent::RemoteLatchReleased),
            ReleaseEvent::ModalSlotFree | ReleaseEvent::RevalidationDone => unreachable!(),
        };
        assert!(
            effects.contains(&QueueEffect::StartPlan(a)),
            "{reason:?}: {effects:?}"
        );
        assert_eq!(q.intents(session(2)).unwrap().front().unwrap().id, other);
        assert_eq!(
            state(&q, 2),
            IntentState::Waiting {
                reason: WaitReason::PlanSlotBusy
            }
        );
    }
    for (event, setup) in [
        (ReleaseEvent::ModalSlotFree, QueueEvent::ModalSlotBusy),
        (
            ReleaseEvent::RevalidationDone,
            QueueEvent::RevalidationStarted(session(1)),
        ),
    ] {
        assert!(WaitReason::NeedsConfirmation.released_by(event));
        let mut q = IntentQueue::new();
        q.apply(QueueEvent::OwnerReturned(session(1)));
        q.apply(setup);
        let a = enqueue(&mut q, 1);
        assert_eq!(
            state(&q, 1),
            IntentState::Waiting {
                reason: WaitReason::NeedsConfirmation
            }
        );
        let effect = match event {
            ReleaseEvent::ModalSlotFree => q.apply(QueueEvent::ModalSlotFree),
            ReleaseEvent::RevalidationDone => q.apply(QueueEvent::RevalidationDone(session(1))),
            _ => unreachable!(),
        };
        assert!(effect.contains(&QueueEffect::StartPlan(a)));
    }
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let first = start(&mut q, 1, 10);
    enqueue(&mut q, 1);
    settle(
        &mut q,
        stamp(1, 10),
        &SemanticOutcome::Failed { error: "x".into() },
        ExecutionEvidence::Verified,
        true,
        false,
    );
    assert_eq!(q.gate(session(1)), ChainGate::default());
    q.apply(QueueEvent::LeaseReleased(stamp(1, 10)));
    assert_eq!(
        q.apply(QueueEvent::Enqueue {
            owner: owner(1),
            request: request()
        })
        .iter()
        .filter(|e| matches!(e, QueueEffect::StartPlan(_)))
        .count(),
        1
    );
    assert_eq!(q.gate(session(1)), ChainGate::Armed { anchor: None });
    assert!(first.0 > 0);
}

#[test]
fn q6_detach_discards_queue_but_never_releases_running_write() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    start(&mut q, 1, 9);
    let waiting = enqueue(&mut q, 1);
    let effects = q.apply(QueueEvent::OwnerDetached(session(1)));
    assert_eq!(
        effects,
        vec![QueueEffect::Cancelled(waiting, CancelReason::OwnerGone)]
    );
    assert_eq!(q.pipeline_count(), 1);
    assert!(matches!(state(&q, 1), IntentState::Running { .. }));
    assert!(q.cancelled(session(1)).is_none());
    assert_eq!(q.write, Some(stamp(1, 9)));
}

#[test]
fn q12_cancelled_planning_job_keeps_latch_until_terminal_callback() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let first = enqueue(&mut q, 1);
    let other = enqueue(&mut q, 2);
    assert_eq!(
        q.apply(QueueEvent::CancelHead(first)),
        vec![
            QueueEffect::CloseConfirm(first),
            QueueEffect::InvalidatePlan(first)
        ]
    );
    q.apply(QueueEvent::OwnerReturned(session(2)));
    assert!(q.cancelled(session(1)).is_some());
    assert_eq!(
        state(&q, 2),
        IntentState::Waiting {
            reason: WaitReason::PlanSlotBusy
        }
    );
    assert!(q.apply(QueueEvent::PlanCompleted(first)).is_empty());
    assert!(q.apply(QueueEvent::PlanSlotFreed(None)).is_empty());
    assert!(q
        .apply(QueueEvent::PlanSlotFreed(Some(IntentId(u64::MAX))))
        .is_empty());
    assert_eq!(
        state(&q, 2),
        IntentState::Waiting {
            reason: WaitReason::PlanSlotBusy
        }
    );
    assert_eq!(
        q.apply(QueueEvent::PlanSlotFreed(Some(first))),
        vec![QueueEffect::StartPlan(other)]
    );
    let effects = q.apply(QueueEvent::CancelHead(other));
    assert!(effects.contains(&QueueEffect::InvalidatePlan(other)));
    q.apply(QueueEvent::PlanSlotFreed(Some(other)));
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let confirming = enqueue(&mut q, 1);
    q.apply(QueueEvent::PlanCompleted(confirming));
    assert_eq!(state(&q, 1), IntentState::AwaitingConfirm);
    q.apply(QueueEvent::CancelHead(confirming));
    assert_eq!(
        q.cancelled(session(1)).unwrap().back().unwrap().state,
        IntentState::Cancelled {
            reason: CancelReason::UserRemoved
        }
    );
}

#[test]
fn q13_anchor_is_only_same_session_trackable_write_and_duplicate_receipts_are_ignored() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    q.apply(QueueEvent::WriteStarted(stamp(1, 99)));
    let head = enqueue(&mut q, 1);
    settle(
        &mut q,
        stamp(1, 100),
        &success(),
        ExecutionEvidence::Verified,
        true,
        false,
    );
    assert_eq!(
        state(&q, 1),
        IntentState::Waiting {
            reason: WaitReason::WriteRunning
        }
    );
    settle(
        &mut q,
        stamp(1, 99),
        &SemanticOutcome::Failed {
            error: "failure".into(),
        },
        ExecutionEvidence::Verified,
        true,
        false,
    );
    assert!(q.intents(session(1)).is_none());
    assert_eq!(q.cancelled(session(1)).unwrap().front().unwrap().id, head);
    assert_eq!(
        q.cancelled(session(1)).unwrap().front().unwrap().state,
        IntentState::Cancelled {
            reason: CancelReason::ChainTripped {
                by: ChainAnchor::ActiveWrite(stamp(1, 99))
            }
        }
    );
    settle(
        &mut q,
        stamp(1, 99),
        &success(),
        ExecutionEvidence::Verified,
        true,
        false,
    );
    assert!(q.intents(session(1)).is_none());
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    q.apply(QueueEvent::WriteStarted(stamp(2, 90)));
    let head = enqueue(&mut q, 1);
    assert_eq!(q.gate(session(1)), ChainGate::Armed { anchor: None });
    settle(
        &mut q,
        stamp(2, 90),
        &SemanticOutcome::Failed {
            error: "other".into(),
        },
        ExecutionEvidence::Verified,
        true,
        false,
    );
    assert_eq!(
        q.apply(QueueEvent::LeaseReleased(stamp(2, 90))),
        vec![QueueEffect::StartPlan(head)]
    );
    // Its own lease-less pull cannot be the tab's predecessor: refused.
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(2)));
    q.apply(QueueEvent::RemoteLatched(session(1)));
    assert!(q
        .apply(QueueEvent::Enqueue {
            owner: owner(1),
            request: request()
        })
        .contains(&QueueEffect::Rejected(EnqueueError::RemoteLatched)));
    // Another tab's pull only blocks, by name, until it ends.
    let waiting = enqueue(&mut q, 2);
    assert_eq!(
        state(&q, 2),
        IntentState::Waiting {
            reason: WaitReason::RemoteLatched
        }
    );
    assert_eq!(
        q.apply(QueueEvent::RemoteLatchReleased),
        vec![QueueEffect::StartPlan(waiting)]
    );
    assert!(matches!(
        q.apply(QueueEvent::Enqueue {
            owner: owner(1),
            request: request()
        })[0],
        QueueEffect::Enqueued(_)
    ));
}

#[test]
fn bounded_capacity_cancel_history_remove_one_and_auto_fetch() {
    let mut q = IntentQueue::new();
    let first = enqueue(&mut q, 1);
    assert!(!q.auto_fetch_allowed(session(1)));
    for _ in 1..MAX_QUEUED_INTENTS {
        enqueue(&mut q, 1);
    }
    assert_eq!(
        q.apply(QueueEvent::Enqueue {
            owner: owner(1),
            request: request()
        }),
        vec![QueueEffect::Rejected(EnqueueError::CapacityRejected)]
    );
    q.apply(QueueEvent::RemoveOne(first));
    assert_eq!(q.intents(session(1)).unwrap().len(), 15);
    assert_eq!(q.cancelled(session(1)).unwrap().len(), 1);
    let mut inserted = Vec::new();
    for _ in 0..40 {
        let id = enqueue(&mut q, 2);
        inserted.push(id);
        q.apply(QueueEvent::RemoveOne(id));
    }
    assert_eq!(
        q.cancelled(session(2)).unwrap().len(),
        MAX_CANCELLED_INTENTS
    );
    assert_eq!(
        q.cancelled(session(2)).unwrap().front().unwrap().id,
        inserted[8]
    );
    assert_eq!(
        q.cancelled(session(2)).unwrap().back().unwrap().id,
        inserted[39]
    );
    assert!(q.auto_fetch_allowed(session(2)));
    q.apply(QueueEvent::DismissCancelled(session(2)));
    assert!(q.cancelled(session(2)).is_none());
}

#[test]
fn admission_busy_waits_for_lease_release_instead_of_replanning_immediately() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let id = enqueue(&mut q, 1);
    q.apply(QueueEvent::PlanCompleted(id));
    q.apply(QueueEvent::Approve(id));
    let effects = q.apply(QueueEvent::Admission {
        id,
        result: Err(crate::app::AdmissionError::Busy),
    });
    assert_eq!(
        effects,
        vec![
            QueueEffect::CloseConfirm(id),
            QueueEffect::InvalidatePlan(id)
        ]
    );
    assert_eq!(
        state(&q, 1),
        IntentState::Waiting {
            reason: WaitReason::WriteRunning
        }
    );
    assert_eq!(
        q.apply(QueueEvent::LeaseReleased(stamp(2, 88))),
        vec![QueueEffect::StartPlan(id)]
    );
}

#[test]
fn departure_requeues_unanswered_confirmation_and_releases_next_candidate() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let first = enqueue(&mut q, 1);
    q.apply(QueueEvent::PlanCompleted(first));
    let other = enqueue(&mut q, 2);
    let effects = q.apply(QueueEvent::OwnerReturned(session(2)));
    assert!(effects.contains(&QueueEffect::CloseConfirm(first)));
    assert!(effects.contains(&QueueEffect::InvalidatePlan(first)));
    assert!(effects.contains(&QueueEffect::StartPlan(other)));
    assert_eq!(
        state(&q, 1),
        IntentState::Waiting {
            reason: WaitReason::NeedsConfirmation
        }
    );
}

#[test]
fn identity_mismatch_trips_chain_without_retargeting_worktree() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let first = enqueue(&mut q, 1);
    let tail = enqueue(&mut q, 1);
    let effects = q.apply(QueueEvent::IdentityChanged(first));
    assert_eq!(
        effects,
        vec![
            QueueEffect::CloseConfirm(first),
            QueueEffect::InvalidatePlan(first)
        ]
    );
    let cancelled = q.cancelled(session(1)).unwrap();
    assert_eq!(
        cancelled[0].state,
        IntentState::Cancelled {
            reason: CancelReason::IdentityChanged
        }
    );
    assert_eq!(cancelled[1].id, tail);
    assert_eq!(
        cancelled[1].state,
        IntentState::Cancelled {
            reason: CancelReason::ChainTripped {
                by: ChainAnchor::QueuedHead(first)
            }
        }
    );
    assert!(q.intents(session(1)).is_none());
}

mod extra;
mod lifecycle;
mod q9;
