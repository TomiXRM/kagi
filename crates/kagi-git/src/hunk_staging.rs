//! Hunk-level stage / unstage (#842, Refs #357).
//!
//! Like [`crate::Backend::stage_file`] / [`crate::Backend::unstage_file`],
//! these change **only the index's staged content** — the working tree is
//! never touched. A hunk is named by its header numbers ([`HunkRange`]): the
//! diff is re-read with the same options the Commit Panel diff uses, and only
//! a hunk with exactly those numbers is applied. When none has them — the
//! index or the file moved since the diff was drawn — the call is refused with
//! [`CommonNote::HunkChanged`] and nothing is written; a neighbouring hunk is
//! never staged in its place.
//!
//! A file that is added or deleted as a whole (untracked, newly staged,
//! removed) diffs as a single hunk that *is* the file, so its hunk is staged
//! or unstaged as the file.

use std::cell::Cell;
use std::path::Path;

use git2::{ApplyLocation, ApplyOptions, Diff, DiffOptions, Repository};
use kagi_domain::diff::HunkRange;
use kagi_domain::plan_note::{CommonNote, PlanNote};

use crate::diff::FileDiff;
use crate::status::ChangeKind;
use crate::{staging, GitError, Head};

fn other(what: &str, e: git2::Error) -> GitError {
    GitError::Other(format!("{what} failed: {}", e.message()))
}

fn hunk_changed(path: &Path) -> GitError {
    GitError::Blocked(Box::new(PlanNote::Common(CommonNote::HunkChanged {
        path: path.display().to_string(),
    })))
}

/// Refuse unless `shown` still has a hunk with exactly `range`.
fn require_hunk(shown: &FileDiff, path: &Path, range: HunkRange) -> Result<(), GitError> {
    if shown.hunks.iter().any(|h| h.range() == range) {
        Ok(())
    } else {
        Err(hunk_changed(path))
    }
}

fn pathspec_options(path: &Path) -> Result<DiffOptions, GitError> {
    let mut opts = DiffOptions::new();
    opts.pathspec(crate::path_to_pathspec(path)?);
    // #292: literal pathspec, as the Commit Panel diffs.
    opts.disable_pathspec_match(true);
    Ok(opts)
}

/// The staged blob of `path` at stage 0, if any.
fn index_oid(repo: &Repository, path: &Path) -> Result<Option<git2::Oid>, GitError> {
    let index = repo.index().map_err(|e| other("repo.index()", e))?;
    Ok(index.get_path(path, 0).map(|entry| entry.id))
}

/// Apply to the index only the hunk of `diff` whose header is `want`, then
/// check the index entry for `path` really moved.
fn apply_one(repo: &Repository, diff: &Diff, path: &Path, want: HunkRange) -> Result<(), GitError> {
    let before = index_oid(repo, path)?;
    let applied = Cell::new(0usize);
    let mut opts = ApplyOptions::new();
    opts.hunk_callback(|hunk| {
        let hit = hunk.is_some_and(|h| {
            HunkRange {
                old: (h.old_start(), h.old_lines()),
                new: (h.new_start(), h.new_lines()),
            } == want
        });
        if hit {
            applied.set(applied.get() + 1);
        }
        hit
    });
    repo.apply(diff, ApplyLocation::Index, Some(&mut opts))
        .map_err(|e| other("apply to index", e))?;
    if applied.get() != 1 {
        return Err(hunk_changed(path));
    }
    // Verify: the staged content of this path is not what it was.
    if index_oid(repo, path)? == before {
        return Err(GitError::Other(format!(
            "hunk staging verify failed: the index entry for '{}' did not change",
            path.display()
        )));
    }
    Ok(())
}

/// Stage the unstaged hunk of `path` whose header is `range` (the Commit
/// Panel's unstaged diff: index → working tree).
pub(crate) fn stage_hunk(repo: &Repository, path: &Path, range: HunkRange) -> Result<(), GitError> {
    let shown = staging::unstaged_file_diff(repo, path)?;
    require_hunk(&shown, path, range)?;
    if shown.change != ChangeKind::Modified {
        return staging::stage_file(repo, path);
    }
    let mut opts = pathspec_options(path)?;
    let diff = repo
        .diff_index_to_workdir(None, Some(&mut opts))
        .map_err(|e| other("diff_index_to_workdir", e))?;
    apply_one(repo, &diff, path, range)
}

/// Unstage the staged hunk of `path` whose header is `range` (the Commit
/// Panel's staged diff: HEAD → index), by applying that hunk reversed.
pub(crate) fn unstage_hunk(
    repo: &Repository,
    path: &Path,
    range: HunkRange,
) -> Result<(), GitError> {
    let shown = staging::staged_file_diff(repo, path)?;
    require_hunk(&shown, path, range)?;
    let head_tree = match crate::resolve_head(repo)? {
        Head::Unborn { .. } => None,
        _ => Some(
            repo.head()
                .and_then(|h| h.peel_to_tree())
                .map_err(|e| other("HEAD tree", e))?,
        ),
    };
    if shown.change != ChangeKind::Modified || head_tree.is_none() {
        let plan = crate::ops::unstage::plan_unstage(repo, std::iter::once(path))?;
        crate::ops::unstage::preflight_unstage(std::iter::once(path), &plan)?;
        crate::ops::unstage::execute_unstage(repo, &plan)?;
        return Ok(());
    }
    let mut opts = pathspec_options(path)?;
    opts.reverse(true);
    let diff = repo
        .diff_tree_to_index(head_tree.as_ref(), None, Some(&mut opts))
        .map_err(|e| other("diff_tree_to_index", e))?;
    apply_one(repo, &diff, path, range.reversed())
}
