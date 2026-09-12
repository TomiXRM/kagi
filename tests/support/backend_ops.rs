//! Fixture adapters for the public Backend boundary (#566).
//! No raw executor calls. Existing supplied plans remain bound through run;
//! primitive-only fixtures build a fresh plan immediately before execution.
#![allow(dead_code, unused_imports, unused_variables)]
use git2::Repository;
use kagi_domain::absorb::{AbsorbOutcome, AbsorbPlan};
use kagi_git::{conflicts::*, ops::*, *};
use std::path::Path;

fn backend(repo: &Repository) -> Result<Backend, GitError> {
    open(repo.workdir().unwrap_or(repo.path()))
}
fn open(path: &Path) -> Result<Backend, GitError> {
    Backend::open_with_policy(path, kagi_git::backend::ExecutionPolicy::human(false))
}
fn run(
    repo: &Repository,
    op: Operation,
    plan: Option<&OperationPlan>,
) -> Result<OperationOutcome, GitError> {
    let mut backend = backend(repo)?;
    let fresh;
    let plan = match plan {
        Some(plan) => plan,
        None => {
            fresh = backend.plan(&op)?;
            &fresh
        }
    };
    backend.run(&op, plan)
}
fn run_at(path: &Path, op: Operation) -> Result<OperationOutcome, GitError> {
    let mut backend = open(path)?;
    let plan = backend.plan(&op)?;
    backend.run(&op, &plan)
}
// Backend opens its own Repository; assertions retaining the fixture handle
// must re-read that handle's cached index after external mutations (also on Err).
fn refresh_fixture_index<T>(repo: &Repository, result: Result<T, GitError>) -> Result<T, GitError> {
    if let Ok(mut index) = repo.index() {
        let _ = index.read(true);
    }
    result
}

pub fn execute_create_branch(repo: &Repository, name: &str, at: &CommitId) -> Result<(), GitError> {
    let result = run(
        repo,
        Operation::CreateBranch {
            name: name.into(),
            at: at.clone(),
        },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::Unit => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_rename_branch(
    repo: &Repository,
    plan: &OperationPlan,
    old_name: &str,
    new_name: &str,
) -> Result<(), GitError> {
    let result = run(
        repo,
        Operation::RenameBranch {
            old_name: old_name.into(),
            new_name: new_name.into(),
        },
        Some(plan),
    )
    .map(|outcome| match outcome {
        OperationOutcome::Unit => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_delete_branch(
    repo: &Repository,
    plan: &OperationPlan,
    name: &str,
) -> Result<(), GitError> {
    let result = run(
        repo,
        Operation::DeleteBranch { name: name.into() },
        Some(plan),
    )
    .map(|outcome| match outcome {
        OperationOutcome::DeleteBranch { .. } => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_stash_push(
    repo: &mut Repository,
    message: Option<&str>,
    include_untracked: bool,
) -> Result<(), GitError> {
    let result = run(
        repo,
        Operation::StashPush {
            message: message.map(str::to_owned),
            include_untracked,
        },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::StashPush { .. } => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_stash_apply(repo: &mut Repository, index: usize) -> Result<(), GitError> {
    let result = run(repo, Operation::StashApply { index }, None).map(|outcome| match outcome {
        OperationOutcome::Unit => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_stash_pop(repo: &mut Repository, index: usize) -> Result<StashPopOutcome, GitError> {
    let result = run(repo, Operation::StashPop { index }, None).map(|outcome| match outcome {
        OperationOutcome::StashPop(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_stash_drop(repo: &mut Repository, index: usize) -> Result<String, GitError> {
    let result = run(repo, Operation::StashDrop { index }, None).map(|outcome| match outcome {
        OperationOutcome::StashDrop { oid: value } => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_lock_worktree(
    repo: &Repository,
    plan: &OperationPlan,
    name: &str,
    reason: Option<&str>,
) -> Result<(), GitError> {
    let result = backend(repo)?.execute_lock_worktree(plan, name, reason);
    refresh_fixture_index(repo, result)
}

pub fn execute_prune_worktrees(repo: &Repository, plan: &OperationPlan) -> Result<usize, GitError> {
    let result = backend(repo)?.execute_prune_worktrees(plan);
    refresh_fixture_index(repo, result)
}

pub fn execute_repair_worktrees(repo: &Repository, plan: &OperationPlan) -> Result<(), GitError> {
    let result = backend(repo)?.execute_repair_worktrees(plan);
    refresh_fixture_index(repo, result)
}

pub fn execute_checkout_tracking_branch(
    repo: &Repository,
    remote_branch: &str,
    local_branch: &str,
) -> Result<(), GitError> {
    let result = run(
        repo,
        Operation::CheckoutTrackingBranch {
            remote_branch: remote_branch.into(),
            local_branch: local_branch.into(),
        },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::Unit => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_switch_to_latest(
    repo: &Repository,
    repo_path: &Path,
    plan: &OperationPlan,
    branch_name: &str,
    remote_branch: &str,
) -> Result<(), GitError> {
    let result = run(
        repo,
        Operation::SwitchToLatestBranch {
            branch_name: branch_name.into(),
            remote_branch: remote_branch.into(),
        },
        Some(plan),
    )
    .map(|outcome| match outcome {
        OperationOutcome::Unit => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_reset_current_to_head(repo: &Repository, target: &CommitId) -> Result<(), GitError> {
    let result = run(
        repo,
        Operation::ResetCurrentToHead {
            target: target.clone(),
        },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::Unit => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_delete_merged_branches(
    repo: &Repository,
    repo_path: &Path,
    plan: &OperationPlan,
    targets: &[CleanupDeleteTarget],
) -> Result<CleanupOutcome, GitError> {
    let result = backend(repo)?
        .execute_delete_merged_branches(plan, targets)
        .result;
    refresh_fixture_index(repo, result)
}

pub fn execute_pull(repo: &Repository, repo_path: &Path) -> Result<PullOutcome, GitError> {
    let result = run(repo, Operation::Pull, None).map(|outcome| match outcome {
        OperationOutcome::Pull(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_pull_branch_ff(
    repo: &Repository,
    repo_path: &Path,
    plan: &OperationPlan,
    branch_name: &str,
) -> Result<PullOutcome, GitError> {
    let result = run(
        repo,
        Operation::PullBranchFf {
            branch_name: branch_name.into(),
        },
        Some(plan),
    )
    .map(|outcome| match outcome {
        OperationOutcome::Pull(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_apply_suggestion(
    repo: &Repository,
    plan: &OperationPlan,
    s: &Suggestion,
    expected: &[String],
) -> Result<SuggestionOutcome, GitError> {
    let result = run(
        repo,
        Operation::ApplySuggestion {
            suggestion: s.clone(),
            expected_original: expected.to_vec(),
        },
        Some(plan),
    )
    .map(|outcome| match outcome {
        OperationOutcome::Suggestion(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_rebase_current_onto(
    repo: &Repository,
    repo_path: &Path,
    onto: &str,
) -> Result<RebaseOutcome, GitError> {
    let result = run(
        repo,
        Operation::RebaseCurrentOnto { onto: onto.into() },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::Rebase(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn fetch_remote(repo: &Repository, repo_path: &Path) -> Result<FetchOutcome, GitError> {
    let result = backend(repo)?.fetch_remote();
    refresh_fixture_index(repo, result)
}

pub fn fetch_remote_branch(
    repo: &Repository,
    repo_path: &Path,
    remote: &str,
    branch: &str,
) -> Result<FetchOutcome, GitError> {
    let result = backend(repo)?.fetch_remote_branch(&format!("{remote}/{branch}"));
    refresh_fixture_index(repo, result)
}

pub fn execute_merge_branch(repo: &Repository, target: &str) -> Result<CommitId, GitError> {
    let result = run(
        repo,
        Operation::MergeBranch {
            target: target.into(),
        },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::Commit(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_merge_into_conflict(
    repo: &Repository,
    target: &str,
) -> Result<Vec<String>, GitError> {
    let result = run(
        repo,
        Operation::MergeIntoConflict {
            target: target.into(),
        },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::MergeIntoConflict(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_push(repo: &Repository, repo_path: &Path) -> Result<PushOutcome, GitError> {
    let result = run(repo, Operation::Push, None).map(|outcome| match outcome {
        OperationOutcome::Push(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_push_branch(
    repo: &Repository,
    repo_path: &Path,
    plan: &OperationPlan,
    branch_name: &str,
    set_upstream: bool,
) -> Result<PushOutcome, GitError> {
    let result = run(
        repo,
        Operation::PushBranch {
            branch_name: branch_name.into(),
            set_upstream,
        },
        Some(plan),
    )
    .map(|outcome| match outcome {
        OperationOutcome::Push(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_set_upstream(
    repo: &Repository,
    plan: &OperationPlan,
    branch_name: &str,
    upstream: &str,
) -> Result<(), GitError> {
    let result = run(
        repo,
        Operation::SetUpstream {
            branch_name: branch_name.into(),
            upstream: upstream.into(),
        },
        Some(plan),
    )
    .map(|outcome| match outcome {
        OperationOutcome::Unit => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_undo_commit(repo: &Repository) -> Result<UndoOutcome, GitError> {
    let result = run(repo, Operation::UndoCommit, None).map(|outcome| match outcome {
        OperationOutcome::Undo(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_amend(
    repo: &Repository,
    mode: AmendMode,
    message: Option<&str>,
) -> Result<AmendOutcome, GitError> {
    let result = run(
        repo,
        Operation::Amend {
            mode,
            message: message.map(str::to_owned),
        },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::Amend(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_create_tag(repo: &Repository, name: &str, at: &CommitId) -> Result<(), GitError> {
    let result = run(
        repo,
        Operation::CreateTag {
            name: name.into(),
            at: at.clone(),
        },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::Unit => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_push_tag(repo_path: &Path, remote: &str, name: &str) -> Result<(), GitError> {
    run_at(
        repo_path,
        Operation::PushTag {
            remote: remote.into(),
            name: name.into(),
        },
    )
    .map(|outcome| match outcome {
        OperationOutcome::Unit => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    })
}

pub fn create_snapshot(repo: &Repository, message: &str) -> Result<SnapshotEntry, GitError> {
    let result = backend(repo)?.create_snapshot(message);
    refresh_fixture_index(repo, result)
}

pub fn prune_snapshots(repo: &Repository, cap: usize) -> Result<Vec<String>, GitError> {
    let result = backend(repo)?.prune_snapshots(cap);
    refresh_fixture_index(repo, result)
}

pub fn delete_snapshot(repo: &Repository, id: &str) -> Result<(), GitError> {
    let result = backend(repo)?.delete_snapshot(id);
    refresh_fixture_index(repo, result)
}

pub fn execute_restore_snapshot(repo: &Repository, id: &str) -> Result<String, GitError> {
    let result =
        run(repo, Operation::RestoreSnapshot { id: id.into() }, None).map(
            |outcome| match outcome {
                OperationOutcome::RestoreSnapshot { savepoint: value } => value,
                other => panic!("unexpected fixture outcome: {other:?}"),
            },
        );
    refresh_fixture_index(repo, result)
}

pub fn execute_checkout(repo: &Repository, branch: &str) -> Result<(), GitError> {
    let result = run(
        repo,
        Operation::Checkout {
            branch: branch.into(),
        },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::Unit => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_checkout_commit(repo: &Repository, id: &CommitId) -> Result<(), GitError> {
    let result =
        run(repo, Operation::CheckoutCommit { id: id.clone() }, None).map(
            |outcome| match outcome {
                OperationOutcome::Unit => (),
                other => panic!("unexpected fixture outcome: {other:?}"),
            },
        );
    refresh_fixture_index(repo, result)
}

pub fn execute_force_with_lease_push(
    repo: &Repository,
    repo_path: &Path,
    plan: &OperationPlan,
) -> Result<(), GitError> {
    let result =
        run(repo, Operation::ForceWithLeasePush, Some(plan)).map(|outcome| match outcome {
            OperationOutcome::Unit => (),
            other => panic!("unexpected fixture outcome: {other:?}"),
        });
    refresh_fixture_index(repo, result)
}

pub fn execute_create_worktree(
    repo: &Repository,
    branch: &str,
    path: impl AsRef<Path>,
    start: &CommitId,
) -> Result<(), GitError> {
    let result = run(
        repo,
        Operation::CreateWorktree {
            branch: branch.into(),
            path: path.as_ref().to_string_lossy().into_owned(),
            start: start.clone(),
        },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::Unit => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_open_worktree_for_branch(
    repo: &Repository,
    branch: &str,
    path: impl AsRef<Path>,
) -> Result<(), GitError> {
    let result = run(
        repo,
        Operation::OpenWorktreeForBranch {
            branch: branch.into(),
            path: path.as_ref().to_string_lossy().into_owned(),
        },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::Unit => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_unlock_worktree(
    repo: &Repository,
    plan: &OperationPlan,
    name: &str,
) -> Result<(), GitError> {
    let result = backend(repo)?.execute_unlock_worktree(plan, name);
    refresh_fixture_index(repo, result)
}

pub fn execute_merge_into_branch(
    repo: &Repository,
    source: &str,
    target: &str,
) -> Result<CommitId, GitError> {
    let result = run(
        repo,
        Operation::MergeIntoBranch {
            source: source.into(),
            target: target.into(),
        },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::Commit(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_discard(
    repo: &Repository,
    plan: &OperationPlan,
    paths: &[String],
) -> Result<DiscardOutcome, GitError> {
    let result = run(
        repo,
        Operation::Discard {
            paths: paths.to_vec(),
        },
        Some(plan),
    )
    .map(|outcome| match outcome {
        OperationOutcome::Discard(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_cherry_pick(repo: &Repository, id: &CommitId) -> Result<CommitId, GitError> {
    let result =
        run(repo, Operation::CherryPick { id: id.clone() }, None).map(|outcome| match outcome {
            OperationOutcome::Commit(value) => value,
            other => panic!("unexpected fixture outcome: {other:?}"),
        });
    refresh_fixture_index(repo, result)
}

pub fn execute_revert(repo: &Repository, id: &CommitId) -> Result<CommitId, GitError> {
    let result =
        run(repo, Operation::Revert { id: id.clone() }, None).map(|outcome| match outcome {
            OperationOutcome::Commit(value) => value,
            other => panic!("unexpected fixture outcome: {other:?}"),
        });
    refresh_fixture_index(repo, result)
}

pub fn execute_delete_remote_branch(repo_path: &Path, remote_branch: &str) -> Result<(), GitError> {
    run_at(
        repo_path,
        Operation::DeleteRemoteBranch {
            remote_branch: remote_branch.into(),
        },
    )
    .map(|outcome| match outcome {
        OperationOutcome::Unit => (),
        other => panic!("unexpected fixture outcome: {other:?}"),
    })
}

pub fn execute_absorb(repo: &Repository, plan: &AbsorbPlan) -> Result<AbsorbOutcome, GitError> {
    let result = backend(repo)?.execute_absorb(plan);
    refresh_fixture_index(repo, result)
}

pub fn execute_conflict_continue(
    repo: &Repository,
    repo_path: &Path,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
) -> Result<ContinueResult, GitError> {
    let result = backend(repo)?.execute_conflict_continue(session, buffer);
    refresh_fixture_index(repo, result)
}

pub fn stage_conflict_resolution(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
) -> Result<(), GitError> {
    let result = backend(repo)?.stage_conflict_resolution(session, buffer);
    refresh_fixture_index(repo, result)
}

pub fn execute_conflict_save(
    repo: &Repository,
    buffer: &ResolutionBuffer,
    path: &Path,
) -> Result<SaveOutcome, GitError> {
    let result = backend(repo)?.execute_conflict_save(buffer, path);
    refresh_fixture_index(repo, result)
}

pub fn execute_merge_commit(repo: &Repository, message: &str) -> Result<CommitId, GitError> {
    let result = run(
        repo,
        Operation::MergeCommit {
            message: message.into(),
        },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::Commit(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn execute_conflict_abort(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
) -> Result<AbortOutcome, GitError> {
    let result = backend(repo)?.execute_conflict_abort(session, buffer);
    refresh_fixture_index(repo, result)
}

pub fn execute_stash_conflict_abort(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
) -> Result<AbortOutcome, GitError> {
    let result = backend(repo)?.execute_stash_conflict_abort(session, buffer);
    refresh_fixture_index(repo, result)
}

pub fn execute_conflict_skip(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
) -> Result<SkipOutcome, GitError> {
    let result = backend(repo)?.execute_conflict_skip(session, buffer);
    refresh_fixture_index(repo, result)
}

pub fn stage_file(repo: &Repository, path: &Path) -> Result<(), GitError> {
    let result = backend(repo)?.stage_file(path);
    refresh_fixture_index(repo, result)
}

pub fn unstage_file(repo: &Repository, path: &Path) -> Result<(), GitError> {
    let result = backend(repo)?.unstage_file(path);
    refresh_fixture_index(repo, result)
}

pub fn execute_commit(repo: &Repository, message: &str) -> Result<CommitId, GitError> {
    let result = run(
        repo,
        Operation::Commit {
            message: message.into(),
        },
        None,
    )
    .map(|outcome| match outcome {
        OperationOutcome::Commit(value) => value,
        other => panic!("unexpected fixture outcome: {other:?}"),
    });
    refresh_fixture_index(repo, result)
}

pub fn stage_files(repo: &Repository, paths: &[std::path::PathBuf]) -> Result<usize, GitError> {
    let result = backend(repo)?.stage_files(paths);
    refresh_fixture_index(repo, result)
}

pub fn unstage_files(repo: &Repository, paths: &[std::path::PathBuf]) -> Result<usize, GitError> {
    let result = backend(repo)?.unstage_files(paths);
    refresh_fixture_index(repo, result)
}

#[path = "remove.rs"]
mod remove;
pub use remove::{execute_remove_worktree, plan_remove_worktree};
pub fn execute_dir_file_resolution(
    repo: &Repository,
    repo_path: &Path,
    plan: &DirFilePlan,
) -> Result<(), GitError> {
    let result = backend(repo)?.execute_planned_dir_file_resolution(plan);
    refresh_fixture_index(repo, result)
}
