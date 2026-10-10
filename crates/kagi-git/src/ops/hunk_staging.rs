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
    let mut approved_hunk = None;
    for hunk in 0..patch.num_hunks() {
        if crate::diff::patch_hunk_approval(&patch, hunk)? == plan.approved {
            if approved_hunk.replace(hunk).is_some() {
                return Err(hunk_changed(plan.path));
            }
        }
    }
    let approved_hunk = approved_hunk.ok_or_else(|| hunk_changed(plan.path))?;
    if plan.staged {
        return reverse_approved_hunk(&mut patch, approved_hunk, plan.approved.range);
    }
    // Freeze the actual bytes that were validated. A workdir-backed Diff can
    // load content again during apply; a parsed patch cannot. This also turns
    // an Untracked delta into an applyable Added delta without stage_file's
    // second read of the mutable working file.
    let bytes = patch.to_buf().map_err(|e| other("patch.to_buf", e))?;
    Diff::from_buffer(&bytes).map_err(|e| other("Diff::from_buffer", e))
}

/// Reverse the validated forward hunk itself. Asking xdiff for a reversed
/// diff can pick a different LCS (e.g. swapped lines), even without drift.
fn reverse_approved_hunk(
    patch: &mut git2::Patch<'_>,
    hunk: usize,
    range: HunkRange,
) -> Result<Diff<'static>, GitError> {
    let bytes = patch.to_buf().map_err(|e| other("patch.to_buf", e))?;
    let headers = bytes
        .split_inclusive(|b| *b == b'\n')
        .take_while(|line| !line.starts_with(b"@@ "));
    let mut reversed = Vec::with_capacity(bytes.len());
    for line in headers.clone() {
        if line.starts_with(b"diff --git ") {
            reversed.extend_from_slice(line);
        } else if let Some(rest) = line.strip_prefix(b"new file mode ") {
            reversed.extend_from_slice(b"deleted file mode ");
            reversed.extend_from_slice(rest);
        } else if let Some(rest) = line.strip_prefix(b"deleted file mode ") {
            reversed.extend_from_slice(b"new file mode ");
            reversed.extend_from_slice(rest);
        } else if let Some(rest) = line.strip_prefix(b"index ") {
            let dots = rest
                .windows(2)
                .position(|s| s == b"..")
                .ok_or_else(|| GitError::Other("patch index header is invalid".into()))?;
            let end = rest
                .iter()
                .position(|b| *b == b' ' || *b == b'\n')
                .unwrap_or(rest.len());
            reversed.extend_from_slice(b"index ");
            reversed.extend_from_slice(&rest[dots + 2..end]);
            reversed.extend_from_slice(b"..");
            reversed.extend_from_slice(&rest[..dots]);
            reversed.extend_from_slice(&rest[end..]);
        }
    }
    // Preserve mode changes and quoted/raw Git paths; do not reconstruct them
    // from a lossy UI path string.
    for (from, to) in [
        (b"new mode ".as_slice(), b"old mode ".as_slice()),
        (b"old mode ".as_slice(), b"new mode ".as_slice()),
        (b"+++ ".as_slice(), b"--- ".as_slice()),
        (b"--- ".as_slice(), b"+++ ".as_slice()),
    ] {
        if let Some(rest) = headers.clone().find_map(|line| line.strip_prefix(from)) {
            reversed.extend_from_slice(to);
            let prefixes = if from == b"+++ " {
                Some((b'b', b'a'))
            } else if from == b"--- " {
                Some((b'a', b'b'))
            } else {
                None
            };
            if let Some((old, new)) = prefixes {
                let prefix = usize::from(rest.starts_with(b"\""));
                if rest.get(prefix..prefix + 2) == Some([old, b'/'].as_slice()) {
                    reversed.extend_from_slice(&rest[..prefix]);
                    reversed.push(new);
                    reversed.extend_from_slice(&rest[prefix + 1..]);
                } else {
                    reversed.extend_from_slice(rest); // /dev/null
                }
            } else {
                reversed.extend_from_slice(rest);
            }
        }
    }
    let range = range.reversed();
    reversed.extend_from_slice(
        format!(
            "@@ -{},{} +{},{} @@\n",
            range.old.0, range.old.1, range.new.0, range.new.1,
        )
        .as_bytes(),
    );
    let (_, count) = patch.hunk(hunk).map_err(|e| other("patch.hunk", e))?;
    for n in 0..count {
        let line = patch
            .line_in_hunk(hunk, n)
            .map_err(|e| other("patch.line_in_hunk", e))?;
        match line.origin() {
            '+' => reversed.push(b'-'),
            '-' => reversed.push(b'+'),
            ' ' => reversed.push(b' '),
            _ => {} // Missing-final-LF markers already include their sigil.
        }
        reversed.extend_from_slice(line.content());
    }
    Diff::from_buffer(&reversed).map_err(|e| other("Diff::from_buffer", e))
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
