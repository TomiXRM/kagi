//! Blocking operation cores (extracted from mod.rs, ADR-0112 / Phase D).
//!
//! These free functions are the synchronous "blocking" backends for each
//! mutating operation. They take a repo path + plan, open a Backend, run the
//! operation, and return a result. The UI's `start_*` handlers call them via
//! `cx.background_spawn`. None of them touch `&self`, `cx`, or `window`.

use std::time::Instant;

use kagi_git::{AmendMode, CommitId, Head, MergeKind, OperationPlan, PullOutcome, StateSummary};

use crate::ui::{BranchPlanKind, BranchPlanModal, CheckoutPlanTarget};

// W3-NOTIFY: blocking cores for pull / push
//
// Everything that may take seconds (repo open → preflight → execute →
// verify snapshot) lives here, free of `&mut KagiApp`, so the UI path can
// run it via `cx.background_spawn` while the headless path calls it inline.
// ──────────────────────────────────────────────────────────────

/// Blocking part of stash push (preflight → execute → verify). Stashing
/// copies the working tree (and untracked files) into the stash, which can
/// take a long time on large repos — running it on the UI thread looked
/// like a total freeze (user-reported).
pub(crate) fn stash_push_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    message: Option<String>,
) -> Result<(String, StateSummary), String> {
    let t0 = Instant::now();
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced
    // (run uses preflight_check_stash for stash ops: HEAD + stash-count guard).
    let op = kagi_git::Operation::StashPush {
        message: message.clone(),
        include_untracked: true,
    };
    repo.run(&op, plan)
        .map_err(|e| format!("Stash push failed: {}", e))?;
    let t_stash = t0.elapsed();
    eprintln!(
        "[kagi] executed: stash-push message={:?}",
        message.unwrap_or_default()
    );

    // Light verify: the full reload that follows on the main thread already
    // rebuilds the complete snapshot, so re-walking 10k commits here only
    // doubled the wall-clock (user asked why stash took ~10s). Status + a
    // stash-count check are enough to confirm the operation took effect.
    let t1 = Instant::now();
    let after = match repo.working_tree_status() {
        Ok(status) => {
            if !status.is_dirty() {
                klog!("verified: working tree clean after stash-push");
            } else {
                klog!("verify: working tree NOT clean after stash-push");
            }
            let count = repo.stash_count().unwrap_or(0);
            klog!("verified: stash count={}", count);
            // resolve_head is crate-private; the predicted head from the
            // plan is accurate here (stash does not move HEAD).
            let head = plan.predicted.head.clone();
            StateSummary {
                head,
                dirty: if status.is_dirty() {
                    "dirty".into()
                } else {
                    "clean".into()
                },
            }
        }
        Err(_) => plan.predicted.clone(),
    };
    eprintln!(
        "[kagi] async: stash-push timing stash={:.1}s verify={:.1}s",
        t_stash.as_secs_f32(),
        t1.elapsed().as_secs_f32()
    );
    Ok(("stashed working tree".to_string(), after))
}

/// Blocking part of pull. Returns (human summary, after-state) or an error
/// message suitable for the oplog / modal.
pub(crate) fn pull_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
) -> Result<(String, StateSummary), String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    let outcome = match repo.run(&kagi_git::Operation::Pull, plan) {
        Ok(kagi_git::OperationOutcome::Pull(o)) => o,
        Ok(_) => return Err("pull: unexpected outcome".to_string()),
        Err(e) => return Err(format!("Pull failed: {}", e)),
    };
    let summary = match &outcome {
        PullOutcome::UpToDate => "already up to date".to_string(),
        PullOutcome::FastForward { to } => format!("fast-forward to {}", to.short()),
        PullOutcome::Merged { commit } => format!("merge commit {}", commit.short()),
    };
    klog!("executed: pull — {}", summary);

    // Verify: re-snapshot for the after-state.
    let after_summary = verify_after_snapshot(repo_path, plan);
    klog!("verified: pull after = {}", after_summary.head);
    Ok((summary, after_summary))
}

/// Blocking part of push. Returns (human summary, after-state) or an error
/// message suitable for the oplog / modal.
pub(crate) fn push_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
) -> Result<(String, StateSummary), String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    let outcome = match repo.run(&kagi_git::Operation::Push, plan) {
        Ok(kagi_git::OperationOutcome::Push(o)) => o,
        Ok(_) => return Err("push: unexpected outcome".to_string()),
        Err(e) => return Err(format!("Push failed: {}", e)),
    };
    let summary = if outcome.set_upstream {
        format!("pushed {} commit(s), set upstream", outcome.pushed)
    } else {
        format!("pushed {} commit(s)", outcome.pushed)
    };
    klog!("executed: push — {}", summary);

    let after_summary = verify_after_snapshot(repo_path, plan);
    klog!("verified: push after = {}", after_summary.head);
    Ok((summary, after_summary))
}

/// Re-snapshot the repo for the verified after-state; falls back to the
/// plan's prediction when the snapshot fails (non-fatal).
pub(crate) fn verify_after_snapshot(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
) -> StateSummary {
    match kagi_git::Backend::open(repo_path) {
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
pub(crate) fn checkout_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    target: &CheckoutPlanTarget,
) -> Result<(String, StateSummary), String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced
    // in one place (run() calls preflight_check as its first line).
    let op = match target {
        CheckoutPlanTarget::Branch(branch) => kagi_git::Operation::Checkout {
            branch: branch.clone(),
        },
        CheckoutPlanTarget::Commit(commit_id) => kagi_git::Operation::CheckoutCommit {
            id: commit_id.clone(),
        },
    };
    repo.run(&op, plan)
        .map_err(|e| format!("Checkout failed: {}", e))?;

    let summary = match target {
        CheckoutPlanTarget::Branch(branch) => {
            klog!("executed: checkout {}", branch);
            format!("checkout {}", branch)
        }
        CheckoutPlanTarget::Commit(commit_id) => {
            klog!("executed: checkout-commit {}", commit_id.short());
            format!("detached: {}", commit_id.short())
        }
    };

    // Verify: re-snapshot and confirm HEAD.
    let after = match kagi_git::Backend::open(repo_path) {
        Ok(mut repo2) => match repo2.snapshot(10_000) {
            Ok(snap) => {
                match (target, &snap.head) {
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
    };
    Ok((summary, after))
}

pub(crate) fn merge_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    target: &str,
    kind: &MergeKind,
) -> Result<(String, StateSummary), String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    match kind {
        MergeKind::Conflicts(_) => {
            // W31: perform the real conflicting merge — leaves markers + index
            // stages + MERGE_HEAD. No commit is created; Conflict Mode takes over
            // on the subsequent reload.
            let op = kagi_git::Operation::MergeIntoConflict {
                target: target.to_string(),
            };
            let files = match repo.run(&op, plan) {
                Ok(kagi_git::OperationOutcome::MergeIntoConflict(f)) => f,
                Ok(_) => return Err("merge-into-conflict: unexpected outcome".to_string()),
                Err(e) => return Err(format!("Merge failed: {}", e)),
            };
            eprintln!(
                "[kagi] executed: merge-into-conflict {} -> {} conflict(s)",
                target,
                files.len()
            );
            let after = verify_after_snapshot(repo_path, plan);
            Ok((
                format!("merge {} (conflicts: {})", target, files.len()),
                after,
            ))
        }
        MergeKind::FastForward | MergeKind::MergeCommit => {
            let op = kagi_git::Operation::MergeBranch {
                target: target.to_string(),
            };
            let _new_head = match repo.run(&op, plan) {
                Ok(kagi_git::OperationOutcome::Commit(c)) => c,
                Ok(_) => return Err("merge: unexpected outcome".to_string()),
                Err(e) => return Err(format!("Merge failed: {}", e)),
            };
            klog!("executed: merge {} -> {}", target, _new_head.short());

            let after = verify_after_snapshot(repo_path, plan);
            klog!("verified: merge after = {}", after.head);
            Ok((format!("merge {}", target), after))
        }
    }
}

pub(crate) fn checkout_tracking_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    remote_branch: &str,
    local_branch: &str,
) -> Result<(String, StateSummary), String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    let op = kagi_git::Operation::CheckoutTrackingBranch {
        remote_branch: remote_branch.to_string(),
        local_branch: local_branch.to_string(),
    };
    repo.run(&op, plan)
        .map_err(|e| format!("Checkout tracking failed: {}", e))?;
    eprintln!(
        "[kagi] executed: checkout-tracking {} -> {}",
        remote_branch, local_branch
    );

    let after = verify_after_snapshot(repo_path, plan);
    klog!("verified: checkout-tracking after = {}", after.head);
    Ok((format!("checkout {}", local_branch), after))
}

pub(crate) fn switch_to_latest_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    branch_name: &str,
    remote_branch: &str,
) -> Result<(String, StateSummary), String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    let op = kagi_git::Operation::SwitchToLatestBranch {
        branch_name: branch_name.to_string(),
        remote_branch: remote_branch.to_string(),
    };
    repo.run(&op, plan)
        .map_err(|e| format!("Switch to latest failed: {}", e))?;
    klog!(
        "executed: switch-to-latest {} <- {}",
        branch_name,
        remote_branch
    );

    let after = verify_after_snapshot(repo_path, plan);
    klog!("verified: switch-to-latest after = {}", after.head);
    Ok((format!("switch {}", branch_name), after))
}

/// Blocking part of cherry-pick (in-memory index merge → commit → safe
/// checkout_head). Scales with the diff size.
pub(crate) fn cherry_pick_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    commit_id: &CommitId,
) -> Result<(String, StateSummary), String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    let op = kagi_git::Operation::CherryPick {
        id: commit_id.clone(),
    };
    let new_id = match repo.run(&op, plan) {
        Ok(kagi_git::OperationOutcome::Commit(c)) => c,
        Ok(_) => return Err("cherry-pick: unexpected outcome".to_string()),
        Err(e) => return Err(format!("Cherry-pick failed: {}", e)),
    };
    eprintln!(
        "[kagi] executed: cherry-pick {} -> {}",
        commit_id.short(),
        new_id.short()
    );

    let after = verify_new_commit_snapshot(repo_path, plan, &new_id, "cherry-pick");
    Ok((format!("{} applied", commit_id.short()), after))
}

/// Blocking part of revert (in-memory inverse merge → commit). Scales with the
/// diff size.
pub(crate) fn revert_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    commit_id: &CommitId,
) -> Result<(String, StateSummary), String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    let op = kagi_git::Operation::Revert {
        id: commit_id.clone(),
    };
    let new_id = match repo.run(&op, plan) {
        Ok(kagi_git::OperationOutcome::Commit(c)) => c,
        Ok(_) => return Err("revert: unexpected outcome".to_string()),
        Err(e) => return Err(format!("Revert failed: {}", e)),
    };
    eprintln!(
        "[kagi] executed: revert {} -> {}",
        commit_id.short(),
        new_id.short()
    );

    let after = verify_new_commit_snapshot(repo_path, plan, &new_id, "revert");
    Ok((format!("reverted {}", commit_id.short()), after))
}

/// Blocking part of commit (tree-build + write). Scales with the staged tree.
/// Returns the new commit id alongside the after-state so the UI finish step can
/// clear the branch draft on the main thread.
pub(crate) fn commit_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    message: &str,
) -> Result<(String, StateSummary), String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    // (Commit's plan is a HEAD snapshot; preflight detects a checkout/commit
    // between plan and execute.)
    let op = kagi_git::Operation::Commit {
        message: message.to_string(),
    };
    let new_id = match repo.run(&op, plan) {
        Ok(kagi_git::OperationOutcome::Commit(c)) => c,
        Ok(_) => return Err("commit: unexpected outcome".to_string()),
        Err(e) => return Err(format!("Commit failed: {}", e)),
    };
    klog!("executed: commit {}", new_id.short());

    // Verify: re-snapshot, check HEAD is the new commit, unstaged remain.
    let after = match kagi_git::Backend::open(repo_path) {
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
                let is_dirty = snap.status.is_dirty();
                eprintln!(
                    "[kagi] verified: working tree {} after commit",
                    if is_dirty {
                        "dirty (unstaged remain)"
                    } else {
                        "clean"
                    }
                );
                StateSummary {
                    head: snap.head.display(),
                    dirty: if is_dirty {
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
    };
    Ok((new_id.short().to_string(), after))
}

/// Blocking part of stash-pop (preflight + apply-then-drop). Re-snapshots HEAD
/// for the after-state.
pub(crate) fn stash_pop_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    stash_index: usize,
) -> Result<(String, StateSummary), String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced
    // in one place. run() runs preflight_check_stash (HEAD + stash count) for
    // StashPop, so a concurrent stash push between plan and execute can't shift
    // indices and pop the WRONG entry.
    let op = kagi_git::Operation::StashPop { index: stash_index };
    repo.run(&op, plan)
        .map_err(|e| format!("Pop failed: {}", e))?;
    klog!("executed: stash-pop index={}", stash_index);

    let after = StateSummary {
        head: plan.current.head.clone(),
        dirty: "changes restored (stash removed)".to_string(),
    };
    Ok(("applied and dropped".to_string(), after))
}

/// Blocking part of standalone stash drop (ADR-0087). Deletes the stash entry
/// without touching the working tree; returns the dropped stash commit OID as
/// the oplog recovery handle.
pub(crate) fn stash_drop_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    stash_index: usize,
) -> Result<(String, StateSummary), String> {
    // StashDrop is not yet an Operation variant (drop is admin-only and goes
    // through execute_stash_drop directly); we run preflight_check_stash inline
    // here with the same two-axis guard (HEAD + stash count) that run() uses
    // for StashPop/StashApply. When StashDrop joins Operation, this becomes a
    // plain run() call.
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.preflight_check_stash(plan, plan.stash_count_at_plan())
        .map_err(|e| format!("Preflight failed: {}", e))?;

    let dropped_oid = repo
        .execute_stash_drop(stash_index)
        .map_err(|e| format!("Drop failed: {}", e))?;
    eprintln!(
        "[kagi] executed: stash-drop index={} oid={}",
        stash_index, dropped_oid
    );

    let after = StateSummary {
        head: plan.current.head.clone(),
        dirty: format!("stash@{{{}}} deleted (oid {})", stash_index, dropped_oid),
    };
    Ok(("entry deleted".to_string(), after))
}

/// Blocking part of discard (W17-DISCARD, ADR-0046). Backup-then-discard scales
/// with the working-tree content written, so it runs on the background path.
/// The returned `after` carries the path→blob backup list (the recovery handle)
/// into the oplog entry.
pub(crate) fn discard_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    paths: &[String],
) -> Result<(String, StateSummary), String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;

    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    // run() returns OperationOutcome::Discard(DiscardOutcome) which carries the
    // backup-blob list (recovery handle) into the oplog.
    let op = kagi_git::Operation::Discard {
        paths: paths.to_vec(),
    };
    let outcome = match repo.run(&op, plan) {
        Ok(kagi_git::OperationOutcome::Discard(d)) => d,
        Ok(_) => return Err("discard: unexpected outcome variant".to_string()),
        Err(e) => return Err(format!("Discard failed: {}", e)),
    };
    let summary = outcome.oplog_summary();
    klog!("executed: {}", summary);

    // Verify: re-read status; targets must have left the unstaged set.
    let dirty = match repo.working_tree_status() {
        Ok(status) => {
            let still: std::collections::HashSet<String> = status
                .unstaged
                .iter()
                .map(|f| f.path.to_string_lossy().replace('\\', "/"))
                .collect();
            let leftover = paths.iter().filter(|p| still.contains(*p)).count();
            if leftover == 0 {
                eprintln!(
                    "[kagi] verified: {} target(s) left the unstaged set",
                    paths.len()
                );
            } else {
                klog!("verify: {} target(s) still unstaged", leftover);
            }
            // Record the recovery handle (path→blob list) in the oplog after-state.
            summary.clone()
        }
        Err(e) => {
            klog!("verify: status error: {}", e);
            summary.clone()
        }
    };

    let after = StateSummary {
        head: plan.current.head.clone(),
        dirty,
    };
    let human = if outcome.backups.len() == 1 {
        format!("{} discarded", outcome.backups[0].path)
    } else {
        format!("{} files discarded", outcome.backups.len())
    };
    Ok((human, after))
}

/// Blocking part of amend (history rewrite: tree-build + commit-replace).
/// Returns (summary-suffix, after, old, new) so the UI footer can render the
/// 旧→新 SHA transition and the restore hint.
pub(crate) fn amend_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    mode: AmendMode,
    message: &str,
) -> Result<(StateSummary, CommitId, CommitId), String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    let op = kagi_git::Operation::Amend {
        mode,
        message: if message.trim().is_empty() {
            None
        } else {
            Some(message.to_string())
        },
    };
    let outcome = match repo.run(&op, plan) {
        Ok(kagi_git::OperationOutcome::Amend(o)) => o,
        Ok(_) => return Err("amend: unexpected outcome".to_string()),
        Err(e) => return Err(format!("Amend failed: {}", e)),
    };
    eprintln!(
        "[kagi] executed: amend {} -> {}",
        outcome.old.short(),
        outcome.new.short()
    );

    let after = StateSummary {
        head: format!(
            "branch @ {} (was {})",
            outcome.new.short(),
            outcome.old.short()
        ),
        dirty: "amended".to_string(),
    };
    Ok((after, outcome.old, outcome.new))
}

/// Blocking part of delete-branch (preflight → ref delete). Lightweight, but
/// kept on the background path for consistency with the other confirm flows.
pub(crate) fn delete_branch_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    branch_name: &str,
) -> Result<StateSummary, String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    let op = kagi_git::Operation::DeleteBranch {
        name: branch_name.to_string(),
    };
    repo.run(&op, plan)
        .map_err(|e| format!("Delete failed: {}", e))?;
    klog!("executed: delete-branch {}", branch_name);

    Ok(StateSummary {
        head: plan.current.head.clone(),
        dirty: format!("branch '{}' deleted", branch_name),
    })
}

pub(crate) fn branch_plan_blocking(
    repo_path: &std::path::Path,
    modal: &BranchPlanModal,
) -> Result<StateSummary, String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    match modal.kind {
        BranchPlanKind::PullFfOnly => {
            let op = kagi_git::Operation::PullBranchFf {
                branch_name: modal.branch_name.clone(),
            };
            let outcome = match repo.run(&op, &modal.plan) {
                Ok(kagi_git::OperationOutcome::Pull(o)) => o,
                Ok(_) => return Err("pull-ff: unexpected outcome".to_string()),
                Err(e) => return Err(format!("Pull failed: {}", e)),
            };
            let dirty = match outcome {
                PullOutcome::UpToDate => {
                    format!("branch '{}' already up to date", modal.branch_name)
                }
                PullOutcome::FastForward { to } => {
                    format!(
                        "branch '{}' fast-forwarded to {}",
                        modal.branch_name,
                        to.short()
                    )
                }
                PullOutcome::Merged { .. } => "unexpected merge outcome".to_string(),
            };
            Ok(StateSummary {
                head: modal.plan.current.head.clone(),
                dirty,
            })
        }
        BranchPlanKind::Push | BranchPlanKind::PushSetUpstream => {
            let set_upstream = modal.kind == BranchPlanKind::PushSetUpstream;
            let op = kagi_git::Operation::PushBranch {
                branch_name: modal.branch_name.clone(),
                set_upstream,
            };
            let outcome = match repo.run(&op, &modal.plan) {
                Ok(kagi_git::OperationOutcome::Push(o)) => o,
                Ok(_) => return Err("push-branch: unexpected outcome".to_string()),
                Err(e) => return Err(format!("Push failed: {}", e)),
            };
            Ok(StateSummary {
                head: modal.plan.current.head.clone(),
                dirty: format!(
                    "branch '{}' pushed {} commit(s){}",
                    modal.branch_name,
                    outcome.pushed,
                    if outcome.set_upstream {
                        " and upstream set"
                    } else {
                        ""
                    }
                ),
            })
        }
    }
}

pub(crate) fn set_upstream_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    branch_name: &str,
    upstream: &str,
) -> Result<StateSummary, String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    let op = kagi_git::Operation::SetUpstream {
        branch_name: branch_name.to_string(),
        upstream: upstream.to_string(),
    };
    repo.run(&op, plan)
        .map_err(|e| format!("Set upstream failed: {}", e))?;
    Ok(StateSummary {
        head: plan.current.head.clone(),
        dirty: format!("branch '{}' upstream set to '{}'", branch_name, upstream),
    })
}

pub(crate) fn rename_branch_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    old_name: &str,
    new_name: &str,
) -> Result<StateSummary, String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
    let op = kagi_git::Operation::RenameBranch {
        old_name: old_name.to_string(),
        new_name: new_name.to_string(),
    };
    repo.run(&op, plan)
        .map_err(|e| format!("Rename failed: {}", e))?;
    Ok(StateSummary {
        head: plan.predicted.head.clone(),
        dirty: format!("branch '{}' renamed to '{}'", old_name, new_name),
    })
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
) -> Result<StateSummary, String> {
    let mut repo =
        kagi_git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    // ADR-0104 Phase 2: route through Backend::run so preflight is enforced.
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
    repo.run(&op, plan).map_err(|e| {
        if allow_existing_branch {
            format!("Open worktree failed: {}", e)
        } else {
            format!("Create worktree failed: {}", e)
        }
    })?;
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

    Ok(plan.predicted.clone())
}

/// Re-snapshot after a new-commit op (cherry-pick / revert) for the after-state,
/// logging the verified HEAD. Falls back to the plan prediction on failure.
pub(crate) fn verify_new_commit_snapshot(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    new_id: &CommitId,
    op: &str,
) -> StateSummary {
    match kagi_git::Backend::open(repo_path) {
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
