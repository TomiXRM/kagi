//! `git replay --onto`: rebase a branch by **ref update only** (#344 slice 1,
//! ADR-0211 決定 4).
//!
//! No working tree or index is touched — not the current worktree's, not the
//! one that has `branch` checked out. The plan is what git itself prints
//! (`update <ref> <new> <old>` lines; the new commits are already written),
//! the execute step feeds exactly those lines to `git update-ref --stdin` as
//! one transaction, and git's own old-value check refuses a branch that
//! moved in between.
//!
//! Version handling (ADR-0211 §2): below git 2.53 `replay` only prints and
//! rejects `--ref-action`; from 2.53 it **updates refs itself** unless told
//! `--ref-action=print`. Both `-c replay.refAction=print` (ignored by old
//! git, honoured by new) and, on 2.53+, the explicit flag are passed, and the
//! plan verifies afterwards that no ref moved — a replay that wrote anything
//! is reported as an error, never as a plan.

use super::*;
use crate::cli::GitFeatures;
use kagi_domain::plan_note::{RebaseNote, RebaseRecovery, RebaseTitle};
use kagi_domain::ref_update::{parse_update_ref_lines, RefScript, RefVerify};
use kagi_domain::remote::shell_quote;

/// How many moved refs the plan lists before folding the rest.
const UPDATE_SAMPLE: usize = 5;

/// The `git replay` argv for `branch` onto `onto`, print-only on every
/// supported git (see module doc).
fn replay_args<'a>(features: GitFeatures, onto: &'a str, range: &'a str) -> Vec<&'a str> {
    let mut args = vec!["-c", "replay.refAction=print", "replay"];
    if features.replay_ref_action_default_update {
        args.push("--ref-action=print");
    }
    args.extend(["--onto", onto, range]);
    args
}

fn short(oid: &str) -> &str {
    &oid[..oid.len().min(7)]
}

/// Which worktree has `branch` checked out, if any (fail-closed: an
/// unreadable worktree registration is an error, not "none").
fn worktree_with_branch(repo: &Repository, branch: &str) -> Result<Option<PathBuf>, GitError> {
    let repositories = super::branch_delete_safety::repositories(repo)?;
    super::branch_delete_safety::checked_out_at(&repositories, branch)
}

/// Analyse replaying `branch` onto `onto`. Runs `git replay` in print mode:
/// its output *is* the preview, so nothing in the plan is Kagi's guess.
pub fn plan_replay_onto(
    repo: &Repository,
    repo_dir: &Path,
    branch: &str,
    onto: &str,
) -> Result<OperationPlan, GitError> {
    check_operand("branch", branch)?;
    check_operand("onto", onto)?;
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;
    let dirty = status_summary_display(&status);
    let title = RebaseTitle::ReplayOnto {
        branch: branch.to_string(),
        onto: onto.to_string(),
    };
    let features = GitFeatures::detected();
    let finish = |warnings: Vec<PlanNote>,
                  blockers: Vec<PlanNote>,
                  from: String,
                  preview_commits: Vec<String>| {
        Ok(OperationPlan {
            disposition: PlanDisposition::for_blockers(&blockers),
            title: PlanTitle::Rebase(title.clone()),
            current: StateSummary {
                head: head.display(),
                dirty: dirty.clone(),
            },
            predicted: StateSummary {
                head: head.display(),
                dirty: dirty.clone(),
            },
            warnings,
            blockers,
            recovery: Some(PlanRecovery {
                kind: RecoveryKind::Rebase(RebaseRecovery::ReplayOnto {
                    branch: branch.to_string(),
                    from: from.clone(),
                }),
                commands: vec![format!(
                    "git update-ref {} {}",
                    shell_quote(&format!("refs/heads/{branch}")),
                    shell_quote(&from)
                )],
            }),
            head_at_plan: head.clone(),
            stash_count_at_plan: 0,
            stash_identity: None,
            pull_identity: None,
            worktree_digest: None,
            preview_files: Vec::new(),
            preview_commits,
            // ADR-0023: rewriting history is destructive (two-stage confirm).
            destructive: true,
            equivalent_command: (!cfg!(windows)).then(|| {
                format!(
                    "git replay --onto {} {} | git update-ref --stdin",
                    shell_quote(onto),
                    shell_quote(&format!("{onto}..{branch}"))
                )
            }),
        })
    };

    if !features.replay_onto {
        return Err(GitError::Other(
            "git replay requires git 2.44 or newer".to_string(),
        ));
    }
    let Ok(branch_ref) = repo.find_branch(branch, BranchType::Local) else {
        return Err(GitError::Other(format!("branch '{branch}' not found")));
    };
    let from = branch_ref
        .get()
        .target()
        .ok_or_else(|| GitError::Other(format!("branch '{branch}' has no target")))?
        .to_string();
    if repo.revparse_single(onto).is_err() {
        return finish(
            Vec::new(),
            vec![PlanNote::Rebase(RebaseNote::InvalidOnto {
                onto: onto.to_string(),
            })],
            from,
            Vec::new(),
        );
    }
    let range = format!("{onto}..{branch}");
    // Pin `onto` where the user saw it: if it is a ref, the transaction
    // verifies it (git refuses the whole script if it moved); a bare object
    // id cannot move.
    let onto_pin = match repo.resolve_reference_from_short_name(onto) {
        Ok(r) => match (r.name(), r.resolve().ok().and_then(|d| d.target())) {
            (Ok(name), Some(oid)) => Some(RefVerify {
                reference: name.to_string(),
                expected: oid.to_string(),
            }),
            _ => None,
        },
        Err(_) => None,
    };

    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    // Empty range: on git 2.50.1 `replay` of nothing exits 1 — the same code
    // as a conflict — so "nothing to do" is decided here, before replay runs.
    let count = run_git(repo_dir, &["rev-list", "--count", &range])?;
    if count.status == 0 && count.stdout.trim() == "0" {
        blockers.push(PlanNote::Rebase(RebaseNote::ReplayNothingToDo {
            branch: branch.to_string(),
            onto: onto.to_string(),
        }));
        return finish(warnings, blockers, from, Vec::new());
    }

    // Merges: git replay refuses them (exit 128); say so before running it.
    let merges = run_git(repo_dir, &["rev-list", "--merges", "--count", &range])?;
    let merge_count: usize = merges.stdout.trim().parse().unwrap_or(0);
    if merges.status == 0 && merge_count > 0 {
        blockers.push(PlanNote::Rebase(RebaseNote::ReplayRangeHasMerges {
            branch: branch.to_string(),
            count: merge_count,
        }));
    }

    // The worktree that has the branch checked out: dirty → blocker, clean →
    // warning that its index/tree are left behind (ADR-0211 §1 iii / §5).
    if let Some(path) = worktree_with_branch(repo, branch)? {
        let wt = Repository::open(&path).map_err(|e| GitError::Other(e.message().to_string()))?;
        let wt_status = working_tree_status(&wt)?;
        let note = if wt_status.is_dirty() {
            RebaseNote::ReplayWorktreeDirty {
                branch: branch.to_string(),
                path: path.display().to_string(),
            }
        } else {
            RebaseNote::ReplayWorktreeStale {
                branch: branch.to_string(),
                path: path.display().to_string(),
            }
        };
        if wt_status.is_dirty() {
            blockers.push(PlanNote::Rebase(note));
        } else {
            warnings.push(PlanNote::Rebase(note));
        }
    }

    if !blockers.is_empty() {
        return finish(warnings, blockers, from, Vec::new());
    }

    // Replay rewrites commits: signatures on the originals do not survive.
    let signed = run_git(repo_dir, &["log", "--format=%G?", &range])?;
    let signed_count = signed
        .stdout
        .lines()
        .filter(|l| !matches!(l.trim(), "" | "N"))
        .count();
    if signed.status == 0 && signed_count > 0 {
        warnings.push(PlanNote::Rebase(RebaseNote::ReplayDropsSignatures {
            count: signed_count,
        }));
    }

    let before: Vec<(String, String)> = local_branch_tips(repo)?;
    let out = run_git(repo_dir, &replay_args(features, onto, &range))?;
    // Replay must not have moved anything (ADR-0211 §2 double-check).
    let after = local_branch_tips(repo)?;
    if before != after {
        return Err(GitError::Other(
            "git replay updated refs by itself while planning; refusing (expected print-only mode)"
                .to_string(),
        ));
    }
    match out.status {
        0 => {}
        1 => {
            blockers.push(PlanNote::Rebase(RebaseNote::ReplayConflicts {
                branch: branch.to_string(),
                onto: onto.to_string(),
            }));
            return finish(warnings, blockers, from, Vec::new());
        }
        code => {
            return Err(GitError::Other(format!(
                "git replay failed (exit {code}): {}",
                out.stderr.trim()
            )));
        }
    }
    let mut script = parse_update_ref_lines(&out.stdout)
        .map_err(|e| GitError::Other(format!("unexpected git replay output: {e}")))?;
    script.verifies.extend(onto_pin);
    let updates = &script.updates;
    if updates.is_empty() {
        blockers.push(PlanNote::Rebase(RebaseNote::ReplayNothingToDo {
            branch: branch.to_string(),
            onto: onto.to_string(),
        }));
        return finish(warnings, blockers, from, Vec::new());
    }
    let branch_update = updates
        .iter()
        .find(|u| u.reference == format!("refs/heads/{branch}"))
        .ok_or_else(|| {
            GitError::Other(format!(
                "git replay printed no update for refs/heads/{branch}"
            ))
        })?;
    if branch_update.old != from {
        return Err(GitError::Other(format!(
            "git replay saw '{branch}' at {} but the plan read {}; please re-plan",
            short(&branch_update.old),
            short(&from)
        )));
    }
    let sample: Vec<String> = updates
        .iter()
        .take(UPDATE_SAMPLE)
        .map(|u| format!("{} {}→{}", u.reference, short(&u.old), short(&u.new)))
        .collect();
    warnings.push(PlanNote::Rebase(RebaseNote::ReplayUpdates {
        count: updates.len(),
        sample,
        more: updates.len().saturating_sub(UPDATE_SAMPLE),
    }));
    // The script travels in the plan verbatim: `preview_commits` is the only
    // free-form list `OperationPlan` has, and execute re-parses it.
    finish(warnings, Vec::new(), from, script.lines())
}

/// `(ref name, oid)` of every local branch, for the print-only check.
fn local_branch_tips(repo: &Repository) -> Result<Vec<(String, String)>, GitError> {
    let mut tips = Vec::new();
    for b in repo
        .branches(Some(BranchType::Local))
        .map_err(|e| GitError::Other(e.message().to_string()))?
    {
        let (b, _) = b.map_err(|e| GitError::Other(e.message().to_string()))?;
        let name = b.get().name().unwrap_or_default().to_string();
        let oid = b.get().target().map(|o| o.to_string()).unwrap_or_default();
        tips.push((name, oid));
    }
    tips.sort();
    Ok(tips)
}

/// The ref script a plan carries (what `plan_replay_onto` printed, plus the
/// `onto` pin).
pub fn replay_plan_script(plan: &OperationPlan) -> Result<RefScript, GitError> {
    parse_update_ref_lines(&plan.preview_commits.join("\n"))
        .map_err(|e| GitError::Other(format!("replay plan is not a ref update list: {e}")))
}

/// Preflight: HEAD unchanged, `onto` and every moved ref still where the
/// plan saw them, the range still merge-free, and the branch's worktree
/// still clean.
pub fn preflight_replay_onto(
    repo: &Repository,
    repo_dir: &Path,
    plan: &OperationPlan,
    branch: &str,
    onto: &str,
) -> Result<RefScript, GitError> {
    if !plan.blockers.is_empty() {
        return Err(GitError::Other(
            "replay-onto refused: plan has blockers".to_string(),
        ));
    }
    preflight_check(repo, plan)?;
    let script = replay_plan_script(plan)?;
    if script.updates.is_empty() {
        return Err(GitError::Other(
            "replay-onto refused: the plan carries no ref updates".to_string(),
        ));
    }
    for v in &script.verifies {
        let now = repo
            .refname_to_id(&v.reference)
            .map(|o| o.to_string())
            .map_err(|e| GitError::Other(format!("{}: {}", v.reference, e.message())))?;
        if now != v.expected {
            return Err(GitError::Other(format!(
                "'{onto}' moved after planning ({} → {}); please re-plan",
                short(&v.expected),
                short(&now)
            )));
        }
    }
    for u in &script.updates {
        let now = repo
            .refname_to_id(&u.reference)
            .map(|o| o.to_string())
            .map_err(|e| GitError::Other(format!("{}: {}", u.reference, e.message())))?;
        if now != u.old {
            return Err(GitError::Other(format!(
                "{} moved after planning ({} → {}); please re-plan",
                u.reference,
                short(&u.old),
                short(&now)
            )));
        }
    }
    let range = format!("{onto}..{branch}");
    let merges = run_git(repo_dir, &["rev-list", "--merges", "--count", &range])?;
    if merges.status == 0 && merges.stdout.trim().parse::<usize>().unwrap_or(0) > 0 {
        return Err(GitError::Other(
            "replay-onto refused at preflight: the range now contains a merge".to_string(),
        ));
    }
    if let Some(path) = worktree_with_branch(repo, branch)? {
        let wt = Repository::open(&path).map_err(|e| GitError::Other(e.message().to_string()))?;
        if working_tree_status(&wt)?.is_dirty() {
            return Err(GitError::Other(format!(
                "replay-onto refused at preflight: '{branch}' is checked out in {} with uncommitted changes",
                path.display()
            )));
        }
    }
    Ok(script)
}

/// Execute: preflight → retain **every** moved ref's old tip under
/// `refs/kagi/backups/` → `git update-ref --stdin` (one transaction; git
/// re-checks `onto` and every old value) → verify every ref is at its new
/// value.
pub(crate) fn execute_replay_onto(
    repo: &Repository,
    repo_dir: &Path,
    plan: &OperationPlan,
    branch: &str,
    onto: &str,
    backup_refs: &mut Vec<String>,
) -> Result<crate::OperationOutcome, GitError> {
    let script = preflight_replay_onto(repo, repo_dir, plan, branch, onto)?;
    let updates = &script.updates;
    let branch_ref = format!("refs/heads/{branch}");
    let branch_update = updates
        .iter()
        .find(|u| u.reference == branch_ref)
        .ok_or_else(|| GitError::Other(format!("plan carries no update for {branch_ref}")))?;
    // One backup per moved ref, in script order; `backups[i]` belongs to
    // `updates[i]` so a multi-branch replay is fully recoverable.
    let operation_id = super::backup::operation_id();
    let mut backups = Vec::with_capacity(updates.len());
    for (index, u) in updates.iter().enumerate() {
        let old =
            git2::Oid::from_str(&u.old).map_err(|e| GitError::Other(e.message().to_string()))?;
        let reference = super::backup::retain_object(repo, &operation_id, index, old)?;
        backup_refs.push(reference.clone());
        backups.push(kagi_domain::operation::ReplayBackup {
            reference: u.reference.clone(),
            old: u.old.clone(),
            backup: reference,
        });
    }
    let reference = backups
        .iter()
        .find(|b| b.reference == branch_ref)
        .map(|b| b.backup.clone())
        .unwrap_or_default();

    let script = script.transaction();
    let out = crate::cli::run_git_with_options(
        repo_dir,
        &["update-ref", "--stdin"],
        crate::cli::GitCliOptions {
            stdin: Some(script.as_bytes()),
            ..Default::default()
        },
    )?;
    if out.status != 0 {
        return Err(GitError::Other(format!(
            "git update-ref refused the transaction (nothing was changed): {}",
            out.stderr.trim()
        )));
    }
    // Verify.
    for u in updates {
        let now = repo
            .refname_to_id(&u.reference)
            .map(|o| o.to_string())
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        if now != u.new {
            return Err(GitError::Other(format!(
                "{} is at {} after update-ref, expected {} — unexpected state",
                u.reference,
                short(&now),
                short(&u.new)
            )));
        }
    }
    Ok(crate::OperationOutcome::ReplayOnto {
        branch: branch.to_string(),
        from: branch_update.old.clone(),
        to: branch_update.new.clone(),
        reference,
        backups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_args_print_only_on_both_sides_of_2_53() {
        let old = GitFeatures::for_version(Some(crate::cli::GitVersion::new(2, 50, 1)));
        assert_eq!(
            replay_args(old, "main", "main..feat"),
            vec![
                "-c",
                "replay.refAction=print",
                "replay",
                "--onto",
                "main",
                "main..feat"
            ]
        );
        let new = GitFeatures::for_version(Some(crate::cli::GitVersion::new(2, 53, 0)));
        assert_eq!(
            replay_args(new, "main", "main..feat"),
            vec![
                "-c",
                "replay.refAction=print",
                "replay",
                "--ref-action=print",
                "--onto",
                "main",
                "main..feat"
            ]
        );
    }
}
