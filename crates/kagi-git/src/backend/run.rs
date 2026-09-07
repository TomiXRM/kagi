//! Shared execution pipeline and the single finalization boundary.
use super::*;

impl Backend {
    /// The receipt belongs to this invocation, never to a later oplog tail read.
    pub fn run_recorded(&mut self, op: &Operation, plan: &OperationPlan) -> recording::RunReport {
        self.run_recorded_with_events(op, plan, None, |_| {})
    }

    pub(super) fn run_recorded_with_events(
        &mut self,
        op: &Operation,
        plan: &OperationPlan,
        fault: Option<stash::StashFaultPoint>,
        mut event: impl FnMut(stash::StashEvent),
    ) -> recording::RunReport {
        let mut partial_after = None;
        let mut backup_refs = Vec::new();
        let mut evidence = stash::StashEvidence::default();
        let action = stash::StashAction::from_operation(op);
        let result = if let Some(action) = &action {
            let attempted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let result = self.run_execution(
                    op,
                    plan,
                    &mut partial_after,
                    &mut evidence,
                    fault,
                    &mut backup_refs,
                );
                if result.is_ok() {
                    event(stash::StashEvent::Executed {
                        action: action.clone(),
                        oid: evidence.oid.clone(),
                        conflicts: evidence.conflicts.len(),
                    });
                    if matches!(fault, Some(stash::StashFaultPoint::AfterMutation)) {
                        panic!("stash fault after mutation");
                    }
                    let verification =
                        if matches!(fault, Some(stash::StashFaultPoint::VerifyFailure)) {
                            Err(GitError::Other("injected verification failure".into()))
                        } else {
                            self.verify_stash_run(action, plan, &mut evidence)
                        };
                    match verification {
                        Ok((dirty, count)) => event(stash::StashEvent::Verified { dirty, count }),
                        Err(error) => {
                            event(stash::StashEvent::VerifyFailed {
                                error: error.to_string(),
                            });
                            return Err(error);
                        }
                    }
                }
                result
            }));
            match attempted {
                Ok(result) => result,
                Err(_) => {
                    evidence.unknown = evidence.started;
                    evidence
                        .observations
                        .push("executor unwound; side effects may have occurred".into());
                    Err(GitError::Other("stash executor panicked".into()))
                }
            }
        } else {
            self.run_execution(
                op,
                plan,
                &mut partial_after,
                &mut evidence,
                None,
                &mut backup_refs,
            )
        };
        let outcome = if action.is_some() {
            stash::stash_outcome(&result, plan, &evidence)
        } else {
            oplog_outcome_from(&result, &plan.predicted, partial_after)
        };
        if let Ok(OperationOutcome::Discard(d)) = &result {
            backup_refs.extend(d.backups.iter().map(|b| b.reference.clone()));
        }
        let recording = self.record_run_oplog_with_backups(
            op.oplog_name(),
            &plan.current,
            outcome,
            backup_refs,
        );
        recording::RunReport {
            result,
            recording,
            stash: action.map(|_| evidence),
        }
    }

    /// Execution only. Every exit is finalized by run_recorded, exactly once.
    fn run_execution(
        &mut self,
        op: &Operation,
        plan: &OperationPlan,
        partial_after: &mut Option<ops::StateSummary>,
        evidence: &mut stash::StashEvidence,
        fault: Option<stash::StashFaultPoint>,
        backup_refs: &mut Vec<String>,
    ) -> Result<OperationOutcome, GitError> {
        // ── Owner-trust gate (ADR-0160). libgit2 does not enforce
        // safe.directory, so a foreign-owned repo opened via git2 reaches here
        // untrusted. Reads already ran; every *mutating* op stops here until the
        // user grants trust. Headless never grants trust, so it stays read-only.
        if let Err(e) = self.require_trust() {
            evidence.stop = Some(stash::StashStopReason::Untrusted);
            return Err(e);
        }
        if !plan.blockers.is_empty() {
            evidence.plan_blocked = true;
            evidence.stop = Some(stash::StashStopReason::PlanBlocked);
            return Err(GitError::Other("plan has blockers".into()));
        }
        if stash::StashAction::from_operation(op).is_some() {
            let identity = plan
                .stash_identity
                .as_ref()
                .ok_or_else(|| GitError::Other("missing stash identity".into()))?;
            let selected = match op {
                Operation::StashApply { index }
                | Operation::StashPop { index }
                | Operation::StashDrop { index } => Some(*index),
                _ => None,
            };
            if selected != identity.selected {
                return Err(GitError::Other(
                    "stash target differs from approved plan".into(),
                ));
            }
            evidence.oid = selected.and_then(|i| identity.oids.get(i).cloned());
        }

        // ── Preflight: refuse if the repo changed between plan and execute. ──
        let preflight = match op {
            Operation::StashPush { .. }
            | Operation::StashApply { .. }
            | Operation::StashPop { .. }
            | Operation::StashDrop { .. } => {
                // Stash ops also verify the stash list hasn't shifted.
                self.preflight_check_stash(plan, plan.stash_count_at_plan())
            }
            // Discard/DeleteBranch already re-plan in the legacy execute path;
            // keep a HEAD preflight for the rest. Commit is non-mutating of
            // HEAD in a way the preflight detects (it advances HEAD), so it is
            // also gated: a confirmed commit plan captures the pre-commit HEAD.
            _ => self.preflight_check(plan),
        }
        .and_then(|()| {
            // A display plan is not authority to omit required safety fields. Derive
            // the family requirements again before any snapshot or mutation (#502).
            let fresh = self.plan(op)?;
            if !fresh.blockers.is_empty() {
                return Err(GitError::Other(
                    fresh
                        .blockers
                        .iter()
                        .map(|blocker| blocker.message_en())
                        .collect::<Vec<_>>()
                        .join("; "),
                ));
            }
            if fresh.title != plan.title
                || ops::remote_source_tip(&fresh) != ops::remote_source_tip(plan)
                || ops::plan_worktree_config_sha(&fresh) != ops::plan_worktree_config_sha(plan)
                || fresh.destructive != plan.destructive
                || fresh.worktree_digest.is_some() != plan.worktree_digest.is_some()
            {
                return Err(GitError::Other(
                    "plan safety requirements differ; please re-plan".into(),
                ));
            }

            Ok(())
        });
        if let Err(e) = preflight {
            if stash::StashAction::from_operation(op).is_some() {
                evidence.preflight_error = Some(e.to_string());
                evidence.stop = Some(stash::StashStopReason::Preflight);
            }
            // ADR-0149: a preflight refusal is still a failed attempt — record
            // it so no write path has an unlogged hole, then propagate.
            return Err(GitError::Preflight(Box::new(e)));
        }

        // ── issue #393: bind worktree-config execution to the exact content the
        // plan displayed. For create/open-worktree, re-hash `.kagi/worktree.toml`
        // and refuse if it changed since planning (display A / execute B TOCTOU).
        if let (true, Some(expected), Some(root)) = (
            matches!(
                op,
                Operation::CreateWorktree { .. } | Operation::OpenWorktreeForBranch { .. }
            ),
            ops::plan_worktree_config_sha(plan),
            self.repo.workdir(),
        ) {
            ops::verify_worktree_config_sha(root, expected)?;
        }

        // ── Auto-snapshot (ADR-0154 / #335): before a destructive op mutates
        // the repo, take a savepoint under `refs/kagi/snapshots/` so the work
        // is recoverable in git's own terms. Gated by `auto_snapshot` (default
        // on). Skipped for RestoreSnapshot itself (it takes its own savepoint)
        // and when the plan is a no-op. A snapshot failure is logged but never
        // blocks the user's operation.
        // A dropped stash commit is its own savepoint (`git stash store <oid>`).
        if matches!(op, Operation::StashDrop { .. }) {
            evidence.worktree_before = Some(self.stash_worktree_fingerprint()?);
        }
        if matches!(
            op,
            Operation::StashApply { .. } | Operation::StashPop { .. }
        ) {
            evidence.conflict_identity_before = self.stash_conflict_identity()?;
        }
        if matches!(op, Operation::StashPush { .. }) {
            evidence.untracked_before = self.working_tree_status()?.untracked;
        }
        if matches!(fault, Some(stash::StashFaultPoint::BeforeMutation)) {
            panic!("stash fault before mutation");
        }
        if self.policy.auto_snapshot
            && plan.destructive
            && !matches!(
                op,
                Operation::RestoreSnapshot { .. } | Operation::StashDrop { .. }
            )
        {
            if stash::StashAction::from_operation(op).is_some() {
                evidence.started = true;
            }
            let snapshot = self.auto_savepoint(op.oplog_name());
            if stash::StashAction::from_operation(op).is_some() {
                evidence.snapshot = snapshot;
            }
        }

        // ── Dispatch (behaviour-identical to the former execute(op)). ──
        if stash::StashAction::from_operation(op).is_some() {
            evidence.started = true;
        }
        let result: Result<OperationOutcome, GitError> = match op {
            Operation::Commit { message } => {
                self.execute_commit(message).map(OperationOutcome::Commit)
            }
            Operation::MergeCommit { message } => self
                .execute_merge_commit(message)
                .map(OperationOutcome::Commit),
            Operation::Checkout { branch } => self
                .execute_checkout(branch)
                .map(|()| OperationOutcome::Unit),
            Operation::CheckoutCommit { id } => self
                .execute_checkout_commit(id)
                .map(|()| OperationOutcome::Unit),
            Operation::CreateBranch { name, at } => self
                .execute_create_branch(name, at)
                .map(|()| OperationOutcome::Unit),
            Operation::CreateBranchWithCheckout {
                name,
                at,
                checkout_after,
            } => {
                match self.execute_create_branch(name, at) {
                    Err(error) => Err(error),
                    Ok(()) if *checkout_after => match self.execute_checkout(name) {
                        Ok(()) => Ok(OperationOutcome::Unit),
                        Err(error) => {
                            // The ref was created, but safe checkout left HEAD on
                            // the previous branch. Preserve the exact recovery
                            // handle while returning the checkout failure.
                            *partial_after = Some(ops::StateSummary {
                                head: plan.current.head.clone(),
                                dirty: format!("created branch '{}' @ {}", name, at.0),
                            });
                            Err(error)
                        }
                    },
                    Ok(()) => Ok(OperationOutcome::Unit),
                }
            }
            Operation::CreateTag { name, at } => self
                .execute_create_tag(name, at)
                .map(|()| OperationOutcome::Unit),
            Operation::PushTag { name, remote } => self
                .execute_push_tag(remote, name)
                .map(|()| OperationOutcome::Unit),
            Operation::CreateWorktree {
                branch,
                path,
                start,
            } => self
                .execute_create_worktree(branch, path.as_str(), start)
                .map(|()| OperationOutcome::Unit),
            Operation::OpenWorktreeForBranch { branch, path } => self
                .execute_open_worktree_for_branch(branch, path.as_str())
                .map(|()| OperationOutcome::Unit),
            Operation::StashPush {
                message,
                include_untracked,
            } => self
                .execute_stash_push(message.as_deref(), *include_untracked)
                .map(|()| OperationOutcome::Unit),
            Operation::StashApply { index } => self.execute_stash_apply(*index).map(|()| {
                evidence.applied = true;
                OperationOutcome::Unit
            }),
            Operation::StashPop { index } => {
                let identity = plan
                    .stash_identity
                    .as_ref()
                    .ok_or_else(|| GitError::Other("missing stash identity".into()))?;
                ops::execute_stash_pop_recorded(&mut self.repo, *index, identity, evidence, fault)
                    .map(OperationOutcome::StashPop)
            }
            Operation::StashDrop { index } => self.execute_stash_drop(*index).and_then(|oid| {
                if evidence.oid.as_deref() != Some(&oid) {
                    return Err(GitError::Other(format!(
                        "drop returned unexpected oid {oid}"
                    )));
                }
                Ok(OperationOutcome::StashDrop { oid })
            }),
            Operation::CherryPick { id } => {
                self.execute_cherry_pick(id).map(OperationOutcome::Commit)
            }
            Operation::MergeBranch { target } => self
                .execute_merge_branch(target)
                .map(OperationOutcome::Commit),
            Operation::MergeIntoConflict { target } => self
                .execute_merge_into_conflict(target)
                .map(OperationOutcome::MergeIntoConflict),
            Operation::MergeIntoBranch { source, target } => self
                .execute_merge_into_branch(source, target)
                .map(OperationOutcome::Commit),
            Operation::CheckoutTrackingBranch {
                remote_branch,
                local_branch,
            } => self
                .execute_checkout_tracking_branch(remote_branch, local_branch)
                .map(|()| OperationOutcome::Unit),
            Operation::SwitchToLatestBranch {
                branch_name,
                remote_branch,
            } => self
                .execute_switch_to_latest(plan, branch_name, remote_branch)
                .map(|()| OperationOutcome::Unit),
            Operation::Revert { id } => self.execute_revert(id).map(OperationOutcome::Commit),
            Operation::Pull => self.execute_pull().map(OperationOutcome::Pull),
            Operation::Push => self.execute_push().map(OperationOutcome::Push),
            Operation::PullBranchFf { branch_name } => self
                .execute_pull_branch_ff(plan, branch_name)
                .map(OperationOutcome::Pull),
            Operation::PushBranch {
                branch_name,
                set_upstream,
            } => self
                .execute_push_branch(plan, branch_name, *set_upstream)
                .map(OperationOutcome::Push),
            Operation::SetUpstream {
                branch_name,
                upstream,
            } => self
                .execute_set_upstream(plan, branch_name, upstream)
                .map(|()| OperationOutcome::Unit),
            Operation::RenameBranch { old_name, new_name } => self
                .execute_rename_branch(plan, old_name, new_name)
                .map(|()| OperationOutcome::Unit),
            Operation::UndoCommit => self.execute_undo_commit().map(OperationOutcome::Undo),
            Operation::Amend { mode, message } => self
                .execute_amend(*mode, message.as_deref())
                .map(OperationOutcome::Amend),
            Operation::DeleteBranch { name } => {
                self.execute_delete_branch(plan, name, backup_refs, partial_after)
            }
            Operation::DeleteRemoteBranch { remote_branch } => self
                .execute_delete_remote_branch(remote_branch)
                .map(|()| OperationOutcome::Unit),
            Operation::ResetCurrentToHead { target } => self
                .execute_reset_current_to_head(target)
                .map(|()| OperationOutcome::Unit),
            Operation::ForceWithLeasePush => self
                .execute_force_with_lease_push(plan)
                .map(|()| OperationOutcome::Unit),
            Operation::RebaseCurrentOnto { onto } => self
                .execute_rebase_current_onto(onto)
                .map(OperationOutcome::Rebase),
            Operation::Discard { paths } => self
                .execute_discard(plan, paths)
                .map(OperationOutcome::Discard),
            Operation::RestoreSnapshot { id } => self
                .execute_restore_snapshot(id)
                .map(|savepoint| OperationOutcome::RestoreSnapshot { savepoint }),
            Operation::ApplySuggestion {
                suggestion,
                expected_original,
            } => self
                .execute_apply_suggestion(plan, suggestion, expected_original)
                .map(OperationOutcome::Suggestion),
        };

        result
    }
}
