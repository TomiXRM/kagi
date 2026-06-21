//! Per-file diffstat — W16-DIFFSTAT (T-DIFFSTAT-001 / 002 / 003)
//!
//! Provides:
//! - [`FileDiffStat`] — UI-independent per-file additions/deletions model.
//! - [`commit_diffstat`] / [`staged_diffstat`] / [`unstaged_diffstat`] —
//!   aggregate additions/deletions per changed file for a commit, the staged
//!   index, or the working tree.
//! - [`bar_segments`] — pure segment-allocation function for the mini bar.
//!
//! # Design notes
//!
//! * **Per-delta aggregation via `Patch`** — additions/deletions are summed per
//!   delta using `git2::Patch::from_diff(&diff, idx)` + `Patch::line_stats()`.
//!   `Diff::stats()` only returns repo-wide totals and is deliberately **not**
//!   used (spec §計算仕様).
//!
//! * **Binary files** — a binary delta yields `is_binary = true` with
//!   `additions = deletions = 0` (no text line stats are available).
//!
//! * **Rename detection** — enabled with the same `DiffFindOptions` as
//!   `diff.rs` so an A+D pair collapses into one `Renamed` entry whose `change`
//!   carries the `from` path.
//!
//! * **Performance** — the aggregation functions compute every delta in the
//!   diff.  Callers that truncate (e.g. the Inspector's `MAX_FILES`) should run
//!   the bar computation only on the truncated set; the per-delta `Patch`
//!   generation here is bounded by the number of changed files, which the diff
//!   itself already represents.

use std::path::PathBuf;

use git2::{Diff, DiffFindOptions, DiffOptions, Repository};
use kagi_domain::status::ChangeKind;

use super::{resolve_head, CommitId, GitError, Head};

// ────────────────────────────────────────────────────────────
// Model (T-DIFFSTAT-001)
// ────────────────────────────────────────────────────────────
//
// `FileDiffStat`, `bar_segments`, and `find_stat` now live in the pure
// `kagi-domain` crate (ADR-0072). They are re-exported here so existing
// `kagi::git::*` paths keep resolving while the git2-backed diffstat
// aggregation functions below construct them.
pub use kagi_domain::diffstat::{bar_segments, find_stat, FileDiffStat};

// ────────────────────────────────────────────────────────────
// Aggregation (T-DIFFSTAT-002)
// ────────────────────────────────────────────────────────────

/// Diffstat for a commit relative to its first parent (root commit → all added).
///
/// Same delta set as `commit_changed_files`.  Rename detection is enabled.
pub fn commit_diffstat(repo: &Repository, id: &CommitId) -> Result<Vec<FileDiffStat>, GitError> {
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

    let mut diff = repo
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&new_tree), None)
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    diff_to_diffstats(&mut diff)
}

/// Diffstat for the staged changes (HEAD tree → index).
///
/// For an unborn HEAD the old tree is the empty tree, so everything is Added.
pub fn staged_diffstat(repo: &Repository) -> Result<Vec<FileDiffStat>, GitError> {
    let head = resolve_head(repo)?;
    let old_tree = match head {
        Head::Unborn { .. } => None,
        _ => {
            let head_ref = repo
                .head()
                .map_err(|e| GitError::Other(e.message().to_string()))?;
            let head_oid = head_ref
                .target()
                .ok_or_else(|| GitError::Other("HEAD has no target OID".to_string()))?;
            let head_commit = repo
                .find_commit(head_oid)
                .map_err(|e| GitError::Other(e.message().to_string()))?;
            Some(
                head_commit
                    .tree()
                    .map_err(|e| GitError::Other(e.message().to_string()))?,
            )
        }
    };

    let mut diff = repo
        .diff_tree_to_index(old_tree.as_ref(), None, None)
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    diff_to_diffstats(&mut diff)
}

/// Diffstat for the unstaged changes of **tracked** files (index → working tree).
///
/// Untracked files are intentionally **excluded**: computing a line diffstat for
/// an untracked file means reading its full content, and a bulk untracked drop
/// (e.g. 300 images) made this cost hundreds of ms on the UI thread on every
/// reload. Untracked files are shown in the commit panel as new ("A") without a
/// `+/−` bar; only tracked modifications get bars.
pub fn unstaged_diffstat(repo: &Repository) -> Result<Vec<FileDiffStat>, GitError> {
    let mut opts = DiffOptions::new();
    // No include_untracked: tracked modifications only (cheap, predictable).
    let mut diff = repo
        .diff_index_to_workdir(None, Some(&mut opts))
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    diff_to_diffstats(&mut diff)
}

// ────────────────────────────────────────────────────────────
// Internal helpers
// ────────────────────────────────────────────────────────────

/// Convert a diff into per-file [`FileDiffStat`] entries.
///
/// Enables rename detection, then for each delta computes additions/deletions
/// via `Patch::from_diff(&diff, idx).line_stats()`.  Binary deltas (None patch
/// or binary flag) become `is_binary = true` with zero counts.
fn diff_to_diffstats(diff: &mut Diff<'_>) -> Result<Vec<FileDiffStat>, GitError> {
    let mut find_opts = DiffFindOptions::new();
    find_opts.renames(true);
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let num_deltas = diff.deltas().count();
    let mut out = Vec::with_capacity(num_deltas);

    for idx in 0..num_deltas {
        let delta = diff.get_delta(idx).unwrap();
        let (path, change) = path_and_change(&delta);

        // Per-delta line stats. `Patch::from_diff` returns None for binary or
        // unchanged deltas — treat None as binary here (unchanged deltas are
        // not emitted by these diffs).
        //
        // Generating the patch is what makes libgit2 inspect file content and
        // populate the binary flag, so we re-read the binary flag from the
        // *patch's own delta* (the pre-patch `diff.get_delta` flag is not yet
        // populated for `diff_tree_to_tree` with default options).
        let patch = git2::Patch::from_diff(diff, idx)
            .map_err(|e| GitError::Other(e.message().to_string()))?;

        let (additions, deletions, is_binary) = match patch {
            None => (0, 0, true),
            Some(p) => {
                let pd = p.delta();
                if pd.new_file().is_binary() || pd.old_file().is_binary() {
                    (0, 0, true)
                } else {
                    // line_stats() → (context, additions, deletions)
                    let (_, add, del) = p
                        .line_stats()
                        .map_err(|e| GitError::Other(e.message().to_string()))?;
                    (add, del, false)
                }
            }
        };

        out.push(FileDiffStat {
            path,
            change,
            additions,
            deletions,
            is_binary,
        });
    }

    Ok(out)
}

/// Resolve `(new-side path, ChangeKind)` for a delta (rename-aware).
fn path_and_change(delta: &git2::DiffDelta<'_>) -> (PathBuf, ChangeKind) {
    use git2::Delta;
    let old_path = delta.old_file().path().map(PathBuf::from);
    let change = match delta.status() {
        Delta::Added | Delta::Untracked => ChangeKind::Added,
        Delta::Deleted => ChangeKind::Deleted,
        Delta::Modified => ChangeKind::Modified,
        Delta::Renamed => ChangeKind::Renamed {
            from: old_path.clone().unwrap_or_default(),
        },
        Delta::Typechange => ChangeKind::TypeChange,
        _ => ChangeKind::Modified,
    };
    let path = delta
        .new_file()
        .path()
        .map(PathBuf::from)
        .or(old_path)
        .unwrap_or_default();
    (path, change)
}
