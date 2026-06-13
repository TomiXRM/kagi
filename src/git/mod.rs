//! Git backend — T002: repository open / T003: working tree status / T004: commit log / T005: refs + snapshot / T011: commit diff / T013: checkout ops
//!
//! This module provides the entry point for opening a local Git repository,
//! extracting basic metadata (repo name, workdir path, HEAD state), querying
//! the working tree status, retrieving the commit log, reading branch /
//! remote branch / tag / stash information as a [`RepoSnapshot`], and
//! computing the file-level diff for a single commit.
//! Network transports (https/ssh) are not used in the MVP.

mod checklist;
pub mod backend;
pub mod cli;
pub mod conflicts;
mod diff;
mod diffstat;
pub mod drafts;
mod log;
pub mod message_gen;
pub mod message_template;
pub mod oplog;
pub mod ops;
mod refs;
pub mod resolution;
mod snapshot;
mod staging;
mod status;
mod trailers;

#[allow(unused_imports)]
pub use backend::Backend;
#[allow(unused_imports)]
pub use diff::{
    DiffLine, DiffLineKind, FileDiff, Hunk, commit_changed_files, commit_file_diff,
    compare_commit_to_workdir, compare_commit_to_workdir_file_diff, compare_commits,
    compare_file_diff,
};
#[allow(unused_imports)]
pub use diffstat::{
    FileDiffStat, bar_segments, commit_diffstat, find_stat, staged_diffstat, unstaged_diffstat,
};
#[allow(unused_imports)]
pub use message_gen::{
    GenError, GenInput, Lang, MessageBackend, Style,
    collect_staged_diff, collect_staged_files, generate_message, rule_based,
    ollama_available, ollama_list_models,
};
#[allow(unused_imports)]
pub use message_template::{TemplateFields, TYPE_CHOICES, assemble, parse_message};
#[allow(unused_imports)]
pub use oplog::{OpLogEntry, OpOutcome, append_oplog, read_oplog_tail};
#[allow(unused_imports)]
pub use drafts::{Draft, clear_draft, load_draft, save_draft};
#[allow(unused_imports)]
pub use log::{Commit, CommitId, Signature, commit_log};
#[allow(unused_imports)]
pub use cli::{GitCliOutput, run_git};
#[allow(unused_imports)]
pub use ops::{
    OperationPlan, StateSummary,
    execute_checkout, execute_checkout_commit, plan_checkout, plan_checkout_commit, preflight_check,
    plan_create_branch, execute_create_branch,
    plan_create_branch_with_checkout,
    plan_create_worktree, execute_create_worktree, validate_worktree_path,
    plan_open_worktree_for_branch, execute_open_worktree_for_branch,
    branch_checked_out_worktree_path,
    plan_stash_push, execute_stash_push,
    plan_stash_apply, execute_stash_apply,
    plan_stash_pop, execute_stash_pop,
    preflight_check_stash,
    plan_cherry_pick, execute_cherry_pick,
    plan_merge_branch, execute_merge_branch, execute_merge_into_conflict, MergeKind,
    default_tracking_branch_name,
    plan_checkout_tracking_branch, execute_checkout_tracking_branch,
    plan_revert, execute_revert,
    plan_pull, execute_pull, PullOutcome,
    plan_push, execute_push, PushOutcome,
    plan_pull_branch_ff, execute_pull_branch_ff,
    plan_push_branch, execute_push_branch,
    plan_set_upstream, execute_set_upstream,
    plan_rename_branch, execute_rename_branch, validate_branch_rename, BranchRenameValidation,
    plan_undo_commit, execute_undo_commit, UndoOutcome,
    plan_undo, plan_redo, execute_undo, execute_redo, HistoryMoveOutcome,
    plan_amend, execute_amend, AmendMode, AmendOutcome,
    plan_delete_branch, execute_delete_branch,
    plan_discard, execute_discard, DiscardBackup, DiscardOutcome,
    fetch_remote, FetchOutcome,
};
#[allow(unused_imports)]
pub use checklist::{checklist, text_has_conflict_marker};
#[allow(unused_imports)]
pub use conflicts::{
    AbortOutcome, ConflictFile, ConflictKind, ConflictOp, ConflictSession, ConflictStatus,
    ContinueBlocker, ContinueOutcome, ContinueRoute, SaveOutcome, SideLabel, SideLabels, SkipOutcome,
    continue_blockers, detect_conflict_session, side_labels,
    plan_conflict_continue, plan_conflict_continue_route, execute_conflict_continue,
    execute_conflict_save, execute_merge_commit,
    plan_conflict_abort, execute_conflict_abort,
    plan_conflict_skip, execute_conflict_skip,
};
#[allow(unused_imports)]
pub use resolution::{
    ConflictHunk, HunkChoice, HunkModel, LineOrigin, Region, ResolutionBuffer, ResolutionChoice,
    ResolvedLine,
};
#[allow(unused_imports)]
pub use refs::{Branch, RemoteBranch, Stash, Tag, UpstreamInfo, Worktree};
#[allow(unused_imports)]
pub use snapshot::{RepoSnapshot, snapshot};
#[allow(unused_imports)]
pub use staging::{
    stage_files, unstage_files,
    stage_file, unstage_file,
    unstaged_file_diff, staged_file_diff,
    plan_commit, execute_commit,
    commit_preview, CommitPreview,
};
#[allow(unused_imports)]
pub use status::{ChangeKind, FileStatus, WorkingTreeStatus, working_tree_status};
#[allow(unused_imports)]
pub use trailers::{CoAuthor, parse_coauthors};

use std::path::{Path, PathBuf};

use git2::Repository;

// ────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────

pub use kagi_domain::head::Head;
#[allow(unused_imports)]
pub use kagi_domain::history::{HistoryEntry, OperationHistory, OperationKind};

/// Basic information about an opened repository.
#[derive(Debug, Clone)]
pub struct RepoInfo {
    /// Directory name of the working tree, e.g. `"repo"`.
    pub name: String,
    /// Absolute path to the working tree root.
    pub workdir: PathBuf,
    /// Current HEAD state.
    pub head: Head,
}

/// Errors that can occur when opening a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    /// The given path does not exist on the filesystem.
    PathNotFound(String),
    /// The path exists but is not a Git repository.
    NotARepository(String),
    /// The repository is bare (has no working tree); bare repos are not
    /// supported in the MVP.
    BareRepository(String),
    /// Any other libgit2 error.
    Other(String),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::PathNotFound(p) => write!(f, "path not found: {}", p),
            GitError::NotARepository(p) => write!(f, "not a git repository: {}", p),
            GitError::BareRepository(p) => write!(f, "bare repository (no working tree): {}", p),
            GitError::Other(msg) => write!(f, "git error: {}", msg),
        }
    }
}

// ────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────

/// Open the Git repository at `path` and return basic metadata.
///
/// # Errors
///
/// | Condition | Error variant |
/// |-----------|--------------|
/// | `path` does not exist | [`GitError::PathNotFound`] |
/// | `path` exists but is not a repository | [`GitError::NotARepository`] |
/// | Repository is bare (no working tree) | [`GitError::BareRepository`] |
/// | Other libgit2 failure | [`GitError::Other`] |
pub fn open_repository(path: &Path) -> Result<RepoInfo, GitError> {
    let path_str = path.display().to_string();

    // 1. Check path existence upfront for a clear error message.
    if !path.exists() {
        return Err(GitError::PathNotFound(path_str));
    }

    // 2. Try to open as a repository.
    let repo = Repository::open(path).map_err(|e| {
        use git2::ErrorCode;
        match e.code() {
            // libgit2 returns NotFound when the path is not a repo.
            ErrorCode::NotFound => GitError::NotARepository(path_str.clone()),
            _ => GitError::Other(e.message().to_string()),
        }
    })?;

    // 3. Reject bare repositories.
    if repo.is_bare() {
        return Err(GitError::BareRepository(path_str));
    }

    // 4. Resolve working directory (non-bare repos always have one).
    let workdir = repo
        .workdir()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| path.to_path_buf());

    // 5. Derive repo name from the workdir's directory name.
    let name = workdir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| workdir.display().to_string());

    // 6. Resolve HEAD state.
    let head = resolve_head(&repo)?;

    Ok(RepoInfo { name, workdir, head })
}

// ────────────────────────────────────────────────────────────
// Internal helpers
// ────────────────────────────────────────────────────────────

/// Determine the current HEAD state of `repo`.
pub(crate) fn resolve_head(repo: &Repository) -> Result<Head, GitError> {
    match repo.head() {
        Ok(reference) => {
            if reference.is_branch() {
                // Attached HEAD: extract branch name and target SHA.
                let branch = reference
                    .shorthand()
                    .unwrap_or("(unknown)")
                    .to_string();
                let target = reference
                    .target()
                    .map(|oid| oid.to_string())
                    .unwrap_or_default();
                Ok(Head::Attached { branch, target })
            } else {
                // Detached HEAD: symbolic name is absent, use direct OID.
                let target = reference
                    .target()
                    .map(|oid| oid.to_string())
                    .unwrap_or_default();
                Ok(Head::Detached { target })
            }
        }
        Err(e) => {
            use git2::ErrorCode;
            // git2 returns UnbornBranch when the repo has no commits yet.
            if e.code() == ErrorCode::UnbornBranch {
                // Read the branch name from the symbolic HEAD reference.
                // find_reference() → Result<Reference>
                // symbolic_target() → Result<Option<&str>>
                let branch = repo
                    .find_reference("HEAD")
                    .ok()
                    .and_then(|r| {
                        // symbolic_target returns Result<Option<&str>, Error>
                        r.symbolic_target().ok().flatten()
                            .and_then(|sym| {
                                // "refs/heads/main" → "main"
                                sym.strip_prefix("refs/heads/").map(|b| b.to_owned())
                            })
                    })
                    .unwrap_or_else(|| "(unknown)".to_string());
                Ok(Head::Unborn { branch })
            } else {
                Err(GitError::Other(e.message().to_string()))
            }
        }
    }
}
