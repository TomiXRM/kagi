//! Checkout, create-branch, create-worktree, stash-push, stash-apply, stash-pop, cherry-pick, pull, push, undo-commit, and delete-branch operation pipelines — T013〜T016, T-HT-003/004/007/009, W2-DELETE
//!
//! Implements the **plan → preflight → execute** pipeline for:
//! - `checkout` (ADR-0004, Guarded class): `plan_checkout` / `preflight_check` / `execute_checkout`
//! - `checkout commit` (ADR-0022): `plan_checkout_commit` / `preflight_check` / `execute_checkout_commit`
//! - `create-branch` (ADR-0004, Safe class): `plan_create_branch` / `execute_create_branch`
//! - `create-worktree` (ADR-0025, Safe-create class): `plan_create_worktree` / `execute_create_worktree`
//! - `stash-push` (ADR-0004, Guarded class): `plan_stash_push` / `execute_stash_push`
//! - `stash-apply` (ADR-0004, Guarded class): `plan_stash_apply` / `execute_stash_apply`
//! - `stash-pop` (ADR-0009, Destructive-緩和): `plan_stash_pop` / `execute_stash_pop`
//! - `cherry-pick` (ADR-0004/0005, Guarded class): `plan_cherry_pick` / `execute_cherry_pick`
//! - `pull` (ADR-0004/0005/0009, Guarded class): `plan_pull` / `execute_pull`
//! - `delete-branch` (ADR-0014, Safe-class + merged-only guard): `plan_delete_branch` / `execute_delete_branch`
//!
//! The checkout operation is **always safe-mode only**: `CheckoutBuilder::safe()` is the only
//! strategy used.  Force-checkout and any reset/clean APIs are intentionally absent.
//!
//! The create-branch operation uses `repo.branch(name, &commit, false)` — force=false is a
//! **literal constant** and must never be changed.
//!
//! The stash-apply operation uses `repo.stash_apply(index, None)` **only**.
//! The stash-pop operation uses `repo.stash_apply(index, None)` then, **only on success**,
//! calls the private `stash_drop_internal` helper.  `repo.stash_drop` is **never** called
//! directly from public API.
//!
//! The cherry-pick operation uses `repo.cherrypick_commit(&commit, &head_commit, 0, None)`
//! **exclusively** for both plan and execute — the working-tree variant `repo.cherrypick()` is
//! **never used**.  This keeps the repo state clean (no CHERRYPICK state, no abort needed).
//!
//! The delete-branch operation uses `Branch::delete()` — a ref-only deletion that does NOT
//! touch the working tree.  **Force delete is intentionally absent.**  Only branches whose
//! tip commit is reachable from HEAD (merged) may be deleted; unmerged branches are a blocker.
//!
//! # Public API
//!
//! - [`plan_checkout`]          — generate an [`OperationPlan`] for checkout
//! - [`plan_checkout_commit`]   — generate an [`OperationPlan`] for detached commit checkout
//! - [`preflight_check`]        — verify HEAD has not moved since planning
//! - [`execute_checkout`]       — perform the checkout (safe-mode only)
//! - [`execute_checkout_commit`] — detach HEAD at a commit (safe-mode only)
//! - [`plan_create_branch`]     — generate an [`OperationPlan`] for branch creation
//! - [`execute_create_branch`]  — create the branch (force=false, no checkout)
//! - [`plan_create_worktree`]   — generate an [`OperationPlan`] for worktree + branch creation
//! - [`execute_create_worktree`] — create the branch and linked worktree
//! - [`plan_stash_push`]        — generate an [`OperationPlan`] for stash push
//! - [`execute_stash_push`]     — stash local modifications
//! - [`plan_stash_apply`]       — generate an [`OperationPlan`] for stash apply
//! - [`execute_stash_apply`]    — apply a stash entry (apply only, no pop/drop)
//! - [`plan_stash_pop`]         — generate an [`OperationPlan`] for stash pop (ADR-0009)
//! - [`execute_stash_pop`]      — apply then drop only on a conflict-free apply (issue #280)
//! - [`preflight_check_stash`]  — verify HEAD + stash count unchanged since planning
//! - [`plan_cherry_pick`]       — generate an [`OperationPlan`] for cherry-pick (in-memory, no WT touch)
//! - [`execute_cherry_pick`]    — apply a cherry-pick commit (in-memory → commit → checkout_head safe)
//! - [`plan_pull`]              — generate an [`OperationPlan`] for pull (fetch + merge/fast-forward)
//! - [`execute_pull`]           — run fetch(CLI) then merge/FF (in-memory, no MERGING state)
//! - [`plan_delete_branch`]     — generate an [`OperationPlan`] for branch deletion (merged only)
//! - [`execute_delete_branch`]  — delete the branch ref (no working-tree changes, no force)
//!
//! # Environment variables (test / headless use only)
//!
//! | Variable            | Effect |
//! |---------------------|--------|
//! | `KAGI_PLAN_CHECKOUT=<branch>`  | generate a plan for `<branch>` and emit a plan log |
//! | `KAGI_CHECKOUT_COMMIT=<row>`    | generate a detached checkout plan for commit row and emit a plan log |
//! | `KAGI_CREATE_BRANCH=<name>`    | generate a create-branch plan for HEAD and emit a plan log |
//! | `KAGI_PLAN_WORKTREE=<name>:<path>` | generate a create-worktree plan for HEAD and emit a plan log |
//! | `KAGI_STASH_PUSH=1`            | generate a stash-push plan and emit a plan log |
//! | `KAGI_STASH_APPLY=<index>`     | generate a stash-apply plan for `<index>` and emit a plan log |
//! | `KAGI_CHERRY_PICK=<sha>`       | generate a cherry-pick plan for `<sha>` and emit a plan log |
//! | `KAGI_PULL=1`                  | generate a pull plan and emit a plan log |
//! | `KAGI_DELETE_BRANCH=<name>`    | generate a delete-branch plan for `<name>` and emit a plan log |
//! | `KAGI_AUTO_CONFIRM=1`          | **(TEST-ONLY)** if there are no blockers, proceed directly to execute after planning. **Never set this in normal use.** |

pub(crate) use std::path::{Path, PathBuf};

pub(crate) use git2::{BranchType, Repository, StashFlags, WorktreeAddOptions};
pub(crate) use kagi_domain::head::Head;

pub(crate) use super::cli::{check_operand, is_flag_like, run_git};
pub(crate) use super::log::CommitId;
pub(crate) use super::resolve_head;
pub(crate) use super::status::{working_tree_status, ChangeKind, FileStatus};
pub(crate) use super::GitError;

pub use kagi_domain::plan::{
    AmendMode, AmendOutcome, BranchNameError, BranchRenameValidation, DiscardBackup,
    DiscardOutcome, FetchOutcome, MergeKind, OperationPlan, PullOutcome, PushOutcome,
    RebaseOutcome, StashPopOutcome, StateSummary, SuggestionOutcome, UndoOutcome,
    WorktreePathError, WorktreeValidationError,
};
// ADR-0129: structured plan text. `OperationPlan` itself moved to kagi-domain
// (shim re-export above); these are the note/title/recovery/disposition types
// producers and consumers share.
pub use kagi_domain::plan_note::{
    ChecklistNote, CommonNote, DiscardNote, NoOpKind, PlanDisposition, PlanNote, PlanRecovery,
    PlanTitle, RecoveryKind,
};

// ────────────────────────────────────────────────────────────
// Per-operation submodules (issue #13 Phase 3 physical split)
// ────────────────────────────────────────────────────────────

mod absorb;
mod branch;
mod branch_cleanup;
mod checkout;
mod cherry_revert;
mod dir_file_conflict;
mod discard;
mod fetch;
mod force_lease;
mod history;
mod merge;
mod merge_into;
mod pr_conflict;
mod pull;
mod push;
mod rebase;
mod remote_branch;
mod remote_common;
mod reset;
mod snapshot;
mod squash_merge;
mod stash;
mod suggestion;
mod switch;
mod tag;
mod worktree;
mod worktree_lifecycle;
mod worktree_paths;
mod worktree_steps;

pub use absorb::*;
pub use branch::*;
pub use branch_cleanup::*;
pub use checkout::*;
pub use cherry_revert::*;
pub use dir_file_conflict::*;
pub use discard::*;
pub use fetch::*;
pub use force_lease::*;
pub use history::*;
pub use merge::*;
pub use merge_into::*;
pub use pr_conflict::*;
pub use pull::*;
pub use push::*;
pub use rebase::*;
pub use remote_branch::*;
pub use reset::*;
pub use snapshot::*;
pub use squash_merge::*;
pub use stash::*;
pub use suggestion::*;
pub use switch::*;
pub use tag::*;
pub use worktree::*;
pub use worktree_lifecycle::*;
pub(crate) use worktree_paths::*;
pub use worktree_steps::*;

// ────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────

/// Validate a local branch rename target without touching repository state.
///
/// This intentionally accepts only `chars()`-level string checks here, then
/// delegates full refname syntax to libgit2's reference validator. Callers pass
/// `existing_branches` from a snapshot or test fixture so this remains a pure
/// function.
pub fn validate_branch_rename(
    old_name: &str,
    new_name: &str,
    existing_branches: &[String],
) -> BranchRenameValidation {
    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        return BranchRenameValidation::Invalid(BranchNameError::Required);
    }
    if trimmed != new_name {
        return BranchRenameValidation::Invalid(BranchNameError::Whitespace);
    }
    if trimmed == old_name {
        return BranchRenameValidation::Invalid(BranchNameError::SameName);
    }
    if existing_branches.iter().any(|name| name == trimmed) {
        return BranchRenameValidation::Invalid(BranchNameError::RenameExists(trimmed.to_string()));
    }

    let full_ref = format!("refs/heads/{}", trimmed);
    if !git2::Reference::is_valid_name(&full_ref) {
        return BranchRenameValidation::Invalid(BranchNameError::RenameInvalid(
            trimmed.to_string(),
        ));
    }

    BranchRenameValidation::Valid
}

pub(crate) fn status_summary_display(status: &super::status::WorkingTreeStatus) -> String {
    let dirty_parts: Vec<String> = [
        (!status.staged.is_empty()).then(|| format!("{} staged", status.staged.len())),
        (!status.unstaged.is_empty()).then(|| format!("{} modified", status.unstaged.len())),
        (!status.untracked.is_empty()).then(|| format!("{} untracked", status.untracked.len())),
        (!status.conflicted.is_empty()).then(|| format!("{} conflicted", status.conflicted.len())),
    ]
    .into_iter()
    .flatten()
    .collect();

    if dirty_parts.is_empty() {
        "clean".to_string()
    } else {
        dirty_parts.join(", ")
    }
}

/// Build a `git2::Signature` from the repository config.
///
/// Falls back to `"kagi <kagi@local>"` if either `user.name` or `user.email`
/// is not configured.
pub(crate) fn build_signature(repo: &Repository) -> Result<git2::Signature<'static>, GitError> {
    let config = repo
        .config()
        .map_err(|e| GitError::Other(format!("failed to open config: {}", e.message())))?;

    let name = config
        .get_string("user.name")
        .unwrap_or_else(|_| "kagi".to_string());
    let email = config
        .get_string("user.email")
        .unwrap_or_else(|_| "kagi@local".to_string());

    git2::Signature::now(&name, &email)
        .map_err(|e| GitError::Other(format!("failed to create signature: {}", e.message())))
}

pub(crate) fn short_oid(oid: git2::Oid) -> String {
    oid.to_string().chars().take(8).collect()
}

pub(crate) fn conflict_paths_from_index(index: &mut git2::Index) -> Result<Vec<String>, GitError> {
    let mut conflict_files = Vec::new();
    let conflicts = index
        .conflicts()
        .map_err(|e| GitError::Other(format!("index.conflicts() failed: {}", e.message())))?;
    for conflict_result in conflicts {
        let conflict = conflict_result
            .map_err(|e| GitError::Other(format!("conflict entry error: {}", e.message())))?;
        let path_bytes: Option<Vec<u8>> = conflict
            .our
            .as_ref()
            .map(|e| e.path.clone())
            .or_else(|| conflict.their.as_ref().map(|e| e.path.clone()))
            .or_else(|| conflict.ancestor.as_ref().map(|e| e.path.clone()));
        if let Some(p) = path_bytes {
            conflict_files.push(String::from_utf8_lossy(&p).into_owned());
        }
    }
    Ok(conflict_files)
}

/// Cap on how many preview `FileStatus` rows a single plan collects. A merge of
/// 10k files must not allocate a 10k-element Vec, clone it into the modal, and
/// hand it to GPUI; the modal only ever renders the first handful anyway (issue
/// #301). `Backend::snapshot` is likewise bounded.
pub(crate) const PREVIEW_FILES_CAP: usize = 200;

/// Cap on how many conflicting file names a merge plan note lists. The note
/// still carries the true `count`; the renderer appends "and N more" (#301).
pub(crate) const MERGE_NOTE_FILE_CAP: usize = 50;

/// Map git2's repository state to the pure-domain [`InProgressOp`], or `None`
/// when the repository is [`git2::RepositoryState::Clean`] (issue #299).
pub(crate) fn in_progress_op(repo: &Repository) -> Option<kagi_domain::plan_note::InProgressOp> {
    use git2::RepositoryState as S;
    use kagi_domain::plan_note::InProgressOp as Op;
    match repo.state() {
        S::Clean => None,
        S::Merge => Some(Op::Merge),
        S::Rebase | S::RebaseInteractive | S::RebaseMerge => Some(Op::Rebase),
        S::CherryPick | S::CherryPickSequence => Some(Op::CherryPick),
        S::Revert | S::RevertSequence => Some(Op::Revert),
        S::Bisect => Some(Op::Bisect),
        _ => Some(Op::Other),
    }
}

pub(crate) fn preview_files_between_trees(
    repo: &Repository,
    old_tree: &git2::Tree<'_>,
    new_tree: &git2::Tree<'_>,
) -> Result<Vec<FileStatus>, GitError> {
    let mut diff = repo
        .diff_tree_to_tree(Some(old_tree), Some(new_tree), None)
        .map_err(|e| {
            GitError::Other(format!(
                "diff_tree_to_tree for preview failed: {}",
                e.message()
            ))
        })?;
    let mut find_opts = git2::DiffFindOptions::new();
    find_opts.renames(true);
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::Other(format!("diff find_similar failed: {}", e.message())))?;

    let mut preview_files = Vec::new();
    for delta in diff.deltas() {
        // #301: bound the collection. The modal renders only a handful; a huge
        // merge must not buffer every delta.
        if preview_files.len() >= PREVIEW_FILES_CAP {
            break;
        }
        use git2::Delta;
        let change = match delta.status() {
            Delta::Added => ChangeKind::Added,
            Delta::Deleted => ChangeKind::Deleted,
            Delta::Modified => ChangeKind::Modified,
            Delta::Renamed => {
                let from = delta
                    .old_file()
                    .path()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default();
                ChangeKind::Renamed { from }
            }
            Delta::Typechange => ChangeKind::TypeChange,
            _ => ChangeKind::Modified,
        };
        let path = delta
            .new_file()
            .path()
            .map(std::path::PathBuf::from)
            .or_else(|| delta.old_file().path().map(std::path::PathBuf::from))
            .unwrap_or_default();
        preview_files.push(FileStatus { path, change });
    }
    Ok(preview_files)
}

/// Working-tree paths where an untracked file would be overwritten by checking
/// out `tree` — i.e. `tree` carries a blob at that path whose content differs
/// from the file on disk (issue #300). This is a read-only, write-free twin of
/// the check real git runs before a merge; it mirrors libgit2's SAFE-checkout
/// "untracked file would be overwritten" rule (identical content is allowed).
///
/// git2's `CheckoutBuilder::dry_run` is unusable here: `GIT_CHECKOUT_NONE`
/// reports no conflicts at all, and `safe()` would actually write to the
/// working tree — neither is acceptable at plan time. Execution instead relies
/// on the atomic SAFE checkout itself (it writes nothing on conflict).
pub(crate) fn untracked_merge_collisions(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    untracked: &[std::path::PathBuf],
) -> Vec<String> {
    let workdir = match repo.workdir() {
        Some(w) => w,
        None => return Vec::new(),
    };
    let mut hits = Vec::new();
    for rel in untracked {
        let Ok(entry) = tree.get_path(rel) else {
            continue; // the merge does not write this path
        };
        if entry.kind() != Some(git2::ObjectType::Blob) {
            continue;
        }
        let Ok(blob) = repo.find_blob(entry.id()) else {
            continue;
        };
        // Differing content is what git refuses; identical content is fine.
        match std::fs::read(workdir.join(rel)) {
            Ok(disk) if disk == blob.content() => {}
            _ => hits.push(rel.display().to_string()),
        }
    }
    hits
}

pub(crate) fn resolve_branch_commit<'repo>(
    repo: &'repo Repository,
    name: &str,
) -> Result<git2::Commit<'repo>, GitError> {
    // #299: resolve only through named refs, never `revparse_single`. The
    // latter accepts revision expressions (`HEAD~3`, `@{-1}`, `:/subject`),
    // which must never become a merge / checkout target — real git rejects
    // them there too. `resolve_reference_from_short_name` does the same DWIM
    // over ref namespaces that a branch/tag name uses (so `v1` → `refs/tags/v1`
    // still resolves, which the worktree-open flow relies on) but does NOT
    // evaluate revision syntax — a revision expression is not a ref, so it
    // fails cleanly.
    repo.find_branch(name, BranchType::Local)
        .or_else(|_| repo.find_branch(name, BranchType::Remote))
        .and_then(|branch| branch.get().peel_to_commit())
        .or_else(|_| {
            repo.resolve_reference_from_short_name(name)
                .and_then(|r| r.peel_to_commit())
        })
        .map_err(|e| GitError::Other(format!("branch '{}' not found: {}", name, e.message())))
}

/// Typed twin of [`merge_dirty_warnings`] (ADR-0129 Phase 2): same firing
/// conditions, but returns structured [`PlanNote`]s. Per-op fan-out PRs switch
/// their callers here; the string version is deleted with `Verbatim` in
/// Phase 3.
#[allow(dead_code)] // consumed by the Phase 2 fan-out PRs (ADR-0129)
pub(crate) fn merge_dirty_warnings_notes(
    status: &super::status::WorkingTreeStatus,
    op: kagi_domain::plan_note::OpPhrase,
) -> Vec<PlanNote> {
    use kagi_domain::plan_note::{CommonNote, DirtyParts, UntrackedCtx};
    let mut warnings = Vec::new();
    if !status.staged.is_empty() || !status.unstaged.is_empty() {
        warnings.push(PlanNote::Common(CommonNote::DirtyRollbackHint {
            parts: DirtyParts {
                staged: status.staged.len(),
                modified: status.unstaged.len(),
            },
            op,
        }));
        warnings.push(PlanNote::Common(CommonNote::SuggestStashPush));
    }
    if !status.untracked.is_empty() {
        warnings.push(PlanNote::Common(CommonNote::UntrackedRemain {
            count: status.untracked.len(),
            ctx: UntrackedCtx::Untouched,
        }));
    }
    warnings
}
