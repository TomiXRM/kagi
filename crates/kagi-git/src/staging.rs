//! Staging backend — T024
//!
//! Provides:
//! - [`stage_file`]           — stage a single file (index-only, WT unchanged)
//! - [`unstage_file`]         — unstage a single file (index-only, WT unchanged)
//! - [`unstaged_file_diff`]   — diff between index and working tree for a file
//! - [`staged_file_diff`]     — diff between HEAD tree and index for a file
//! - [`plan_commit`]          — plan a commit (plan pipeline)
//! - [`execute_commit`]       — create a commit from the current index
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

use std::path::{Path, PathBuf};

use git2::{DiffOptions, Repository};

use super::{
    checklist::checklist,
    diff::{DiffLine, DiffLineKind, FileDiff, Hunk},
    log::CommitId,
    ops::{build_signature, OperationPlan, StateSummary},
    resolve_head,
    status::working_tree_status,
    status::ChangeKind,
    status::FileStatus,
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
pub fn stage_file(repo: &Repository, path: &Path) -> Result<(), GitError> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working tree".to_string()))?;

    let abs_path = workdir.join(path);
    let mut index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;

    if abs_path.exists() {
        // File exists in working tree — stage it.
        index
            .add_path(path)
            .map_err(|e| GitError::Other(format!("index.add_path failed: {}", e.message())))?;
    } else {
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
pub fn unstage_file(repo: &Repository, path: &Path) -> Result<(), GitError> {
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
            let path_str = path.to_string_lossy().to_string();
            repo.reset_default(Some(&head_obj), [path_str.as_str()])
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
    let path_str = path.to_string_lossy();
    let mut diff_opts = DiffOptions::new();
    diff_opts.pathspec(path_str.as_ref());
    diff_opts.include_untracked(true);
    diff_opts.show_untracked_content(true);
    // Recurse into untracked dirs so single-file untracked entries are shown.
    diff_opts.recurse_untracked_dirs(true);

    let diff = repo
        .diff_index_to_workdir(None, Some(&mut diff_opts))
        .map_err(|e| GitError::Other(format!("diff_index_to_workdir failed: {}", e.message())))?;

    patch_to_file_diff(&diff, path)
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
    let path_str = path.to_string_lossy();
    let mut diff_opts = DiffOptions::new();
    diff_opts.pathspec(path_str.as_ref());

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

    patch_to_file_diff(&diff, path)
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
// plan_commit
// ────────────────────────────────────────────────────────────

/// Analyse whether creating a commit is safe and return an [`OperationPlan`].
///
/// # Blocker conditions
///
/// - `message` is empty after trimming whitespace.
/// - No files are staged in the index.
/// - The repository has conflicted files.
///
/// # Warning conditions
///
/// - Unstaged or untracked changes exist that will **not** be included in
///   this commit.
///
/// # Predicted state
///
/// - HEAD branch advances by one commit.
/// - Staged files become empty (committed).
/// - Unstaged / untracked changes remain.
///
/// # Errors
///
/// Returns [`GitError::Other`] if the repository cannot be queried.
pub fn plan_commit(repo: &Repository, message: &str) -> Result<OperationPlan, GitError> {
    // ── 1. Current HEAD and status ───────────────────────────
    let head = resolve_head(repo)?;
    let status = working_tree_status(repo)?;

    // ── 2. Build current StateSummary ────────────────────────
    let head_display = head.display();

    let dirty_parts: Vec<String> = [
        (!status.staged.is_empty()).then(|| format!("{} staged", status.staged.len())),
        (!status.unstaged.is_empty()).then(|| format!("{} modified", status.unstaged.len())),
        (!status.untracked.is_empty()).then(|| format!("{} untracked", status.untracked.len())),
        (!status.conflicted.is_empty()).then(|| format!("{} conflicted", status.conflicted.len())),
    ]
    .into_iter()
    .flatten()
    .collect();

    let dirty_display = if dirty_parts.is_empty() {
        "clean".to_string()
    } else {
        dirty_parts.join(", ")
    };

    let current = StateSummary {
        head: head_display.clone(),
        dirty: dirty_display,
    };

    // ── 3. Check blockers ────────────────────────────────────
    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Empty message.
    if message.trim().is_empty() {
        blockers.push("Commit message must not be empty.".to_string());
    }

    // Nothing staged.
    if status.staged.is_empty() {
        blockers.push(
            "Nothing to commit: no files are staged. \
             Use stage_file() to stage changes before committing."
                .to_string(),
        );
    }

    // Conflict state.
    if !status.conflicted.is_empty() {
        blockers.push(format!(
            "Repository has {} conflicted file(s). \
             Resolve all conflicts before committing.",
            status.conflicted.len()
        ));
    }

    // Unstaged / untracked changes remain (warning, not blocker).
    let leftover_count = status.unstaged.len() + status.untracked.len();
    if leftover_count > 0 {
        let mut parts = Vec::new();
        if !status.unstaged.is_empty() {
            parts.push(format!("{} modified", status.unstaged.len()));
        }
        if !status.untracked.is_empty() {
            parts.push(format!("{} untracked", status.untracked.len()));
        }
        warnings.push(format!(
            "{} file(s) ({}) will NOT be included in this commit.",
            leftover_count,
            parts.join(", ")
        ));
    }

    // Staged-content checklist (ADR-0043 rules 4/5/6): conflict markers (block),
    // secret/.env (warn), large binary (warn).  Inspects index BLOBs, not WT.
    // Rules 1–3 (staged empty / message empty / repo conflicted) are handled
    // above; the checklist adds the content-level rules.
    let (mut check_blockers, mut check_warnings) = checklist(repo, &status)?;
    blockers.append(&mut check_blockers);
    warnings.append(&mut check_warnings);

    // ── 4. Predicted StateSummary ─────────────────────────────
    // After commit: staged becomes empty; unstaged/untracked remain.
    let msg_summary: String = message.trim().chars().take(72).collect();

    let remaining_parts: Vec<String> = [
        (!status.unstaged.is_empty()).then(|| format!("{} modified", status.unstaged.len())),
        (!status.untracked.is_empty()).then(|| format!("{} untracked", status.untracked.len())),
    ]
    .into_iter()
    .flatten()
    .collect();

    let predicted_dirty = if remaining_parts.is_empty() {
        "clean".to_string()
    } else {
        remaining_parts.join(", ")
    };

    let branch_name = match &head {
        Head::Attached { branch, .. } => branch.clone(),
        Head::Unborn { branch } => branch.clone(),
        Head::Detached { target } => target.get(..8).unwrap_or(target).to_string(),
    };

    let predicted = StateSummary {
        head: format!("branch: {} (+1 commit: \"{}\")", branch_name, msg_summary),
        dirty: predicted_dirty,
    };

    // ── 5. Recovery guidance ──────────────────────────────────
    let recovery = format!(
        "To amend the commit message immediately after:\n  git commit --amend\n\
         To undo the commit while keeping changes staged:\n  git revert HEAD\n\
         (Staged files: {})",
        status
            .staged
            .iter()
            .map(|f| f.path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Use staged file list as preview_files.
    let preview_files: Vec<FileStatus> = status.staged.clone();

    Ok(OperationPlan {
        title: format!("Commit: \"{}\"", msg_summary),
        current,
        predicted,
        warnings,
        blockers,
        recovery,
        head_at_plan: head,
        stash_count_at_plan: 0,
        preview_files,
        preview_commits: Vec::new(),
        destructive: false,
    })
}

// ────────────────────────────────────────────────────────────
// execute_commit
// ────────────────────────────────────────────────────────────

/// Create a commit from the current index state.
///
/// # Behaviour
///
/// 1. Reads the current index and writes it as a tree object in the ODB.
/// 2. Resolves the committer/author signature from repo config (falls back to
///    `"kagi <kagi@local>"`).
/// 3. Creates the commit:
///    - **Normal repo** (HEAD exists): one parent — the current HEAD commit.
///    - **Unborn HEAD** (initial commit): no parents.
/// 4. The working tree is **not** modified; unstaged changes remain intact.
///
/// Returns the new commit's [`CommitId`].
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn execute_commit(repo: &Repository, message: &str) -> Result<CommitId, GitError> {
    // ── 1. Write the current index as a tree ─────────────────
    let mut index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
    let tree_oid = index
        .write_tree()
        .map_err(|e| GitError::Other(format!("index.write_tree() failed: {}", e.message())))?;
    let tree = repo
        .find_tree(tree_oid)
        .map_err(|e| GitError::Other(format!("find_tree failed: {}", e.message())))?;

    // ── 2. Build signature ────────────────────────────────────
    let sig = build_signature(repo)?;

    // ── 3. Resolve parents ────────────────────────────────────
    let head = resolve_head(repo)?;

    let new_oid = match head {
        Head::Unborn { .. } => {
            // Initial commit — no parents.
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])
                .map_err(|e| GitError::Other(format!("commit (initial) failed: {}", e.message())))?
        }
        _ => {
            // Normal commit — one parent (HEAD).
            let head_ref = repo
                .head()
                .map_err(|e| GitError::Other(format!("repo.head() failed: {}", e.message())))?;
            let head_oid = head_ref
                .target()
                .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
            let head_commit = repo.find_commit(head_oid).map_err(|e| {
                GitError::Other(format!("find_commit(HEAD) failed: {}", e.message()))
            })?;

            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&head_commit])
                .map_err(|e| GitError::Other(format!("commit failed: {}", e.message())))?
        }
    };

    Ok(CommitId(new_oid.to_string()))
}

// ────────────────────────────────────────────────────────────
// Internal helpers
// ────────────────────────────────────────────────────────────

/// Convert a [`git2::Diff`] to a [`FileDiff`] for the given `path`.
///
/// This is the common Patch→FileDiff conversion logic shared between
/// `unstaged_file_diff` and `staged_file_diff`.
///
/// `is_staged` controls whether "new path not found" is treated as a fully
/// untracked file (for unstaged diff) or as an empty diff (for staged diff).
fn patch_to_file_diff(diff: &git2::Diff<'_>, path: &Path) -> Result<FileDiff, GitError> {
    let num_deltas = diff.deltas().count();

    if num_deltas == 0 {
        return Ok(FileDiff {
            old_path: None,
            new_path: Some(path.to_path_buf()),
            change: ChangeKind::Modified,
            hunks: vec![],
            is_binary: false,
        });
    }

    // Find the delta index matching `path`.
    let delta_idx = (0..num_deltas)
        .find(|&i| {
            let delta = diff.get_delta(i).unwrap();
            let np = delta.new_file().path();
            let op = delta.old_file().path();
            np == Some(path) || op == Some(path)
        })
        .unwrap_or(0);

    let delta = diff.get_delta(delta_idx).unwrap();

    let old_path = delta.old_file().path().map(PathBuf::from);
    let new_path = delta.new_file().path().map(PathBuf::from);

    use git2::Delta;
    let change = match delta.status() {
        Delta::Added | Delta::Untracked => ChangeKind::Added,
        Delta::Deleted => ChangeKind::Deleted,
        Delta::Modified => ChangeKind::Modified,
        Delta::Renamed => {
            let from = old_path.clone().unwrap_or_default();
            ChangeKind::Renamed { from }
        }
        Delta::Typechange => ChangeKind::TypeChange,
        _ => ChangeKind::Modified,
    };

    // Binary check.
    let is_binary_flag = delta.new_file().is_binary() || delta.old_file().is_binary();

    // Get Patch for this delta.
    let patch_opt = git2::Patch::from_diff(diff, delta_idx)
        .map_err(|e| GitError::Other(format!("Patch::from_diff failed: {}", e.message())))?;

    let patch = match patch_opt {
        None => {
            return Ok(FileDiff {
                old_path,
                new_path,
                change,
                hunks: vec![],
                is_binary: true,
            });
        }
        Some(p) => {
            if is_binary_flag {
                return Ok(FileDiff {
                    old_path,
                    new_path,
                    change,
                    hunks: vec![],
                    is_binary: true,
                });
            }
            p
        }
    };

    // Extract hunks and lines.
    let num_hunks = patch.num_hunks();
    let mut hunks = Vec::with_capacity(num_hunks);

    for h_idx in 0..num_hunks {
        let (diff_hunk, line_count) = patch.hunk(h_idx).map_err(|e| {
            GitError::Other(format!("patch.hunk({}) failed: {}", h_idx, e.message()))
        })?;

        let old_range = (diff_hunk.old_start(), diff_hunk.old_lines());
        let new_range = (diff_hunk.new_start(), diff_hunk.new_lines());

        let mut lines = Vec::with_capacity(line_count);

        for l_idx in 0..line_count {
            let diff_line = patch.line_in_hunk(h_idx, l_idx).map_err(|e| {
                GitError::Other(format!(
                    "patch.line_in_hunk({},{}) failed: {}",
                    h_idx,
                    l_idx,
                    e.message()
                ))
            })?;

            let kind = match diff_line.origin() {
                '+' | '>' => DiffLineKind::Added,
                '-' | '<' => DiffLineKind::Removed,
                _ => DiffLineKind::Context,
            };

            let content = String::from_utf8_lossy(diff_line.content()).into_owned();

            lines.push(DiffLine {
                kind,
                content,
                old_lineno: diff_line.old_lineno(),
                new_lineno: diff_line.new_lineno(),
            });
        }

        hunks.push(Hunk {
            old_range,
            new_range,
            lines,
        });
    }

    // Same lazy-BINARY-flag workaround as diff.rs's diff_to_file_diff (the
    // two builders are near-duplicates — consolidate when one grows again):
    // workdir/index deltas only get their BINARY flag after content
    // callbacks, so an image lands here as "0 hunks, not binary" and painted
    // an EMPTY pane. A content change with no text hunks is binary.
    let is_binary = hunks.is_empty()
        && (delta.old_file().size() > 0 || delta.new_file().size() > 0)
        && delta.old_file().id() != delta.new_file().id();

    Ok(FileDiff {
        old_path,
        new_path,
        change,
        hunks,
        is_binary,
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
pub fn stage_files(repo: &Repository, paths: &[std::path::PathBuf]) -> Result<usize, GitError> {
    if paths.is_empty() {
        return Ok(0);
    }
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working tree".to_string()))?;
    let mut index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;

    for path in paths {
        if workdir.join(path).exists() {
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
}

/// Unstage every path in `paths`.
///
/// Same semantics as [`unstage_file`] (`git reset HEAD -- <paths>`), done in
/// a single `reset_default` call when HEAD exists.  Returns the number of
/// paths processed.
pub fn unstage_files(repo: &Repository, paths: &[std::path::PathBuf]) -> Result<usize, GitError> {
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
            let path_strs: Vec<String> = paths
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            repo.reset_default(Some(&head_obj), path_strs.iter().map(|s| s.as_str()))
                .map_err(|e| GitError::Other(format!("reset_default failed: {}", e.message())))?;
        }
    }
    Ok(paths.len())
}
