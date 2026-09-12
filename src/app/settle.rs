//! Settlement: turning one family's completion into the state change and the
//! deliveries it earns (ADR-0196 決定 2 §2.5).
//!
//! Split out of `flow.rs` on the lifecycle boundary — that file owns approval
//! and admission, this one owns the other end. `apply` is the single place a
//! write becomes settled: exactly once per operation, releasing the lease only
//! on a proven stop, registering the reconcile requirement when there is one,
//! and routing the completion by the [`OwnerStamp`] frozen at admission.
use super::*;

/// Whether the writer is proven stopped, and the child that must still be
/// accounted for when it is not.
///
/// An unproven termination is the one case where the scope stays reserved
/// (ADR-0175). It is not a dead end: either the executor already saw the child
/// go (`child_stopped`), or it hands over the pid so a later reconcile read can
/// prove it went away.
fn termination(
    result: &Result<kagi_git::OperationOutcome, kagi_git::GitError>,
) -> (bool, Option<u32>) {
    match result {
        Err(kagi_git::GitError::TerminationUnknown(t)) => (t.child_stopped, t.pid),
        _ => (true, None),
    }
}

pub fn apply(s: &mut Sessions, completion: impl Into<Completion>) -> Vec<Delivery> {
    // The auto-stash a pull left behind: the reconcile read must account for
    // that entry, and only the report carries what the backend saw of it.
    let mut pull_recovery: Option<kagi_git::backend::stash::StashEvidence> = None;
    // The child of an unproven termination, if there is one to account for.
    let mut unaccounted_child: Option<u32> = None;
    let (id, report, stopped, remote_recovery) = match completion.into() {
        Completion::Remove(c) => {
            let c = *c;
            let stopped = !c.report.progress.termination_unknown;
            (
                c.id,
                ExecutionReport {
                    recording: c.report.recording.clone(),
                    evidence: FamilyEvidence::Remove(c.report),
                },
                stopped,
                None,
            )
        }
        Completion::Stash(c) => match c.report {
            StashExecutionReport::Local(report) => (
                c.id,
                ExecutionReport {
                    recording: report.recording.clone(),
                    evidence: FamilyEvidence::Stash(*report),
                },
                true,
                None,
            ),
            StashExecutionReport::Remote(report) => {
                let stopped = report.evidence.stopped;
                let recovery = Some(report.evidence.clone());
                (
                    c.id,
                    ExecutionReport {
                        recording: report.recording.clone(),
                        evidence: FamilyEvidence::RemoteStash(*report),
                    },
                    stopped,
                    recovery,
                )
            }
        },
        Completion::Conflict(c) => {
            let c = *c;
            (
                c.id,
                ExecutionReport {
                    recording: c.report.recording.clone(),
                    evidence: FamilyEvidence::Conflict(c.report),
                },
                true,
                None,
            )
        }
        // A CLI writer whose termination is unconfirmed may still be running:
        // the scope stays reserved (ADR-0175), exactly as `complete_git` does.
        Completion::Run(c) => {
            let c = *c;
            let (stopped, child) = termination(&c.report.result);
            unaccounted_child = child;
            (
                c.id,
                ExecutionReport {
                    recording: c.report.recording.clone(),
                    evidence: FamilyEvidence::Run(c.report),
                },
                stopped,
                None,
            )
        }
        // The workflow settles on the receipt that decided it — not on the
        // last child that ran. A pull that failed and whose stash was then
        // restored is a failed pull, not a successful pop (ADR-0196 決定 5).
        Completion::Pull(c) => {
            let c = *c;
            pull_recovery = c.report.terminal.stash.clone();
            let decisive = c.report.decisive().clone();
            let (stopped, child) = termination(&decisive.result);
            unaccounted_child = child;
            (
                c.id,
                ExecutionReport {
                    recording: decisive.recording,
                    evidence: FamilyEvidence::Pull(c.report),
                },
                stopped,
                None,
            )
        }
    };
    if s.settled.contains(&id) {
        return vec![];
    }
    let Some(owner) = s.operations.remove(&id) else {
        return vec![];
    };
    s.settled.insert(id);
    if stopped {
        s.release_lease(&owner.plan.scope(), id);
    }
    // An unconfirmed termination is itself a reconcile requirement: the lease
    // is retained, so there must be an entry to acknowledge it against.
    if matches!(
        report.recording.entry().outcome,
        kagi_git::OpOutcome::Unknown { .. }
    ) || !stopped
    {
        s.reconcile.insert(
            id,
            ReconcileEntry {
                plan: owner.plan.clone(),
                stopped,
                remote: remote_recovery,
                pull: pull_recovery,
                child: unaccounted_child,
            },
        );
    }
    let mut deliveries = vec![];
    match (&owner.plan, &report.evidence) {
        (Planned::Remove { plan, .. }, FamilyEvidence::Remove(r)) => {
            let manager = InvalidTarget {
                worktree: plan.worktree.clone(),
                path: plan.repo.clone(),
            };
            let target = InvalidTarget {
                worktree: plan.worktree_id.clone(),
                path: plan.target.clone(),
            };
            deliveries.push(Delivery::Invalidate(manager.clone()));
            s.stale.insert(manager.worktree.clone());
            deliveries.push(if r.target_exists == Some(false) {
                Delivery::RemovedTarget(target.clone())
            } else {
                Delivery::Invalidate(target.clone())
            });
            s.stale.insert(target.worktree);
        }
        (Planned::Stash { plan, request, .. }, FamilyEvidence::Stash(r)) => {
            let manager = InvalidTarget {
                worktree: plan.worktree.clone(),
                path: plan.repo.clone(),
            };
            if matches!(
                plan.action,
                StashAction::Apply { .. } | StashAction::Pop { .. }
            ) && !r.evidence.conflicts.is_empty()
            {
                // The conflict belongs to the session that approved the stash,
                // and to the *visit* it approved it in. A closed owner leaves no
                // payload behind, and one the user has since left gets no new
                // proposal — the next visit must re-observe it live (#557).
                let session = request.owner.session;
                if let (Some(oid), true) = (
                    &r.evidence.oid,
                    s.visit(session) == Some(request.owner.visit),
                ) {
                    s.clear_stash_conflict(session);
                    s.stash_conflicts.insert(
                        session,
                        StashConflict {
                            operation: id,
                            oid: oid.clone(),
                            identity: r.evidence.conflict_identity.clone(),
                            pending: false,
                            visit: request.owner.visit,
                        },
                    );
                }
            }
            deliveries.push(Delivery::Invalidate(manager.clone()));
            s.stale.insert(manager.worktree.clone());
        }
        (Planned::RemoteStash { .. }, FamilyEvidence::RemoteStash(_)) => {}
        (Planned::Conflict { request, .. }, FamilyEvidence::Conflict(report)) => {
            let matches_in_flight = matches!(
                s.conflict_states.get(&request.owner.session),
                Some(ConflictOwnerState::InFlight { operation, revision })
                    if *operation == id && revision == &report.evidence.before.revision
            );
            if s.is_attached(request.owner.session) && matches_in_flight {
                s.conflict_states.insert(
                    request.owner.session,
                    ConflictOwnerState::Settled(report.evidence.after.clone()),
                );
            }
            let target = InvalidTarget {
                worktree: request
                    .owner
                    .worktree
                    .clone()
                    .expect("local conflict owner has a worktree"),
                path: request.owner.path.clone(),
            };
            s.stale.insert(target.worktree.clone());
            deliveries.push(Delivery::Invalidate(target));
        }
        (Planned::Run(request), FamilyEvidence::Run(_)) => {
            let target = InvalidTarget {
                worktree: request
                    .owner
                    .worktree
                    .clone()
                    .expect("a run owner is a local worktree tab"),
                path: request.owner.path.clone(),
            };
            s.stale.insert(target.worktree.clone());
            deliveries.push(Delivery::Invalidate(target));
        }
        (Planned::Pull(request), FamilyEvidence::Pull(_)) => {
            let target = InvalidTarget {
                worktree: request
                    .owner
                    .worktree
                    .clone()
                    .expect("a pull owner is a local worktree tab"),
                path: request.owner.path.clone(),
            };
            s.stale.insert(target.worktree.clone());
            deliveries.push(Delivery::Invalidate(target));
        }
        _ => unreachable!("completion family is fixed by its owned job"),
    }
    // Shared refs / stash / worktree administration make every *open* sibling
    // worktree of the same repository stale, not only the one that was written.
    // Index- and working-tree-only changes stay scoped to their target (#482).
    if owner.plan.changes_shared_refs() {
        let common_dir = match &owner.plan {
            Planned::Remove { plan, .. } => &plan.common_dir,
            Planned::Stash { plan, .. } => &plan.common_dir,
            Planned::RemoteStash { .. } => unreachable!("remote refs have no local siblings"),
            Planned::Conflict { .. } => unreachable!("conflict writes are worktree-local"),
            Planned::Run(request) => &request.repo,
            Planned::Pull(request) => &request.repo,
        };
        let delivered: Vec<_> = deliveries
            .iter()
            .filter_map(|delivery| match delivery {
                Delivery::Invalidate(t) | Delivery::RemovedTarget(t) => Some(t.worktree.clone()),
                Delivery::Completed { .. } | Delivery::RemoteCompleted { .. } => None,
            })
            .collect();
        for worktree in s.siblings_of(common_dir) {
            if delivered.contains(&worktree) {
                continue;
            }
            let path = s
                .path_of(&worktree)
                .unwrap_or_else(|| worktree.git_dir.clone());
            s.stale.insert(worktree.clone());
            deliveries.push(Delivery::Invalidate(InvalidTarget { worktree, path }));
        }
    }
    // The stamp is the one frozen at admission — never re-derived from the
    // sessions map here, which may have moved on (ADR-0196 決定 3).
    let stamp = owner.stamp;
    deliveries.push(match owner.attachment {
        OwnerAttachment::Local(attachment) => Delivery::Completed {
            id,
            attachment,
            stamp,
            report: Box::new(report),
        },
        OwnerAttachment::Remote(attachment) => Delivery::RemoteCompleted {
            id,
            attachment,
            stamp,
            report: Box::new(report),
        },
    });
    deliveries
}
