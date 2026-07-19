//! Merged-branch cleanup — collect / plan / execute (ADR-0128)
//!
//! Reduces the repository to per-branch facts ([`BranchCleanupInput`]), lets
//! the pure `kagi_domain::branch_cleanup` module classify them, and implements
//! the guarded batch deletion:
//!
//! - [`collect_branch_cleanup`] — build the cleanup table rows (read-only).
//! - [`plan_delete_merged_branches`] — validate a selection against a fresh
//!   collect and produce an [`OperationPlan`].
//! - [`execute_delete_merged_branches`] — re-verify each tip OID right before
//!   deleting (local via git2, remote via one `ls-remote`), then delete.
//!
//! git2's `Branch::delete()` performs **no** merged check (unlike `git branch
//! -d`), so the OID + ancestor re-verification here is the only safety valve —
//! never call the raw delete outside this pipeline. Remote deletion uses a
//! plain `git push origin --delete` after the `ls-remote` OID comparison;
//! `--force-with-lease` is not used anywhere in this codebase (see `push.rs`),
//! so a small race window between the check and the push remains and is
//! covered by the oplog recording every deleted tip OID for recovery.
//!
//! Deleting a branch is ref-only: the working tree, index, and HEAD are never
//! touched. Remote-first ordering: the flaky (network) half runs before the
//! reliable (local ref) half, so a mid-branch failure can only leave the
//! *local* branch behind — the recoverable direction.

use std::collections::HashMap;

use super::branch::pre_clean_branch_config;
use super::*;

pub use kagi_domain::branch_cleanup::{
    build_rows, copy_all_text, BranchCleanupInput, BranchCleanupRow, CleanupDeleteTarget,
    CleanupDeleted, CleanupOutcome, MergedBranchStatus,
};

/// First-parent walk cap on main when collecting merge commits. Branches
/// merged further back than this are still classified (ancestor check is
/// exact); only the merge *timestamp* and the grown detection lose coverage.
const FIRST_PARENT_WALK_CAP: usize = 4000;

// ────────────────────────────────────────────────────────────
// collect_branch_cleanup
// ────────────────────────────────────────────────────────────

/// Resolve the default branch name: `origin/HEAD` symref if present, else the
/// first of `main` / `master` that exists locally or on origin.
fn default_branch_name(repo: &Repository) -> String {
    if let Ok(head_ref) = repo.find_reference("refs/remotes/origin/HEAD") {
        if let Ok(Some(sym)) = head_ref.symbolic_target() {
            if let Some(name) = sym.strip_prefix("refs/remotes/origin/") {
                return name.to_string();
            }
        }
    }
    for cand in ["main", "master"] {
        if repo.find_branch(cand, BranchType::Local).is_ok()
            || repo
                .find_reference(&format!("refs/remotes/origin/{}", cand))
                .is_ok()
        {
            return cand.to_string();
        }
    }
    "main".to_string()
}

/// Tip OID of the default branch — local branch preferred, `origin/<name>`
/// as fallback. `None` when neither exists (unborn / exotic repo).
fn resolve_main_tip(repo: &Repository, default: &str) -> Option<git2::Oid> {
    if let Ok(b) = repo.find_branch(default, BranchType::Local) {
        if let Some(oid) = b.get().target() {
            return Some(oid);
        }
    }
    repo.find_reference(&format!("refs/remotes/origin/{}", default))
        .ok()
        .and_then(|r| r.target())
}

/// One merge commit found on main's first-parent line: the merged-in parent
/// (`parent^2`…), the merge commit itself, and its commit time.
struct MainMerge {
    merged_tip: git2::Oid,
    merge_commit: git2::Oid,
    time: i64,
}

/// Walk main's first-parent history and record every merge commit's non-first
/// parents, newest first. This is what dates a merged branch and what detects
/// the merged-then-grown (WARN) pattern.
fn collect_main_merges(repo: &Repository, main_tip: git2::Oid) -> Vec<MainMerge> {
    let mut merges = Vec::new();
    let mut cursor = repo.find_commit(main_tip).ok();
    let mut steps = 0;
    while let Some(c) = cursor {
        if steps >= FIRST_PARENT_WALK_CAP {
            break;
        }
        steps += 1;
        if c.parent_count() >= 2 {
            let t = c.time().seconds();
            for p in 1..c.parent_count() {
                if let Ok(pid) = c.parent_id(p) {
                    merges.push(MainMerge {
                        merged_tip: pid,
                        merge_commit: c.id(),
                        time: t,
                    });
                }
            }
        }
        cursor = c.parent(0).ok();
    }
    merges
}

/// Build the Branch Cleanup table (ADR-0128): every local branch and every
/// `origin/*` remote branch, merged into one logical row per name, classified
/// by `kagi_domain::branch_cleanup`. Read-only.
///
/// `now` is Unix seconds, passed in by the caller for the staleness check.
pub fn collect_branch_cleanup(
    repo: &Repository,
    now: i64,
) -> Result<Vec<BranchCleanupRow>, GitError> {
    let head = resolve_head(repo)?;
    let current_branch = match &head {
        Head::Attached { branch, .. } => Some(branch.clone()),
        _ => None,
    };
    let default = default_branch_name(repo);
    let Some(main_tip) = resolve_main_tip(repo, &default) else {
        return Ok(Vec::new());
    };

    let merges = collect_main_merges(repo, main_tip);
    // Newest occurrence wins when the same tip shows up twice.
    let mut merge_time_by_tip: HashMap<git2::Oid, i64> = HashMap::new();
    for m in &merges {
        merge_time_by_tip.entry(m.merged_tip).or_insert(m.time);
    }

    // name → (local tip, remote tip, upstream gone)
    let mut by_name: HashMap<String, (Option<git2::Oid>, Option<git2::Oid>, bool)> = HashMap::new();

    let locals = repo
        .branches(Some(BranchType::Local))
        .map_err(|e| GitError::Other(format!("branch iteration failed: {}", e.message())))?;
    for item in locals {
        let (branch, _) =
            item.map_err(|e| GitError::Other(format!("branch iteration failed: {}", e.message())))?;
        let name = match branch.name() {
            Ok(Some(n)) => n.to_string(),
            _ => continue,
        };
        let Some(tip) = branch.get().target() else {
            continue;
        };
        // `[gone]`: an upstream is configured but the remote-tracking ref no
        // longer exists — the squash-merge heuristic (GitHub deletes the
        // branch when the PR is merged).
        let configured = repo
            .branch_upstream_name(&format!("refs/heads/{}", name))
            .is_ok();
        let gone = configured && branch.upstream().is_err();
        by_name.insert(name, (Some(tip), None, gone));
    }

    let remotes = repo
        .branches(Some(BranchType::Remote))
        .map_err(|e| GitError::Other(format!("branch iteration failed: {}", e.message())))?;
    for item in remotes {
        let (branch, _) =
            item.map_err(|e| GitError::Other(format!("branch iteration failed: {}", e.message())))?;
        let full = match branch.name() {
            Ok(Some(n)) => n.to_string(),
            _ => continue,
        };
        if full.ends_with("/HEAD") {
            continue;
        }
        let Some(name) = full.strip_prefix("origin/") else {
            continue; // origin only (ADR-0128 non-goal: other remotes)
        };
        let Some(tip) = branch.get().target() else {
            continue;
        };
        by_name
            .entry(name.to_string())
            .or_insert((None, None, false))
            .1 = Some(tip);
    }

    let mut inputs = Vec::new();
    for (name, (local_tip, remote_tip, gone)) in by_name {
        let Some(tip) = local_tip.or(remote_tip) else {
            continue;
        };
        let is_ancestor =
            tip == main_tip || repo.graph_descendant_of(main_tip, tip).unwrap_or(false);

        let mut merged_at = merge_time_by_tip.get(&tip).copied();
        let mut grown_ahead = None;
        if !is_ancestor {
            // Merged-then-grown: some merge commit in main merged an ancestor
            // of this tip — and the merge commit itself is NOT an ancestor of
            // the tip. The second condition is what separates a genuinely
            // merged-then-grown branch (develop) from a branch that merely
            // forked off main after the merge: the fork inherits main's whole
            // history, merge commits included, so for it the merge commit IS
            // an ancestor. Take the newest qualifying merge for the timestamp.
            for m in &merges {
                let merged_tip_in_history =
                    repo.graph_descendant_of(tip, m.merged_tip).unwrap_or(false);
                if !merged_tip_in_history {
                    continue;
                }
                let merge_in_history = tip == m.merge_commit
                    || repo
                        .graph_descendant_of(tip, m.merge_commit)
                        .unwrap_or(false);
                if merge_in_history {
                    continue;
                }
                let (ahead, _) = repo.graph_ahead_behind(tip, main_tip).unwrap_or((0, 0));
                merged_at = Some(m.time);
                grown_ahead = Some(ahead);
                break; // merges are newest-first
            }
        }

        let tip_committed_at = repo
            .find_commit(tip)
            .map(|c| c.time().seconds())
            .unwrap_or(now);

        inputs.push(BranchCleanupInput {
            is_current: current_branch.as_deref() == Some(name.as_str()),
            is_default: name == default,
            name,
            local_tip: local_tip.map(|o| CommitId(o.to_string())),
            remote_tip: remote_tip.map(|o| CommitId(o.to_string())),
            tip_is_ancestor_of_main: is_ancestor,
            merged_at,
            grown_ahead,
            upstream_gone: gone,
            tip_committed_at,
        });
    }

    Ok(build_rows(&inputs, now))
}

// ────────────────────────────────────────────────────────────
// plan_delete_merged_branches
// ────────────────────────────────────────────────────────────

/// Validate a selection of delete targets against a **fresh** collect and
/// produce an [`OperationPlan`].
///
/// # Blocker conditions
///
/// - Empty selection.
/// - A target is no longer a cleanup candidate (branch gone, or reclassified
///   to a non-deletable class such as `MergedThenGrown`).
/// - A target's local or remote tip moved since the table was built.
///
/// # Warning conditions
///
/// - A `SquashMergedLikely` target is included: the `[gone]` heuristic has no
///   local proof of the merge.
/// - Any target has a remote half: deletion writes to origin over the network.
pub fn plan_delete_merged_branches(
    repo: &Repository,
    now: i64,
    targets: &[CleanupDeleteTarget],
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let current = StateSummary {
        head: head.display(),
        dirty: status_summary_display(&status),
    };

    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    let mut preview_commits = Vec::new();

    if targets.is_empty() {
        blockers.push("No branches selected for deletion.".to_string());
    }

    let fresh = collect_branch_cleanup(repo, now)?;
    for t in targets {
        let Some(row) = fresh.iter().find(|r| r.name == t.name) else {
            blockers.push(format!(
                "Branch '{}' is no longer a cleanup candidate. Refresh the list.",
                t.name
            ));
            continue;
        };
        if !row.deletable {
            blockers.push(format!(
                "Branch '{}' is not safely deletable (it may have grown new commits since merge). Refresh the list.",
                t.name
            ));
            continue;
        }
        if row.local_tip != t.local_tip || row.remote_tip != t.remote_tip {
            blockers.push(format!(
                "Branch '{}' moved since the list was built. Refresh the list.",
                t.name
            ));
            continue;
        }
        let mut places = Vec::new();
        if let Some(l) = &t.local_tip {
            places.push(format!("local {}", l.short()));
        }
        if let Some(r) = &t.remote_tip {
            places.push(format!("origin {}", r.short()));
        }
        preview_commits.push(format!("{}  ({})", t.name, places.join(", ")));
    }

    if targets
        .iter()
        .any(|t| t.status == MergedBranchStatus::SquashMergedLikely)
    {
        warnings.push(
            "Some branches are only *likely* squash-merged (upstream gone); \
             there is no local proof of the merge."
                .to_string(),
        );
    }
    if targets.iter().any(|t| t.remote_tip.is_some()) {
        warnings.push("Remote branches on 'origin' will be deleted (network write).".to_string());
    }

    let recovery = "Every deleted tip OID is recorded in the oplog. To restore:\n  \
                    git branch <name> <oid>          (local)\n  \
                    git push origin <oid>:refs/heads/<name>   (remote)"
        .to_string();

    Ok(OperationPlan {
        title: format!("Delete {} merged branch(es)", targets.len()),
        predicted: StateSummary {
            head: current.head.clone(),
            dirty: current.dirty.clone(),
        },
        current,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files: Vec::new(),
        preview_commits,
        destructive: false,
    })
}

// ────────────────────────────────────────────────────────────
// execute_delete_merged_branches
// ────────────────────────────────────────────────────────────

/// Delete the targeted branches, remote halves first, re-verifying every tip
/// OID immediately before deletion.
///
/// Per-branch failures do **not** abort the run (each deletion is
/// independent); they are collected in [`CleanupOutcome::failed`]. Only a
/// global preflight failure (HEAD moved since planning) returns `Err`.
pub fn execute_delete_merged_branches(
    repo: &Repository,
    repo_path: &Path,
    plan: &OperationPlan,
    targets: &[CleanupDeleteTarget],
) -> Result<CleanupOutcome, GitError> {
    preflight_check(repo, plan)?;

    let default = default_branch_name(repo);
    let main_tip = resolve_main_tip(repo, &default);

    let mut outcome = CleanupOutcome::default();
    let mut failed: HashMap<String, String> = HashMap::new();

    // ── Phase 1: one ls-remote for every remote half, OID comparison ──
    let remote_targets: Vec<&CleanupDeleteTarget> =
        targets.iter().filter(|t| t.remote_tip.is_some()).collect();
    // Branch names whose remote half is verified and should be pushed away.
    let mut to_push: Vec<&CleanupDeleteTarget> = Vec::new();
    // Branch names already absent on the remote (stale tracking ref only).
    let mut already_gone: Vec<&CleanupDeleteTarget> = Vec::new();

    if !remote_targets.is_empty() {
        let mut args = vec!["ls-remote".to_string(), "origin".to_string()];
        for t in &remote_targets {
            args.push(format!("refs/heads/{}", t.name));
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        match run_git(repo_path, &arg_refs) {
            Ok(out) if out.status == 0 => {
                let mut live: HashMap<&str, &str> = HashMap::new();
                for line in out.stdout.lines() {
                    if let Some((oid, refname)) = line.split_once('\t') {
                        if let Some(name) = refname.strip_prefix("refs/heads/") {
                            live.insert(name, oid.trim());
                        }
                    }
                }
                for t in &remote_targets {
                    let planned = t.remote_tip.as_ref().expect("filtered on remote_tip");
                    match live.get(t.name.as_str()) {
                        Some(oid) if *oid == planned.0 => to_push.push(t),
                        Some(oid) => {
                            failed.insert(
                                t.name.clone(),
                                format!(
                                    "remote branch moved since plan ({} → {}). Fetch and re-plan.",
                                    planned.short(),
                                    &oid[..8.min(oid.len())]
                                ),
                            );
                        }
                        None => already_gone.push(t),
                    }
                }
            }
            Ok(out) => {
                for t in &remote_targets {
                    failed.insert(t.name.clone(), format!("ls-remote failed: {}", out.stderr));
                }
            }
            Err(e) => {
                for t in &remote_targets {
                    failed.insert(t.name.clone(), format!("ls-remote failed: {}", e));
                }
            }
        }
    }

    // ── Phase 2: batch remote delete, per-branch fallback on failure ──
    let mut remote_deleted: Vec<String> = Vec::new();
    if !to_push.is_empty() {
        let mut args = vec!["push", "origin", "--delete"];
        args.extend(to_push.iter().map(|t| t.name.as_str()));
        let batch_ok = matches!(run_git(repo_path, &args), Ok(out) if out.status == 0);
        if batch_ok {
            remote_deleted.extend(to_push.iter().map(|t| t.name.clone()));
        } else {
            for t in &to_push {
                match run_git(repo_path, &["push", "origin", "--delete", &t.name]) {
                    Ok(out) if out.status == 0 => remote_deleted.push(t.name.clone()),
                    Ok(out) => {
                        failed.insert(
                            t.name.clone(),
                            format!("push --delete failed: {}", out.stderr.trim()),
                        );
                    }
                    Err(e) => {
                        failed.insert(t.name.clone(), format!("push --delete failed: {}", e));
                    }
                }
            }
        }
    }
    // Prune stale tracking refs for branches the remote no longer has, and
    // make sure pushed deletions dropped theirs too (push normally does).
    for t in already_gone.iter().chain(to_push.iter()) {
        if let Ok(mut r) = repo.find_reference(&format!("refs/remotes/origin/{}", t.name)) {
            if remote_deleted.contains(&t.name) || already_gone.iter().any(|g| g.name == t.name) {
                let _ = r.delete();
            }
        }
    }

    // ── Phase 3: local deletes, OID + ancestor re-verified ──
    for t in targets {
        if failed.contains_key(&t.name) {
            continue;
        }
        let mut deleted_local = false;
        if let Some(planned) = &t.local_tip {
            match delete_local_verified(repo, t, planned, main_tip) {
                Ok(()) => deleted_local = true,
                Err(msg) => {
                    failed.insert(t.name.clone(), msg);
                    continue;
                }
            }
        }
        let was_remote_deleted = remote_deleted.contains(&t.name);
        if deleted_local || was_remote_deleted {
            outcome.deleted.push(CleanupDeleted {
                name: t.name.clone(),
                local_tip: deleted_local.then(|| t.local_tip.clone()).flatten(),
                remote_tip: was_remote_deleted.then(|| t.remote_tip.clone()).flatten(),
            });
        }
    }

    outcome.failed = failed.into_iter().collect();
    outcome.failed.sort();
    Ok(outcome)
}

/// Delete one local branch after re-verifying its tip OID (and, for
/// `FullyMerged` targets, that the tip is still an ancestor of main).
fn delete_local_verified(
    repo: &Repository,
    target: &CleanupDeleteTarget,
    planned: &CommitId,
    main_tip: Option<git2::Oid>,
) -> Result<(), String> {
    let mut branch = repo
        .find_branch(&target.name, BranchType::Local)
        .map_err(|_| "local branch disappeared since plan. Refresh the list.".to_string())?;
    let tip = branch
        .get()
        .target()
        .ok_or_else(|| "branch has no target OID".to_string())?;
    if tip.to_string() != planned.0 {
        return Err(format!(
            "local branch moved since plan ({} → {}). Refresh the list.",
            planned.short(),
            short_oid(tip)
        ));
    }
    if target.status == MergedBranchStatus::FullyMerged {
        let still_merged = match main_tip {
            Some(m) => tip == m || repo.graph_descendant_of(m, tip).unwrap_or(false),
            None => false,
        };
        if !still_merged {
            return Err("branch is no longer merged into the default branch.".to_string());
        }
    }
    pre_clean_branch_config(repo, &target.name);
    branch
        .delete()
        .map_err(|e| format!("branch delete failed: {}", e.message()))?;
    if repo.find_branch(&target.name, BranchType::Local).is_ok() {
        return Err("branch still exists after delete — unexpected state".to_string());
    }
    Ok(())
}
