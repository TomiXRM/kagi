//! Q9: whatever the event order, the window never has two pipeline owners
//! and a background tab never plans (ADR-0204 決定 1).
use super::*;
#[test]
fn q9_deterministic_event_sequences_never_create_two_pipeline_owners() {
    let mut q = IntentQueue::new();
    q.apply(QueueEvent::OwnerReturned(session(1)));
    let mut seed = 0x355u64;
    for step in 0..2000 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let pick = (seed >> 12) % 3 + 1;
        // The pipeline head, if any, in a stable session order.
        let mut heads: Vec<_> = q
            .per_session
            .iter()
            .filter_map(|(session, queue)| queue.front().map(|h| (*session, h.id, h.state)))
            .collect();
        heads.sort_unstable_by_key(|(session, ..)| session.tab.0);
        let active_head = heads.iter().copied().find(|(_, _, state)| {
            matches!(
                state,
                IntentState::Planning
                    | IntentState::AwaitingConfirm
                    | IntentState::Admitting
                    | IntentState::Running { .. }
            )
        });
        match seed % 26 {
            0..=2 => {
                q.apply(QueueEvent::Enqueue {
                    owner: owner(pick),
                    request: request(),
                });
            }
            3 => {
                q.apply(QueueEvent::OwnerReturned(session(pick)));
            }
            4 => {
                q.apply(QueueEvent::OwnerDeparted(session(pick)));
            }
            5 => {
                let job = q.planning_job;
                q.apply(QueueEvent::PlanSlotFreed(job));
            }
            6 => {
                q.apply(QueueEvent::ModalSlotFree);
            }
            7 => {
                q.apply(QueueEvent::RevalidationDone(session(pick)));
            }
            8 => {
                q.apply(QueueEvent::ReconcileAcknowledged(session(pick)));
            }
            9 => {
                q.apply(QueueEvent::RemoteLatched(session(pick)));
            }
            10 => {
                q.apply(QueueEvent::RemoteLatchReleased);
            }
            11 => {
                q.apply(QueueEvent::ModalSlotBusy);
            }
            12 => {
                q.apply(QueueEvent::RevalidationStarted(session(pick)));
            }
            13 => {
                q.apply(QueueEvent::OwnerDetached(session(pick)));
            }
            14 => {
                q.apply(QueueEvent::CancelAll(session(pick)));
            }
            15 => {
                q.apply(QueueEvent::PlanSlotTaken);
            }
            16 => {
                if let Some((_, id, _)) = heads.get((pick as usize) % heads.len().max(1)) {
                    let event = match (seed >> 20) % 4 {
                        0 => QueueEvent::CancelHead(*id),
                        1 => QueueEvent::PlanError(*id),
                        2 => QueueEvent::Reject(*id),
                        _ => QueueEvent::IdentityChanged(*id),
                    };
                    q.apply(event);
                }
            }
            17 => {
                if let Some((_, id, IntentState::Admitting)) = active_head {
                    let error = match (seed >> 20) % 4 {
                        0 => AdmissionError::Busy,
                        1 => AdmissionError::NeedsReconcile,
                        2 => AdmissionError::StaleApproval,
                        _ => AdmissionError::Identity("moved".into()),
                    };
                    q.apply(QueueEvent::Admission {
                        id,
                        result: Err(error),
                    });
                }
            }
            18 => {
                if let Some((_, _, IntentState::Running { stamp: writer })) = active_head {
                    // A non-Success settle whose lease may outlive it.
                    let outcome = match (seed >> 20) % 3 {
                        0 => SemanticOutcome::Failed { error: "x".into() },
                        _ => SemanticOutcome::Unknown {
                            after: summary(),
                            evidence: "x".into(),
                        },
                    };
                    settle(
                        &mut q,
                        writer,
                        &outcome,
                        ExecutionEvidence::Unverified,
                        (seed >> 22).is_multiple_of(2),
                        true,
                    );
                    if (seed >> 24).is_multiple_of(2) {
                        q.apply(QueueEvent::LeaseReleased(Some(writer)));
                    }
                }
            }
            19 => {
                if let Some(id) = q
                    .per_session
                    .get(&session(pick))
                    .and_then(|queue| queue.back())
                    .map(|i| i.id)
                {
                    q.apply(QueueEvent::RemoveOne(id));
                }
            }
            20 => {
                q.apply(QueueEvent::DismissCancelled(session(pick)));
            }
            _ => {
                if let Some((session, id, phase)) = active_head {
                    match phase {
                        IntentState::Planning => {
                            q.apply(QueueEvent::PlanCompleted(id));
                        }
                        IntentState::AwaitingConfirm => {
                            q.apply(QueueEvent::Approve(id));
                        }
                        IntentState::Admitting => {
                            q.apply(QueueEvent::Admission {
                                id,
                                result: Ok(stamp(session.tab.0, step + 1000)),
                            });
                        }
                        IntentState::Running { stamp: writer } => {
                            settle(
                                &mut q,
                                writer,
                                &success(),
                                ExecutionEvidence::Verified,
                                true,
                                false,
                            );
                            q.apply(QueueEvent::LeaseReleased(Some(writer)));
                        }
                        IntentState::Queued
                        | IntentState::Waiting { .. }
                        | IntentState::Settled
                        | IntentState::Cancelled { .. } => unreachable!(),
                    }
                }
            }
        }
        let owner = q.active;
        for (session, queue) in &q.per_session {
            if Some(*session) != owner {
                assert!(
                    !queue.iter().any(|i| i.state == IntentState::Planning),
                    "background planned on step {step}"
                );
            }
        }
        assert!(
            q.pipeline_count() <= 1,
            "multiple pipeline intents on step {step}"
        );
    }
}
