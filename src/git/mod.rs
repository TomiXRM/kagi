//! Git backend — T002: repository open / T003: working tree status / T004: commit log / T005: refs + snapshot / T011: commit diff / T013: checkout ops
//!
//! This module provides the entry point for opening a local Git repository,
//! extracting basic metadata (repo name, workdir path, HEAD state), querying
//! the working tree status, retrieving the commit log, reading branch /
//! remote branch / tag / stash information as a [`RepoSnapshot`], and
//! computing the file-level diff for a single commit.
//! Network transports (https/ssh) are not used in the MVP.

pub mod backend;
mod checklist;
pub mod cli;
pub mod conflicts;
mod diff;
mod diffstat;
pub mod drafts;
mod file_history;
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
pub use checklist::{checklist, text_has_conflict_marker};
#[allow(unused_imports)]
pub use cli::{run_git, GitCliOutput};
#[allow(unused_imports)]
pub use conflicts::{
    continue_blockers, detect_conflict_session, execute_conflict_abort, execute_conflict_continue,
    execute_conflict_save, execute_conflict_skip, execute_merge_commit, plan_conflict_abort,
    plan_conflict_continue, plan_conflict_continue_route, plan_conflict_skip, side_labels,
    stage_conflict_resolution, AbortOutcome, ConflictFile, ConflictKind, ConflictOp,
    ConflictSession, ConflictStatus, ContinueBlocker, ContinueOutcome, ContinueRoute, SaveOutcome,
    SideLabel, SideLabels, SkipOutcome,
};
#[allow(unused_imports)]
pub use diff::{
    commit_changed_files, commit_file_diff, compare_commit_to_workdir,
    compare_commit_to_workdir_file_diff, compare_commits, compare_file_diff, DiffLine,
    DiffLineKind, FileDiff, Hunk,
};
#[allow(unused_imports)]
pub use diffstat::{
    bar_segments, commit_diffstat, find_stat, staged_diffstat, unstaged_diffstat, FileDiffStat,
};
#[allow(unused_imports)]
pub use drafts::{clear_draft, load_draft, save_draft, Draft};
#[allow(unused_imports)]
pub use file_history::{
    file_history, CommitSummary, FileChangeSummary, FileChangeType, FileHistory, FileHistoryEntry,
    FileHistoryEntryKind, FileHistoryRequest,
};
#[allow(unused_imports)]
pub use log::{commit_log, Commit, CommitId, Signature};
#[allow(unused_imports)]
pub use message_gen::{
    collect_staged_diff, collect_staged_files, generate_message, ollama_available,
    ollama_list_models, rule_based, GenError, GenInput, Lang, MessageBackend, Style,
};
#[allow(unused_imports)]
pub use message_template::{assemble, parse_message, TemplateFields, TYPE_CHOICES};
#[allow(unused_imports)]
pub use oplog::{append_oplog, read_oplog_tail, OpLogEntry, OpOutcome};
#[allow(unused_imports)]
pub use ops::{
    branch_checked_out_worktree_path, default_tracking_branch_name, execute_amend,
    execute_checkout, execute_checkout_commit, execute_checkout_tracking_branch,
    execute_cherry_pick, execute_create_branch, execute_create_worktree, execute_delete_branch,
    execute_discard, execute_merge_branch, execute_merge_into_conflict,
    execute_open_worktree_for_branch, execute_pull, execute_pull_branch_ff, execute_push,
    execute_push_branch, execute_redo, execute_rename_branch, execute_revert, execute_set_upstream,
    execute_stash_apply, execute_stash_drop, execute_stash_pop, execute_stash_push, execute_undo,
    execute_undo_commit, fetch_remote, plan_amend, plan_checkout, plan_checkout_commit,
    plan_checkout_tracking_branch, plan_cherry_pick, plan_create_branch,
    plan_create_branch_with_checkout, plan_create_worktree, plan_delete_branch, plan_discard,
    plan_merge_branch, plan_open_worktree_for_branch, plan_pull, plan_pull_branch_ff,
    plan_pull_remote, plan_push, plan_push_branch, plan_redo, plan_rename_branch, plan_revert,
    plan_set_upstream, plan_stash_apply, plan_stash_drop, plan_stash_drop_remote, plan_stash_pop,
    plan_stash_push, plan_undo, plan_undo_commit, preflight_check, preflight_check_stash,
    validate_branch_rename, validate_worktree_path, AmendMode, AmendOutcome,
    BranchRenameValidation, DiscardBackup, DiscardOutcome, FetchOutcome, HistoryMoveOutcome,
    MergeKind, OperationPlan, PullOutcome, PushOutcome, StateSummary, UndoOutcome,
};
#[allow(unused_imports)]
pub use refs::{Branch, RemoteBranch, Stash, Tag, UpstreamInfo, Worktree};
#[allow(unused_imports)]
pub use resolution::{
    ConflictHunk, HunkChoice, HunkModel, LineOrigin, Region, ResolutionBuffer, ResolutionChoice,
    ResolvedLine,
};
#[allow(unused_imports)]
pub use snapshot::{snapshot, RepoSnapshot};
#[allow(unused_imports)]
pub use staging::{
    commit_preview, execute_commit, plan_commit, stage_file, stage_files, staged_file_diff,
    unstage_file, unstage_files, unstaged_file_diff, CommitPreview,
};
#[allow(unused_imports)]
pub use status::{working_tree_status, ChangeKind, FileStatus, WorkingTreeStatus};
#[allow(unused_imports)]
pub use trailers::{parse_coauthors, CoAuthor};

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
    /// True when this path is a linked git worktree rather than the main
    /// working tree — used to mark worktree tabs distinctly in the UI.
    pub is_worktree: bool,
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

    Ok(RepoInfo {
        name,
        workdir,
        head,
        is_worktree: repo.is_worktree(),
    })
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
                let branch = reference.shorthand().unwrap_or("(unknown)").to_string();
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
                        r.symbolic_target().ok().flatten().and_then(|sym| {
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
