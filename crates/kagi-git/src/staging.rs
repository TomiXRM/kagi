//! Staging backend — T024
//!
//! Provides:
//! - [`stage_file`]           — stage a single file (index-only, WT unchanged)
//! - [`unstage_file`]         — unstage a single file (index-only, WT unchanged)
//! - [`unstaged_file_diff`]   — diff between index and working tree for a file
//! - [`staged_file_diff`]     — diff between HEAD tree and index for a file
//!
//! # Design notes
//!
//! * **Index-only operations** — `stage_file` and `unstage_file` only modify
//!   the git index (`.git/index`).  The working tree file content is **never**
//!   changed by either function.  Tests assert this invariant.
//!
//! * **`unstage_file` implementation** — uses `repo.reset_default(target, [path])`,
//!   which is the libgit2 equivalent of `git reset HEAD -- <path>`.  When HEAD
//!   is unborn (no commits), the path is simply removed from the index via
//!   `index.remove_path`.
//!
//! * **`stage_file` for deleted files** — if the file no longer exists in the
//!   working tree, `index.add_path` would fail; we call `index.remove_path`
//!   instead so the deletion is staged.
//!
//! * **Untracked files in `unstaged_file_diff`** — computed via
//!   `diff_index_to_workdir` with `include_untracked` + `show_untracked_content`
//!   so that a new, unstaged file appears as a single Added hunk containing all
//!   its lines.
//!
//! * **Rename handling** — MVP treats a rename as two entries (old Deleted + new
//!   Added).  Full rename detection in staging is v0.2+ scope.
//!
//! * **Conflicts** — `plan_commit` blocks on conflicted files; staging a
//!   conflicted file is not supported in MVP.

use std::path::Path;

use git2::{DiffOptions, Repository};
use kagi_domain::plan_note::{CommonNote, PlanNote};

use super::{
    diff::{patch_hunks, patch_to_file_diff, FileDiff},
    resolve_head,
    status::working_tree_status,
    status::ChangeKind,
    status::WorkingTreeStatus,
    GitError, Head,
};

// ────────────────────────────────────────────────────────────
// stage_file
// ────────────────────────────────────────────────────────────

/// Stage a single file at `path` (relative to the repository root).
///
/// This function modifies **only the git index**.  The working tree file
/// content is never changed.
///
/// # Behaviour
///
/// * If the file **exists** in the working tree: calls `index.add_path(path)`.
/// * If the file has been **deleted** from the working tree: calls
///   `index.remove_path(path)` so the deletion is staged.
///
/// After the index update, `index.write()` is called to persist the change.
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
/// Whether the index marks `path` `skip-worktree`.
///
/// Sparse-checkout sets this bit and removes the file; the bit is what Git
/// itself acts on, so reading it avoids re-implementing cone / non-cone /
/// negated pattern matching (#675).
fn is_sparse_excluded(index: &git2::Index, path: &Path) -> bool {
    index.get_path(path, 0).is_some_and(|entry| {
        git2::IndexEntryExtendedFlag::from_bits_truncate(entry.flags_extended)
            .contains(git2::IndexEntryExtendedFlag::SKIP_WORKTREE)
    })
}

/// Test the directory entry itself, not a symlink's destination. Only a
/// genuinely missing entry may be staged as a deletion; other I/O errors abort.
fn worktree_entry_present(path: &Path) -> Result<bool, GitError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(GitError::Other(format!(
            "cannot inspect working tree entry '{}': {error}",
            path.display()
        ))),
    }
}

pub(crate) fn stage_file(repo: &Repository, path: &Path) -> Result<(), GitError> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working tree".to_string()))?;

    let abs_path = workdir.join(path);
    let mut index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;

    if worktree_entry_present(&abs_path)? {
        // libgit2 stages symlinks as mode 120000 with raw read_link bytes,
        // without following the target (even when it is missing).
        index
            .add_path(path)
            .map_err(|e| GitError::Other(format!("index.add_path failed: {}", e.message())))?;
    } else {
        // "Absent from the working tree" is not the same as "deleted". A
        // sparse-checkout excluded path is absent because Git removed it, and
        // staging it here would record a deletion the user never made — and
        // could not even see, since the file is invisible to them. Git refuses
        // this same operation ("paths ... exist outside of your sparse-checkout
        // definition, so will not be updated in the index"); so do we (#675).
        if is_sparse_excluded(&index, path) {
            return Err(GitError::Blocked(Box::new(PlanNote::Common(
                CommonNote::SparseExcludedPath {
                    path: path.display().to_string(),
                },
            ))));
        }
        // File was deleted — stage the deletion.
        index
            .remove_path(path)
            .map_err(|e| GitError::Other(format!("index.remove_path failed: {}", e.message())))?;
    }

    index
        .write()
        .map_err(|e| GitError::Other(format!("index.write() failed: {}", e.message())))?;

    Ok(())
}

// ────────────────────────────────────────────────────────────
// unstage_file
// ────────────────────────────────────────────────────────────

/// Unstage a single file at `path` (relative to the repository root).
///
/// This function modifies **only the git index**.  The working tree file
/// content is never changed.
///
/// # Behaviour
///
/// * **Normal repo** (HEAD exists): calls
///   `repo.reset_default(Some(&head_object), [path])`, which is the libgit2
///   equivalent of `git reset HEAD -- <path>`.  This restores the index entry
///   for `path` to the HEAD tree content (effectively unstaging the change).
///   If `path` does not exist in HEAD (new file), the path is removed from the
///   index so it becomes untracked.
///
/// * **Unborn HEAD** (no commits yet): there is no HEAD tree to reset to, so
///   the path is simply removed from the index via `index.remove_path`.
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub(crate) fn unstage_file(repo: &Repository, path: &Path) -> Result<(), GitError> {
    let head = resolve_head(repo)?;

    match head {
        Head::Unborn { .. } => {
            // No HEAD tree — just remove from index.
            let mut index = repo
                .index()
                .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
            // remove_path returns an error if the path isn't in the index.
            // Ignore "not found" errors gracefully.
            let _ = index.remove_path(path);
            index
                .write()
                .map_err(|e| GitError::Other(format!("index.write() failed: {}", e.message())))?;
        }
        _ => {
            // HEAD exists — use reset_default to restore the index entry.
            let head_ref = repo
                .head()
                .map_err(|e| GitError::Other(format!("repo.head() failed: {}", e.message())))?;
            let head_oid = head_ref
                .target()
                .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
            let head_obj = repo.find_object(head_oid, None).map_err(|e| {
                GitError::Other(format!("find_object(HEAD) failed: {}", e.message()))
            })?;

            // reset_default(Some(&head_obj), [path]) is equivalent to
            // `git reset HEAD -- <path>`:
            // - If path exists in HEAD tree: restores index entry to HEAD content.
            // - If path does NOT exist in HEAD tree: removes it from index.
            // #293: a lossy pathspec would make reset_default a silent no-op
            // (U+FFFD matches nothing) — bail on a non-UTF-8 path instead.
            let path_str = super::path_to_pathspec(path)?;
            repo.reset_default(Some(&head_obj), [path_str])
                .map_err(|e| GitError::Other(format!("reset_default failed: {}", e.message())))?;
        }
    }

    Ok(())
}

// ────────────────────────────────────────────────────────────
// unstaged_file_diff
// ────────────────────────────────────────────────────────────

/// Return the diff between the **index** and the **working tree** for `path`.
///
/// This is the "unstaged" diff — what `git diff <path>` would show.
///
/// For untracked files (`git diff` would show nothing, but `git diff --no-index`
/// would), this function uses `include_untracked` + `show_untracked_content` so
/// the whole file appears as Added lines.
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn unstaged_file_diff(repo: &Repository, path: &Path) -> Result<FileDiff, GitError> {
    // A conflicted path first: it has no stage-0 index entry, so the normal
    // index→workdir diff emits a Conflicted delta that Patch::from_diff has no
    // content for, and the pane painted "+0 −0" with no hunks (GUI report,
    // after a conflicted stash pop). Show ours (stage 2) → worktree instead —
    // exactly what the conflicted operation injected, markers included.
    if let Some(fallback) = conflicted_file_diff(repo, path)? {
        return Ok(fallback);
    }

    let mut diff_opts = DiffOptions::new();
    diff_opts.pathspec(super::path_to_pathspec(path)?);
    // #292: treat the pathspec literally so glob metacharacters in the name
    // ([ ] * ?) and glob-prefix collisions don't match a different file.
    diff_opts.disable_pathspec_match(true);
    diff_opts.include_untracked(true);
    diff_opts.show_untracked_content(true);
    // Recurse into untracked dirs so single-file untracked entries are shown.
    diff_opts.recurse_untracked_dirs(true);

    let diff = repo
        .diff_index_to_workdir(None, Some(&mut diff_opts))
        .map_err(|e| GitError::Other(format!("diff_index_to_workdir failed: {}", e.message())))?;

    patch_to_file_diff(repo, &diff, path)
}

/// The diff shown for a path with unresolved index conflicts: ours (stage 2)
/// against the working tree. `Ok(None)` when the path is not conflicted.
fn conflicted_file_diff(repo: &Repository, path: &Path) -> Result<Option<FileDiff>, GitError> {
    let index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
    if !index.has_conflicts() {
        return Ok(None);
    }
    let conflicts = index
        .conflicts()
        .map_err(|e| GitError::Other(format!("index.conflicts failed: {}", e.message())))?;
    let entry = conflicts.flatten().find(|c| {
        [&c.our, &c.their, &c.ancestor]
            .into_iter()
            .flatten()
            .any(|e| std::str::from_utf8(&e.path).ok() == path.to_str())
    });
    let Some(entry) = entry else {
        return Ok(None);
    };

    // Ours (stage 2) is the user's side. A delete/modify conflict may lack it;
    // the ancestor is the next-best "before", and no entry at all means the
    // whole worktree file reads as added.
    let base_bytes: Vec<u8> = entry
        .our
        .as_ref()
        .or(entry.ancestor.as_ref())
        .and_then(|e| repo.find_blob(e.id).ok())
        .map(|b| b.content().to_vec())
        .unwrap_or_default();

    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working tree".to_string()))?;
    let wt_bytes = std::fs::read(workdir.join(path)).unwrap_or_default();

    let patch = git2::Patch::from_buffers(&base_bytes, Some(path), &wt_bytes, Some(path), None)
        .map_err(|e| GitError::Other(format!("Patch::from_buffers failed: {}", e.message())))?;

    Ok(Some(FileDiff {
        old_path: Some(path.to_path_buf()),
        new_path: Some(path.to_path_buf()),
        change: ChangeKind::Modified,
        hunks: patch_hunks(&patch)?,
        is_binary: false,
    }))
}

// ────────────────────────────────────────────────────────────
// staged_file_diff
// ────────────────────────────────────────────────────────────

/// Return the diff between the **HEAD tree** and the **index** for `path`.
///
/// This is the "staged" diff — what `git diff --cached <path>` would show.
///
/// For unborn HEAD (no commits), `old_tree = None` is used (equivalent to
/// diffing against an empty tree, so all staged lines appear as Added).
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn staged_file_diff(repo: &Repository, path: &Path) -> Result<FileDiff, GitError> {
    let mut diff_opts = DiffOptions::new();
    diff_opts.pathspec(super::path_to_pathspec(path)?);
    // #292: literal pathspec match (see unstaged_file_diff).
    diff_opts.disable_pathspec_match(true);

    let head = resolve_head(repo)?;

    let old_tree = match head {
        Head::Unborn { .. } => {
            // No commits — diff against empty tree.
            None
        }
        _ => {
            let head_ref = repo
                .head()
                .map_err(|e| GitError::Other(format!("repo.head() failed: {}", e.message())))?;
            let head_oid = head_ref
                .target()
                .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
            let head_commit = repo.find_commit(head_oid).map_err(|e| {
                GitError::Other(format!("find_commit(HEAD) failed: {}", e.message()))
            })?;
            let tree = head_commit
                .tree()
                .map_err(|e| GitError::Other(format!("commit.tree() failed: {}", e.message())))?;
            Some(tree)
        }
    };

    let diff = repo
        .diff_tree_to_index(old_tree.as_ref(), None, Some(&mut diff_opts))
        .map_err(|e| GitError::Other(format!("diff_tree_to_index failed: {}", e.message())))?;

    patch_to_file_diff(repo, &diff, path)
}

// ────────────────────────────────────────────────────────────
// commit_preview  (T-COMMIT-001)
// ────────────────────────────────────────────────────────────

/// A read-only summary of what the *next* commit would contain.
///
/// Built purely from the current repository status + config — no git mutation
/// happens.  Used by the Commit Panel preview header (T-COMMIT-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitPreview {
    /// Total number of staged files (== `added + modified + deleted + other`).
    pub staged_count: usize,
    /// Number of staged files that are additions (`A`).
    pub added: usize,
    /// Number of staged files that are modifications (`M`).
    pub modified: usize,
    /// Number of staged files that are deletions (`D`).
    pub deleted: usize,
    /// Number of staged files that are neither A/M/D (rename/typechange).
    pub other: usize,
    /// Target branch / ref for the commit, ready to display:
    /// - attached  → the branch name (e.g. `"main"`)
    /// - unborn    → `"<branch> (unborn)"`
    /// - detached  → `"<short-sha> (detached)"`
    pub target_branch: String,
    /// Author line `"Name <email>"` from `user.name` / `user.email`, or
    /// `"(unknown)"` when neither is configured.
    pub author: String,
}

impl CommitPreview {
    /// Human-readable A/M/D summary, e.g. `"+2 ~1 -1"`.  Empty staged → `""`.
    pub fn summary(&self) -> String {
        if self.staged_count == 0 {
            return String::new();
        }
        let mut parts: Vec<String> = Vec::new();
        if self.added > 0 {
            parts.push(format!("+{}", self.added));
        }
        if self.modified > 0 {
            parts.push(format!("~{}", self.modified));
        }
        if self.deleted > 0 {
            parts.push(format!("-{}", self.deleted));
        }
        if self.other > 0 {
            parts.push(format!("\u{00b1}{}", self.other));
        }
        parts.join(" ")
    }
}

/// Build a [`CommitPreview`] for the current repository state.
///
/// Pure read: opens no new git operation beyond status + HEAD + config reads.
/// Never panics — author defaults to `"(unknown)"` when config is missing, and
/// all HEAD states (attached / unborn / detached) are handled.
///
/// # Errors
///
/// Returns [`GitError`] only if the working-tree status or HEAD cannot be read.
pub fn commit_preview(repo: &Repository) -> Result<CommitPreview, GitError> {
    let status = working_tree_status(repo)?;
    commit_preview_from_status(repo, &status)
}

/// Like [`commit_preview`] but reuses an already-computed [`WorkingTreeStatus`],
/// avoiding a second `working_tree_status` walk. Callers that already have the
/// status (e.g. the commit panel's reload) should use this — on a repo with
/// hundreds of changes, `working_tree_status` is the expensive part.
pub fn commit_preview_from_status(
    repo: &Repository,
    status: &WorkingTreeStatus,
) -> Result<CommitPreview, GitError> {
    let head = resolve_head(repo)?;

    let mut added = 0usize;
    let mut modified = 0usize;
    let mut deleted = 0usize;
    let mut other = 0usize;
    for f in &status.staged {
        match f.change {
            ChangeKind::Added => added += 1,
            ChangeKind::Modified => modified += 1,
            ChangeKind::Deleted => deleted += 1,
            ChangeKind::Renamed { .. } | ChangeKind::TypeChange => other += 1,
        }
    }

    let target_branch = match &head {
        Head::Attached { branch, .. } => branch.clone(),
        Head::Unborn { branch } => format!("{} (unborn)", branch),
        Head::Detached { target } => {
            let short: String = target.chars().take(8).collect();
            format!("{} (detached)", short)
        }
    };

    // Author from config; "(unknown)" fallback when nothing is set (no panic).
    let author = repo
        .config()
        .ok()
        .map(|cfg| {
            let name = cfg.get_string("user.name").ok();
            let email = cfg.get_string("user.email").ok();
            match (name, email) {
                (Some(n), Some(e)) => format!("{} <{}>", n, e),
                (Some(n), None) => n,
                (None, Some(e)) => format!("<{}>", e),
                (None, None) => "(unknown)".to_string(),
            }
        })
        .unwrap_or_else(|| "(unknown)".to_string());

    Ok(CommitPreview {
        staged_count: status.staged.len(),
        added,
        modified,
        deleted,
        other,
        target_branch,
        author,
    })
}

// ────────────────────────────────────────────────────────────
// Batch stage / unstage (T-UI-002: stage all / unstage all)
// ────────────────────────────────────────────────────────────

/// Stage every path in `paths` with a **single index write**.
///
/// Same per-file semantics as [`stage_file`] (existing files are added,
/// deleted files have their removal staged), but the on-disk index is
/// written once at the end, so staging hundreds of files is fast.
/// Returns the number of paths processed.
pub(crate) fn stage_files(
    repo: &Repository,
    paths: &[std::path::PathBuf],
) -> Result<usize, GitError> {
    if paths.is_empty() {
        return Ok(0);
    }
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working tree".to_string()))?;
    let mut index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;

    let result = (|| {
        for path in paths {
            // Same rule as `stage_file`: absent from the working tree is not the
            // same as deleted. This is the path "stage everything" takes, so it is
            // the one a user in a sparse-checkout repository actually reaches
            // (#675). Refuse the whole batch rather than skipping the offending
            // paths — a partially applied stage is harder to reason about than one
            // that did not happen.
            let present = worktree_entry_present(&workdir.join(path))?;
            if !present && is_sparse_excluded(&index, path) {
                return Err(GitError::Blocked(Box::new(PlanNote::Common(
                    CommonNote::SparseExcludedPath {
                        path: path.display().to_string(),
                    },
                ))));
            }
            if present {
                index.add_path(path).map_err(|e| {
                    GitError::Other(format!(
                        "index.add_path({}) failed: {}",
                        path.display(),
                        e.message()
                    ))
                })?;
            } else {
                index.remove_path(path).map_err(|e| {
                    GitError::Other(format!(
                        "index.remove_path({}) failed: {}",
                        path.display(),
                        e.message()
                    ))
                })?;
            }
        }

        index
            .write()
            .map_err(|e| GitError::Other(format!("index.write() failed: {}", e.message())))?;
        Ok(paths.len())
    })();
    if result.is_err() {
        // Repository::index shares libgit2's cached index. Discard every
        // uncommitted batch edit so a later stage cannot accidentally write it.
        index.read(true).map_err(|error| {
            GitError::Other(format!(
                "cannot reload index after aborted stage: {}",
                error.message()
            ))
        })?;
    }
    result
}

/// Unstage every path in `paths`.
///
/// Same semantics as [`unstage_file`] (`git reset HEAD -- <paths>`), done in
/// a single `reset_default` call when HEAD exists.  Returns the number of
/// paths processed.
pub(crate) fn unstage_files(
    repo: &Repository,
    paths: &[std::path::PathBuf],
) -> Result<usize, GitError> {
    if paths.is_empty() {
        return Ok(0);
    }
    let head = resolve_head(repo)?;
    match head {
        Head::Unborn { .. } => {
            let mut index = repo
                .index()
                .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
            for path in paths {
                let _ = index.remove_path(path);
            }
            index
                .write()
                .map_err(|e| GitError::Other(format!("index.write() failed: {}", e.message())))?;
        }
        _ => {
            let head_ref = repo
                .head()
                .map_err(|e| GitError::Other(format!("repo.head() failed: {}", e.message())))?;
            let head_oid = head_ref
                .target()
                .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
            let head_obj = repo.find_object(head_oid, None).map_err(|e| {
                GitError::Other(format!("find_object(HEAD) failed: {}", e.message()))
            })?;
            // #293: same silent-no-op hazard as unstage_file — a lossy pathspec
            // matches nothing. Bail on any non-UTF-8 path rather than skipping it.
            let path_strs: Vec<&str> = paths
                .iter()
                .map(|p| super::path_to_pathspec(p))
                .collect::<Result<_, _>>()?;
            repo.reset_default(Some(&head_obj), path_strs.iter().copied())
                .map_err(|e| GitError::Other(format!("reset_default failed: {}", e.message())))?;
        }
    }
    Ok(paths.len())
}
