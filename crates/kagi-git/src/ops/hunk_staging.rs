//! Content-bound hunk staging (#1131): plan the displayed approval, preflight
//! against the exact diff that execution will apply, then verify the index.
//! The worktree and refs are never written.

use std::cell::Cell;
use std::path::Path;

use git2::{ApplyLocation, ApplyOptions, Diff, DiffOptions, Repository};
use kagi_domain::diff::{HunkApproval, HunkRange};
use kagi_domain::plan_note::{CommonNote, PlanNote};

use crate::{GitError, Head};

fn other(what: &str, e: git2::Error) -> GitError {
    GitError::Other(format!("{what} failed: {}", e.message()))
}

fn hunk_changed(path: &Path) -> GitError {
    GitError::Blocked(Box::new(PlanNote::Common(CommonNote::HunkChanged {
        path: path.display().to_string(),
    })))
}

pub(crate) struct HunkPlan<'a> {
    path: &'a Path,
    approved: HunkApproval,
    staged: bool,
}

pub(crate) fn plan_hunk(path: &Path, approved: HunkApproval, staged: bool) -> HunkPlan<'_> {
    HunkPlan {
        path,
        approved,
        staged,
    }
}

/// Prepare and validate the same diff object that execution applies. Never
/// re-read a mutable worktree between content validation and index application.
pub(crate) fn preflight_hunk<'a>(
    repo: &'a Repository,
    plan: &HunkPlan<'_>,
) -> Result<Diff<'a>, GitError> {
    let mut opts = pathspec_options(plan.path)?;
    let mut index = repo.index().map_err(|e| other("repo.index", e))?;
    index.read(true).map_err(|e| other("index.read", e))?;
    let diff = if plan.staged {
        let tree = match crate::resolve_head(repo)? {
            Head::Unborn { .. } => None,
            _ => Some(
                repo.head()
                    .and_then(|h| h.peel_to_tree())
                    .map_err(|e| other("HEAD tree", e))?,
            ),
        };
        opts.reverse(true);
        repo.diff_tree_to_index(tree.as_ref(), Some(&index), Some(&mut opts))
            .map_err(|e| other("diff_tree_to_index", e))?
    } else {
        opts.include_untracked(true)
            .show_untracked_content(true)
            .recurse_untracked_dirs(true);
        repo.diff_index_to_workdir(Some(&index), Some(&mut opts))
            .map_err(|e| other("diff_index_to_workdir", e))?
    };
    if diff.deltas().len() != 1 {
        return Err(hunk_changed(plan.path));
    }
    let mut patch = git2::Patch::from_diff(&diff, 0)
        .map_err(|e| other("Patch::from_diff", e))?
        .ok_or_else(|| hunk_changed(plan.path))?;
    let want = if plan.staged {
        plan.approved.reversed()
    } else {
        plan.approved
    };
    let mut matches = 0;
    for hunk in 0..patch.num_hunks() {
        if crate::diff::patch_hunk_approval(&patch, hunk)? == want {
            matches += 1;
        }
    }
    if matches != 1 {
        return Err(hunk_changed(plan.path));
    }
    // Freeze the actual bytes that were validated. A workdir-backed Diff can
    // load content again during apply; a parsed patch cannot. This also turns
    // an Untracked delta into an applyable Added delta without stage_file's
    // second read of the mutable working file.
    let bytes = patch.to_buf().map_err(|e| other("patch.to_buf", e))?;
    Diff::from_buffer(&bytes).map_err(|e| other("Diff::from_buffer", e))
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
    // libgit2 removes whole-file deltas without invoking hunk_callback.
    // Preflight already checked that this file's sole patch is approved.
    let whole_file = diff
        .deltas()
        .all(|d| matches!(d.status(), git2::Delta::Added | git2::Delta::Deleted));
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
    if applied.get() != 1 && !whole_file {
        return Err(GitError::Other(
            "hunk staging verify failed: approved hunk was not applied exactly once".into(),
        ));
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

pub(crate) fn execute_hunk(
    repo: &Repository,
    plan: &HunkPlan<'_>,
    diff: &Diff<'_>,
) -> Result<(), GitError> {
    let range = if plan.staged {
        plan.approved.range.reversed()
    } else {
        plan.approved.range
    };
    apply_one(repo, diff, plan.path, range)
}
