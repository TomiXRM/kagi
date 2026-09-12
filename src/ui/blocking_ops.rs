//! Blocking operation cores (extracted from mod.rs, ADR-0112 / Phase D).
//!
//! Invariant (ADR-0078): these UI-side workers use [`kagi_git::Backend`] rather
//! than opening repositories directly; using Git bindings here would bypass
//! centralized preflight, verification, and oplog recording.
//!
//! These free functions are the synchronous "blocking" backends for each
//! mutating operation. They take a repo path + plan, open a Backend, run the
//! operation, and return a result. The UI's `start_*` handlers call them via
//! `cx.background_spawn`. None of them touch `&self`, `cx`, or `window`.

use kagi_git::backend::recording::RunReport;
use kagi_git::{AmendMode, CommitId, Head, MergeKind, OperationPlan, PullOutcome, StateSummary};

use crate::ui::i18n;
use crate::ui::settings::Settings;
use crate::ui::{BranchPlanKind, BranchPlanModal, CheckoutPlanTarget};

/// Open a [`kagi_git::Backend`] with the `auto_snapshot` setting applied
/// (ADR-0154 / #335). Every blocking op opens through here so the automatic
/// pre-destructive savepoint honours the user's toggle (default on).
pub(crate) fn open_backend(
    repo_path: &std::path::Path,
) -> Result<kagi_git::Backend, kagi_git::GitError> {
    kagi_git::Backend::open_with_policy(repo_path, execution_policy())
}

pub(crate) fn execution_policy() -> kagi_git::backend::ExecutionPolicy {
    kagi_git::backend::ExecutionPolicy::human(Settings::load().auto_snapshot())
}

mod discard;
pub(crate) use discard::discard_blocking;
mod pull;
pub(crate) use pull::{pull_blocking, refuse_blocked_pull};

// Background and headless hosts share these operation cores.

/// Blocking part of push. Returns the backend's receipt (ADR-0196 Wave 2).
pub(crate) fn push_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let report = repo.run_recorded(&kagi_git::Operation::Push, plan);
    if let Ok(outcome) = &report.result {
        klog!("executed: push — {}", push_summary(outcome));
        let after_summary = verify_after_snapshot(repo_path, plan);
        klog!("verified: push after = {}", after_summary.head);
    }
    Ok(report)
}

/// The push line the footer and the `async: push finished` contract line share.
pub(crate) fn push_summary(outcome: &kagi_git::OperationOutcome) -> String {
    match outcome {
        kagi_git::OperationOutcome::Push(o) if o.set_upstream => {
            format!("pushed {} commit(s), set upstream", o.pushed)
        }
        kagi_git::OperationOutcome::Push(o) => format!("pushed {} commit(s)", o.pushed),
        _ => "push: unexpected outcome".to_string(),
    }
}

/// Re-snapshot the repo for the verified after-state; falls back to the
/// plan's prediction when the snapshot fails (non-fatal).
pub(crate) fn verify_after_snapshot(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
) -> StateSummary {
    match open_backend(repo_path) {
        Ok(mut repo2) => match repo2.snapshot(10_000) {
            Ok(snap) => StateSummary {
                head: snap.head.display(),
                dirty: if snap.status.is_dirty() {
                    "dirty".to_string()
                } else {
                    "clean".to_string()
                },
            },
            Err(_) => plan.predicted.clone(),
        },
        Err(_) => plan.predicted.clone(),
    }
}

// ──────────────────────────────────────────────────────────────
// W15-ASYNCOPS: blocking cores for the tree-size-proportional ops
//
// Same shape as the pull/push/stash cores above: repo open → preflight →
// execute → verify snapshot, free of `&mut KagiApp`, so the UI button path can
// run them via `cx.background_spawn`. The headless KAGI_* path keeps calling the
// synchronous `confirm_*` methods (unchanged log文言/order). ref-order rules and
// in-memory semantics are unchanged — only the threading moved.
// ──────────────────────────────────────────────────────────────

/// Blocking part of checkout (branch or commit). `checkout_tree` writes the
/// working tree on disk, which scales with tree size.
///
/// Returns the backend's own receipt (ADR-0196 Wave 2). It comes back even when
/// the operation failed or its termination is unknown, because that is exactly
/// when the UI must present the *real* outcome instead of re-synthesizing one
/// from a stringified error (#643 A1/A2). Only a repository that would not open
/// is an `Err` — nothing ran, so there is nothing to report.
pub(crate) fn checkout_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    target: &CheckoutPlanTarget,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    // ADR-0104 Phase 2: route through the run pipeline so preflight is enforced
    // in one place. `run_recorded`, not `run`: `run` drops the receipt, and the
    // UI then had to invent one.
    let op = match target {
        CheckoutPlanTarget::Branch(branch) => kagi_git::Operation::Checkout {
            branch: branch.clone(),
        },
        CheckoutPlanTarget::Commit(commit_id) => kagi_git::Operation::CheckoutCommit {
            id: commit_id.clone(),
        },
    };
    let report = repo.run_recorded(&op, plan);
    if report.result.is_err() {
        return Ok(report);
    }

    match target {
        CheckoutPlanTarget::Branch(branch) => klog!("executed: checkout {}", branch),
        CheckoutPlanTarget::Commit(commit_id) => {
            klog!("executed: checkout-commit {}", commit_id.short())
        }
    }

    // Verify: re-snapshot and confirm HEAD. Evidence for the log only — the
    // receipt already holds the recorded `after` and is not rewritten from it.
    match open_backend(repo_path).and_then(|mut repo2| repo2.snapshot(10_000)) {
        Ok(snap) => match (target, &snap.head) {
            (
                CheckoutPlanTarget::Branch(branch),
                Head::Attached {
                    branch: actual_branch,
                    ..
                },
            ) if actual_branch == branch => {
                klog!("verified: HEAD={}", actual_branch);
            }
            (CheckoutPlanTarget::Commit(commit_id), Head::Detached { target: t })
                if t == &commit_id.0 =>
            {
                klog!("verified: detached HEAD={}", commit_id.short());
            }
            other => {
                eprintln!(
                    "[kagi] verify: unexpected HEAD state after checkout: {:?}",
                    other
                );
            }
        },
        Err(e) => klog!("verify: snapshot error: {}", e),
    }
    Ok(report)
}

/// Reopen the merge plan's worktree without accepting a retargeted locator.
pub(crate) fn open_merge_backend(
    owner: &crate::app::Attachment,
) -> Result<kagi_git::Backend, String> {
    let repo = open_backend(&owner.path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    if owner.worktree.is_none() || repo.write_worktree_id().ok() != owner.worktree {
        return Err(i18n::Msg::MergeDestinationChanged.t().to_string());
    }
    Ok(repo)
}

/// Merge `source` into `target` without checking `target` out (ADR-0144).
///
/// Separate from [`merge_blocking`] rather than another arm inside it: this one
/// takes two branch names and can never enter Conflict Mode, so folding it in
/// would mean a `kind` that is meaningless for half the callers.
pub(crate) fn merge_into_branch_blocking(
    owner: &crate::app::Attachment,
    plan: &OperationPlan,
    source: &str,
    target: &str,
) -> Result<RunReport, String> {
    let repo_path = owner.path.as_path();
    let mut repo = open_merge_backend(owner)?;
    let op = kagi_git::Operation::MergeIntoBranch {
        source: source.to_string(),
        target: target.to_string(),
    };
    let report = repo.run_recorded(&op, plan);
    if let Ok(kagi_git::OperationOutcome::Commit(new_tip)) = &report.result {
        klog!(
            "executed: merge-into {} -> {} = {}",
            source,
            target,
            new_tip.short()
        );
        let after = verify_after_snapshot(repo_path, plan);
        klog!("verified: merge-into after = {}", after.head);
    }
    Ok(report)
}

pub(crate) fn merge_blocking(
    owner: &crate::app::Attachment,
    plan: &OperationPlan,
    target: &str,
    kind: &MergeKind,
) -> Result<RunReport, String> {
    let repo_path = owner.path.as_path();
    let mut repo = open_merge_backend(owner)?;
    let op = match kind {
        // W31: perform the real conflicting merge — leaves markers + index
        // stages + MERGE_HEAD. No commit is created; Conflict Mode takes over
        // on the subsequent reload.
        MergeKind::Conflicts(_) => kagi_git::Operation::MergeIntoConflict {
            target: target.to_string(),
        },
        MergeKind::FastForward | MergeKind::MergeCommit => kagi_git::Operation::MergeBranch {
            target: target.to_string(),
        },
    };
    let report = repo.run_recorded(&op, plan);
    match &report.result {
        Ok(kagi_git::OperationOutcome::MergeIntoConflict(files)) => {
            eprintln!(
                "[kagi] executed: merge-into-conflict {} -> {} conflict(s)",
                target,
                files.len()
            );
        }
        Ok(kagi_git::OperationOutcome::Commit(new_head)) => {
            klog!("executed: merge {} -> {}", target, new_head.short());
            let after = verify_after_snapshot(repo_path, plan);
            klog!("verified: merge after = {}", after.head);
        }
        _ => {}
    }
    Ok(report)
}

/// The ` — merge …` suffix of the `async: merge finished` contract line.
pub(crate) fn merge_summary(
    off_branch: bool,
    source: &str,
    into: &str,
    outcome: &kagi_git::OperationOutcome,
) -> String {
    match outcome {
        _ if off_branch => format!("merge {source} into {into}"),
        kagi_git::OperationOutcome::MergeIntoConflict(files) => {
            format!("merge {} (conflicts: {})", source, files.len())
        }
        _ => format!("merge {}", source),
    }
}

pub(crate) fn checkout_tracking_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    remote_branch: &str,
    local_branch: &str,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let op = kagi_git::Operation::CheckoutTrackingBranch {
        remote_branch: remote_branch.to_string(),
        local_branch: local_branch.to_string(),
    };
    let report = repo.run_recorded(&op, plan);
    if report.result.is_ok() {
        eprintln!(
            "[kagi] executed: checkout-tracking {} -> {}",
            remote_branch, local_branch
        );
        let after = verify_after_snapshot(repo_path, plan);
        klog!("verified: checkout-tracking after = {}", after.head);
    }
    Ok(report)
}

pub(crate) fn switch_to_latest_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    branch_name: &str,
    remote_branch: &str,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let op = kagi_git::Operation::SwitchToLatestBranch {
        branch_name: branch_name.to_string(),
        remote_branch: remote_branch.to_string(),
    };
    let report = repo.run_recorded(&op, plan);
    if report.result.is_ok() {
        klog!(
            "executed: switch-to-latest {} <- {}",
            branch_name,
            remote_branch
        );
        let after = verify_after_snapshot(repo_path, plan);
        klog!("verified: switch-to-latest after = {}", after.head);
    }
    Ok(report)
}

/// Blocking part of cherry-pick (in-memory index merge → commit → safe
/// checkout_head). Scales with the diff size.
pub(crate) fn cherry_pick_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    commit_id: &CommitId,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let op = kagi_git::Operation::CherryPick {
        id: commit_id.clone(),
    };
    let report = repo.run_recorded(&op, plan);
    if let Ok(kagi_git::OperationOutcome::Commit(new_id)) = &report.result {
        eprintln!(
            "[kagi] executed: cherry-pick {} -> {}",
            commit_id.short(),
            new_id.short()
        );
        // Log evidence only; the receipt already carries the verified `after`.
        verify_new_commit_snapshot(repo_path, plan, new_id, "cherry-pick");
    }
    Ok(report)
}

/// Blocking part of revert (in-memory inverse merge → commit). Scales with the
/// diff size.
pub(crate) fn revert_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    commit_id: &CommitId,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let op = kagi_git::Operation::Revert {
        id: commit_id.clone(),
    };
    let report = repo.run_recorded(&op, plan);
    if let Ok(kagi_git::OperationOutcome::Commit(new_id)) = &report.result {
        eprintln!(
            "[kagi] executed: revert {} -> {}",
            commit_id.short(),
            new_id.short()
        );
        // Log evidence only; the receipt already carries the verified `after`.
        verify_new_commit_snapshot(repo_path, plan, new_id, "revert");
    }
    Ok(report)
}

/// Blocking part of commit (tree-build + write). Scales with the staged tree.
pub(crate) fn commit_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    message: &str,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    // Commit's plan is a HEAD snapshot; preflight detects a checkout/commit
    // between plan and execute.
    let op = kagi_git::Operation::Commit {
        message: message.to_string(),
    };
    let report = repo.run_recorded(&op, plan);
    if let Ok(kagi_git::OperationOutcome::Commit(new_id)) = &report.result {
        klog!("executed: commit {}", new_id.short());
        log_commit_verification(repo_path, new_id);
    }
    Ok(report)
}

/// Verify evidence for the log only: HEAD is the new commit, unstaged remain.
/// The receipt already carries the recorded `after`.
fn log_commit_verification(repo_path: &std::path::Path, new_id: &CommitId) {
    match open_backend(repo_path) {
        Ok(mut repo2) => match repo2.snapshot(10_000) {
            Ok(snap) => {
                if let Head::Attached { target, branch } = &snap.head {
                    if *target == new_id.0 {
                        eprintln!(
                            "[kagi] verified: commit HEAD={} on {}",
                            new_id.short(),
                            branch
                        );
                    } else {
                        klog!("verify: HEAD mismatch after commit");
                    }
                }
                eprintln!(
                    "[kagi] verified: working tree {} after commit",
                    if snap.status.is_dirty() {
                        "dirty (unstaged remain)"
                    } else {
                        "clean"
                    }
                );
            }
            Err(e) => klog!("verify: snapshot error: {}", e),
        },
        Err(e) => klog!("verify: repo open error: {}", e),
    }
}

/// Blocking part of amend (history rewrite: tree-build + commit-replace).
pub(crate) fn amend_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    mode: AmendMode,
    message: &str,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let op = kagi_git::Operation::Amend {
        mode,
        message: if message.trim().is_empty() {
            None
        } else {
            Some(message.to_string())
        },
    };
    let report = repo.run_recorded(&op, plan);
    if let Ok(kagi_git::OperationOutcome::Amend(outcome)) = &report.result {
        eprintln!(
            "[kagi] executed: amend {} -> {}",
            outcome.old.short(),
            outcome.new.short()
        );
    }
    Ok(report)
}

/// Blocking part of delete-branch (preflight → ref delete). Lightweight, but
/// kept on the background path for consistency with the other confirm flows.
pub(crate) fn delete_branch_blocking(
    owner: &crate::app::Attachment,
    plan: &OperationPlan,
    branch_name: &str,
) -> Result<RunReport, String> {
    let mut repo = open_backend(&owner.path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    if repo.write_worktree_id().ok().as_ref() != owner.worktree.as_ref() || owner.worktree.is_none()
    {
        return Err("worktree identity changed; reopen the repository".into());
    }
    let op = kagi_git::Operation::DeleteBranch {
        name: branch_name.to_string(),
    };
    let report = repo.run_recorded(&op, plan);
    if report.result.is_ok() {
        klog!("executed: delete-branch {}", branch_name);
    }
    Ok(report)
}

pub(crate) fn delete_remote_branch_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    remote_branch: &str,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let op = kagi_git::Operation::DeleteRemoteBranch {
        remote_branch: remote_branch.to_string(),
    };
    let report = repo.run_recorded(&op, plan);
    if report.result.is_ok() {
        klog!("executed: delete-remote-branch {}", remote_branch);
    }
    Ok(report)
}

pub(crate) fn branch_plan_blocking(
    repo_path: &std::path::Path,
    modal: &BranchPlanModal,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let op = match modal.kind {
        BranchPlanKind::PullFfOnly => kagi_git::Operation::PullBranchFf {
            branch_name: modal.branch_name.clone(),
        },
        BranchPlanKind::Push | BranchPlanKind::PushSetUpstream => kagi_git::Operation::PushBranch {
            branch_name: modal.branch_name.clone(),
            set_upstream: modal.kind == BranchPlanKind::PushSetUpstream,
        },
    };
    Ok(repo.run_recorded(&op, &modal.plan))
}

/// The footer line for a branch-plan outcome (pull-ff / push / push-set-upstream).
pub(crate) fn branch_plan_summary(
    branch_name: &str,
    outcome: &kagi_git::OperationOutcome,
) -> String {
    match outcome {
        kagi_git::OperationOutcome::Pull(PullOutcome::UpToDate) => {
            format!("branch '{}' already up to date", branch_name)
        }
        kagi_git::OperationOutcome::Pull(PullOutcome::FastForward { to }) => {
            format!("branch '{}' fast-forwarded to {}", branch_name, to.short())
        }
        kagi_git::OperationOutcome::Pull(PullOutcome::Merged { .. }) => {
            "unexpected merge outcome".to_string()
        }
        kagi_git::OperationOutcome::Push(o) => format!(
            "branch '{}' pushed {} commit(s){}",
            branch_name,
            o.pushed,
            if o.set_upstream {
                " and upstream set"
            } else {
                ""
            }
        ),
        _ => "unexpected outcome".to_string(),
    }
}

pub(crate) fn set_upstream_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    branch_name: &str,
    upstream: &str,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let op = kagi_git::Operation::SetUpstream {
        branch_name: branch_name.to_string(),
        upstream: upstream.to_string(),
    };
    Ok(repo.run_recorded(&op, plan))
}

pub(crate) fn rename_branch_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    old_name: &str,
    new_name: &str,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let op = kagi_git::Operation::RenameBranch {
        old_name: old_name.to_string(),
        new_name: new_name.to_string(),
    };
    Ok(repo.run_recorded(&op, plan))
}

/// Blocking part of create-worktree (checks out a full tree into a new linked
/// worktree on disk — scales with tree size).
pub(crate) fn create_worktree_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    branch_input: &str,
    path_input: &str,
    at: &CommitId,
    allow_existing_branch: bool,
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let op = if allow_existing_branch {
        kagi_git::Operation::OpenWorktreeForBranch {
            branch: branch_input.to_string(),
            path: path_input.to_string(),
        }
    } else {
        kagi_git::Operation::CreateWorktree {
            branch: branch_input.to_string(),
            path: path_input.to_string(),
            start: at.clone(),
        }
    };
    let report = repo.run_recorded(&op, plan);
    if report.result.is_err() {
        return Ok(report);
    }
    eprintln!(
        "[kagi] executed: create-worktree '{}' path='{}' @ {}",
        branch_input,
        path_input,
        at.short()
    );

    // Verify: open the linked worktree and log its HEAD.
    let verify_path = {
        let path = std::path::PathBuf::from(path_input);
        if path.is_absolute() {
            path
        } else {
            repo_path.join(path)
        }
    };
    match kagi_git::Backend::open(&verify_path) {
        Ok(linked) => {
            let head = linked.head_shorthand();
            eprintln!(
                "[kagi] verified: worktree '{}' HEAD={}",
                verify_path.display(),
                head.unwrap_or_else(|| "?".to_string())
            );
        }
        Err(e) => klog!("verify: worktree open error: {}", e),
    }

    // Assign + persist this worktree's port block now, so it is stable across
    // kagi restarts and ready for the terminal-wiring PR to inject as KAGI_PORT
    // (issue #342 / ADR-0171). Numbers-only (no socket bound); exhaustion of the
    // range is surfaced via klog, never worked around.
    let s = Settings::load();
    let (start, end) = s.worktree_port_range();
    let range = kagi_domain::worktree_ports::PortRange { start, end };
    let per = s.worktree_ports_per_worktree();
    match kagi_git::worktree_ports::assign_block(&verify_path, range, per) {
        Some(p) => klog!("worktree-port: assigned {} → {}", verify_path.display(), p),
        None => klog!(
            "worktree-port: range {start}-{end} exhausted for {}",
            verify_path.display()
        ),
    }

    Ok(report)
}

/// Re-snapshot after a new-commit op (cherry-pick / revert) for the after-state,
/// logging the verified HEAD. Falls back to the plan prediction on failure.
pub(crate) fn verify_new_commit_snapshot(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    new_id: &CommitId,
    op: &str,
) -> StateSummary {
    match open_backend(repo_path) {
        Ok(mut repo2) => match repo2.snapshot(10_000) {
            Ok(snap) => {
                if let Head::Attached { target, branch } = &snap.head {
                    if *target == new_id.0 {
                        eprintln!(
                            "[kagi] verified: {} HEAD={} on {}",
                            op,
                            new_id.short(),
                            branch
                        );
                    } else {
                        eprintln!(
                            "[kagi] verify: HEAD={} expected {}",
                            &target[..8.min(target.len())],
                            new_id.short()
                        );
                    }
                    let is_clean = !snap.status.is_dirty();
                    eprintln!(
                        "[kagi] verified: working tree {}",
                        if is_clean {
                            "clean"
                        } else {
                            "dirty (unexpected)"
                        }
                    );
                }
                StateSummary {
                    head: snap.head.display(),
                    dirty: if snap.status.is_dirty() {
                        "dirty".to_string()
                    } else {
                        "clean".to_string()
                    },
                }
            }
            Err(e) => {
                klog!("verify: snapshot error: {}", e);
                plan.predicted.clone()
            }
        },
        Err(e) => {
            klog!("verify: repo open error: {}", e);
            plan.predicted.clone()
        }
    }
}

#[cfg(test)]
#[path = "blocking_ops_tests.rs"]
mod tests;
