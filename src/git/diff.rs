//! Commit diff backend — T011 / T012
//!
//! Provides:
//! - [`commit_changed_files`] — file-level diff (T011)
//! - [`commit_file_diff`] — unified diff for a single file (T012)
//!
//! # Design notes
//!
//! * **first-parent diff only** — matches `git show` semantics.  For merge
//!   commits the second+ parents are ignored; only the diff against
//!   `parents[0]` is reported.  This avoids the complexity of combined diffs
//!   and keeps the result predictable for users.
//! * **rename detection** — `find_similar` with `renames(true)` is called
//!   after the raw diff so that A+D pairs are collapsed into R entries.
//!   Without this call, renames appear as an unrelated Added + Deleted pair.
//! * **Copied and other exotic statuses** — mapped to `ChangeKind::Modified`
//!   as a safe default (noted in implementation memo).
//! * **Root commit** — compared against `None` old_tree; libgit2 treats this
//!   as an empty tree, so all files appear as Added.
//!
//! # Panics
//!
//! Never panics.  All path conversions use lossy UTF-8 or `OsString`.

use std::path::{Path, PathBuf};

use git2::{Diff, DiffFindOptions, DiffOptions, Repository};

use super::{ChangeKind, CommitId, FileStatus, GitError};

// ────────────────────────────────────────────────────────────
// Diff models (architecture.md §3)
// ────────────────────────────────────────────────────────────

/// The kind of a diff line: unchanged context, a newly-added line, or a
/// removed line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLineKind {
    /// Unchanged context line (present in both old and new).
    Context,
    /// Line that was added in the new version.
    Added,
    /// Line that was removed from the old version.
    Removed,
}

/// One line in a diff hunk.
#[derive(Debug, Clone)]
pub struct DiffLine {
    /// The role of this line (context, added, or removed).
    pub kind: DiffLineKind,
    /// The line content, including any trailing newline, as lossy UTF-8.
    pub content: String,
    /// 1-based line number in the old file, if applicable.
    pub old_lineno: Option<u32>,
    /// 1-based line number in the new file, if applicable.
    pub new_lineno: Option<u32>,
}

/// A contiguous block of changes in a file diff.
#[derive(Debug, Clone)]
pub struct Hunk {
    /// `(start, count)` range in the old file.
    pub old_range: (u32, u32),
    /// `(start, count)` range in the new file.
    pub new_range: (u32, u32),
    /// The lines belonging to this hunk (context + added + removed).
    pub lines: Vec<DiffLine>,
}

/// The complete diff for one file in a commit.
#[derive(Debug, Clone)]
pub struct FileDiff {
    /// Path in the old tree (populated for Deleted / Renamed files).
    pub old_path: Option<PathBuf>,
    /// Path in the new tree (populated for Added / Modified / Renamed files).
    pub new_path: Option<PathBuf>,
    /// The type of change that produced this diff.
    pub change: ChangeKind,
    /// The diff hunks.  Empty for binary files.
    pub hunks: Vec<Hunk>,
    /// `true` if git detected the file as binary (no text diff available).
    pub is_binary: bool,
}

// ────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────

/// Return the list of files changed in `id` relative to its first parent.
///
/// * For a **root commit** (no parents) all files are returned as
///   [`ChangeKind::Added`].
/// * For a **merge commit** only the diff against `parents[0]` is returned
///   (first-parent diff, same as `git show`).
/// * Rename detection is enabled; a file that was renamed appears as a single
///   [`ChangeKind::Renamed`] entry rather than as a separate Deleted + Added
///   pair.
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn commit_changed_files(
    repo: &Repository,
    id: &CommitId,
) -> Result<Vec<FileStatus>, GitError> {
    // 1. Resolve the commit object.
    let oid = git2::Oid::from_str(&id.0).map_err(|e| GitError::Other(e.message().to_string()))?;
    let commit = repo
        .find_commit(oid)
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    // 2. Resolve the commit's own tree.
    let new_tree = commit
        .tree()
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    // 3. Resolve the first parent's tree (None for root commits).
    let parent_tree = if commit.parent_count() > 0 {
        let parent = commit
            .parent(0)
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        let tree = parent
            .tree()
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        Some(tree)
    } else {
        // Root commit: diff against empty tree.
        None
    };

    // 4. Compute raw diff (old=parent_tree, new=commit_tree).
    //    old_tree=None is equivalent to the empty tree in libgit2.
    let mut diff: Diff<'_> = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&new_tree), None)
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    // 5. Enable rename detection so Added+Deleted pairs collapse into Renamed.
    let mut find_opts = DiffFindOptions::new();
    find_opts.renames(true);
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    // 6. Convert deltas to FileStatus entries.
    let mut result = Vec::new();
    for delta in diff.deltas() {
        use git2::Delta;
        let change = match delta.status() {
            Delta::Added => ChangeKind::Added,
            Delta::Deleted => ChangeKind::Deleted,
            Delta::Modified => ChangeKind::Modified,
            Delta::Renamed => {
                let from = delta
                    .old_file()
                    .path()
                    .map(PathBuf::from)
                    .unwrap_or_default();
                ChangeKind::Renamed { from }
            }
            Delta::Typechange => ChangeKind::TypeChange,
            // Copied and all other statuses are mapped to Modified.
            // Copied: the file is a new copy of another file; semantically
            //         "added but with history", treated as Added by some tools.
            //         We use Modified as a conservative fallback.
            _ => ChangeKind::Modified,
        };

        // For renames the canonical (new) path is in new_file().
        let path = delta
            .new_file()
            .path()
            .map(PathBuf::from)
            .or_else(|| delta.old_file().path().map(PathBuf::from))
            .unwrap_or_default();

        result.push(FileStatus { path, change });
    }

    Ok(result)
}

/// Return the unified diff for a single file changed in commit `id`.
///
/// The `path` argument should be the **new-side path** of the file (i.e. the
/// path reported by [`commit_changed_files`]).  For deleted files, the old
/// path is used as a fallback by libgit2's pathspec matching.
///
/// The implementation:
/// 1. Recomputes the first-parent tree diff with a `pathspec` filter so only
///    the requested file is included.
/// 2. Iterates over `diff.num_deltas()` and picks the delta whose
///    `new_file().path()` (or `old_file().path()` for deletions) matches
///    `path`.  This is more robust than assuming `delta[0]` when the
///    pathspec might match multiple entries (e.g. directory prefixes).
/// 3. Calls `Patch::from_diff(&diff, idx)` on the matched delta.
/// 4. Extracts hunks and lines via `Patch::hunk()` / `Patch::line_in_hunk()`.
///
/// Binary files are returned with `is_binary = true` and `hunks` empty.
/// EOF-marker lines (`=`, `>`, `<`) are folded into `Context` / `Added` /
/// `Removed` so the display is clean.
///
/// Line content is decoded as lossy UTF-8 (never panics on arbitrary bytes).
///
/// # Errors
///
/// Returns [`GitError::Other`] on any libgit2 failure.
pub fn commit_file_diff(
    repo: &Repository,
    id: &CommitId,
    path: &Path,
) -> Result<FileDiff, GitError> {
    // 1. Resolve the commit.
    let oid = git2::Oid::from_str(&id.0).map_err(|e| GitError::Other(e.message().to_string()))?;
    let commit = repo
        .find_commit(oid)
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let new_tree = commit
        .tree()
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let parent_tree = if commit.parent_count() > 0 {
        let parent = commit
            .parent(0)
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        Some(
            parent
                .tree()
                .map_err(|e| GitError::Other(e.message().to_string()))?,
        )
    } else {
        None
    };

    // 2. Build diff with a pathspec so only the target file is included.
    //    pathspec uses the new-path of the file.
    let path_str = path.to_string_lossy();
    let mut diff_opts = DiffOptions::new();
    diff_opts.pathspec(path_str.as_ref());

    let mut diff: Diff<'_> = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&new_tree), Some(&mut diff_opts))
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    // 3. Enable rename detection (keeps parity with commit_changed_files).
    let mut find_opts = DiffFindOptions::new();
    find_opts.renames(true);
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    // 4. Find the delta index whose new_file path (or old_file path for
    //    deletions) matches `path`.  Fall back to index 0 if none matches.
    // Count deltas by collecting from the iterator (git2 exposes no num_deltas()).
    let num_deltas = diff.deltas().count();
    if num_deltas == 0 {
        // No delta found — return an empty diff rather than an error.
        return Ok(FileDiff {
            old_path: None,
            new_path: Some(path.to_path_buf()),
            change: ChangeKind::Modified,
            hunks: vec![],
            is_binary: false,
        });
    }

    let delta_idx = (0..num_deltas)
        .find(|&i| {
            let delta = diff.get_delta(i).unwrap();
            let np = delta.new_file().path();
            let op = delta.old_file().path();
            np == Some(path) || op == Some(path)
        })
        .unwrap_or(0);

    let delta = diff.get_delta(delta_idx).unwrap();

    // 5. Extract metadata from the delta.
    let old_path = delta.old_file().path().map(PathBuf::from);
    let new_path = delta.new_file().path().map(PathBuf::from);

    use git2::Delta;
    let change = match delta.status() {
        Delta::Added => ChangeKind::Added,
        Delta::Deleted => ChangeKind::Deleted,
        Delta::Modified => ChangeKind::Modified,
        Delta::Renamed => {
            let from = old_path.clone().unwrap_or_default();
            ChangeKind::Renamed { from }
        }
        Delta::Typechange => ChangeKind::TypeChange,
        _ => ChangeKind::Modified,
    };

    // 6. Get the Patch for this delta.
    //    `Patch::from_diff` returns `Ok(None)` for unchanged or binary deltas.
    //    We use this as the primary binary-detection mechanism because the
    //    `GIT_DIFF_FLAG_BINARY` flag on `DiffFile` is only populated after
    //    content inspection, which `diff_tree_to_tree` does not always do.
    let patch_opt = git2::Patch::from_diff(&diff, delta_idx)
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    // Treat a None patch as binary when the delta is not Unmodified.
    // (Unmodified deltas are never returned by diff_tree_to_tree unless
    //  include_unmodified is set, so in practice None always means binary here.)
    let is_binary_from_flag = delta.new_file().is_binary() || delta.old_file().is_binary();

    let patch = match patch_opt {
        None => {
            // Binary or empty-patch: no text diff available.
            return Ok(FileDiff {
                old_path,
                new_path,
                change,
                hunks: vec![],
                is_binary: true,
            });
        }
        Some(p) => {
            // Patch exists — still check the delta-level binary flag as a
            // belt-and-suspenders guard (e.g. mixed binary/text situations).
            if is_binary_from_flag {
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

    // 7. Extract hunks and lines.
    let num_hunks = patch.num_hunks();
    let mut hunks = Vec::with_capacity(num_hunks);

    for h_idx in 0..num_hunks {
        let (diff_hunk, line_count) = patch
            .hunk(h_idx)
            .map_err(|e| GitError::Other(e.message().to_string()))?;

        let old_range = (diff_hunk.old_start(), diff_hunk.old_lines());
        let new_range = (diff_hunk.new_start(), diff_hunk.new_lines());

        let mut lines = Vec::with_capacity(line_count);

        for l_idx in 0..line_count {
            let diff_line = patch
                .line_in_hunk(h_idx, l_idx)
                .map_err(|e| GitError::Other(e.message().to_string()))?;

            // Map origin character to DiffLineKind.
            // origin() can return: ' ' context, '+' added, '-' removed,
            // '=' context-EOF, '>' add-EOF, '<' remove-EOF, 'F'/'H'/'B'.
            // EOF-marker lines are folded into their logical kind.
            let kind = match diff_line.origin() {
                '+' | '>' => DiffLineKind::Added,
                '-' | '<' => DiffLineKind::Removed,
                // ' ', '=', and all other values → Context
                _ => DiffLineKind::Context,
            };

            // Decode content as lossy UTF-8 (never panics on arbitrary bytes).
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

    Ok(FileDiff {
        old_path,
        new_path,
        change,
        hunks,
        is_binary: false,
    })
}
