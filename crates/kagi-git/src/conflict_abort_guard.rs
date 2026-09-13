//! What the in-progress operation wrote, and what the user wrote on top
//! of it — the analysis an abort needs before it rolls anything back.
//!
//! Split out of `conflict_abort.rs` on that boundary: reconstructing the
//! operation's own output and classifying mid-conflict edits is a read,
//! with no write in it, and the abort executor is long enough already.

use std::path::Path;

use git2::Repository;

use super::conflicts::{read_head_oid, ConflictOp, ConflictSession};
use super::GitError;

/// Paths among `touched` that are NOT conflicted and that the user edited (or
/// re-created) during Conflict Mode on top of the operation's output.  These
/// are the paths the abort's `read_tree` + force-checkout would destroy, so
/// their presence blocks the abort.
///
/// Two kinds of mid-conflict edit are caught:
///
/// 1. **Unstaged** — working tree differs from the index (`diff_index_to_workdir`).
/// 2. **Staged** (#307) — the user edited a non-conflicted file *and* `git add`ed
///    it, so index == workdir and the diff in (1) sees nothing.  The staged blob
///    would be silently lost by `index.read_tree(&tree)` and the force-checkout.
///    Detect it by reconstructing the operation's *own* clean-merge output and
///    flagging any non-conflicted path whose current index blob differs from it.
///    Reconstruction (not a raw index→tree diff) is what keeps the operation's
///    legitimate clean merges — including auto-merged hunks that match neither
///    parent — from being false-flagged.
///
/// Neither kind applies to a path the operation itself conflicted on: abort
/// discards resolution progress by design. #704: `session.files` is the *live*
/// unmerged set, which is empty once the resolution has been staged (Continue,
/// or a `git add` of the resolved file) — so taking it as "the paths that were
/// conflicted" reclassified every resolved path as a clean merge the user had
/// edited, and abort refused with no way out of a still-`MERGING` repository.
/// The operation's own reconstruction is what remembers them, and it is
/// derived from `MERGE_HEAD` / the sequencer files on disk, so it survives a
/// restart the way process memory would not.
///
/// `orig_oid` / `orig_tree` are ORIG_HEAD's oid and tree (the abort target).
pub(crate) fn mid_conflict_edits(
    repo: &Repository,
    session: &ConflictSession,
    touched: &[String],
    orig_oid: git2::Oid,
    orig_tree: &git2::Tree<'_>,
) -> Result<Vec<String>, GitError> {
    let result = reconstruct_op_result(repo, session, orig_oid, orig_tree)?;
    let mut conflicted: std::collections::BTreeSet<String> = session
        .files
        .iter()
        .filter_map(|f| f.path.to_str().map(str::to_string))
        .collect();
    if let Some(result) = &result {
        for entry in result.conflicts().map_err(|e| {
            GitError::Other(format!("reconstructed conflicts failed: {}", e.message()))
        })? {
            let entry = entry.map_err(|e| {
                GitError::Other(format!("reconstructed conflict entry: {}", e.message()))
            })?;
            for side in [&entry.our, &entry.their, &entry.ancestor] {
                if let Some(path) = side
                    .as_ref()
                    .and_then(|side| std::str::from_utf8(&side.path).ok())
                {
                    conflicted.insert(path.to_string());
                }
            }
        }
    }
    let non_conflicted: Vec<&str> = touched
        .iter()
        .map(String::as_str)
        .filter(|p| !conflicted.contains(*p))
        .collect();

    let mut edited: Vec<String> = Vec::new();

    // (1) Unstaged edits: working tree differs from the index.
    if !non_conflicted.is_empty() {
        let mut opts = git2::DiffOptions::new();
        // A file the op deleted and the user re-created shows up as untracked.
        opts.include_untracked(true);
        opts.disable_pathspec_match(true);
        for p in &non_conflicted {
            opts.pathspec(*p);
        }
        let diff = repo
            .diff_index_to_workdir(None, Some(&mut opts))
            .map_err(|e| {
                GitError::Other(format!("diff index → workdir failed: {}", e.message()))
            })?;
        edited.extend(
            diff.deltas()
                .filter_map(|d| d.new_file().path().or_else(|| d.old_file().path()))
                .filter_map(|p| p.to_str().map(str::to_string)),
        );
    }

    // (2) Staged edits: current index blob differs from the operation's own
    //     reconstructed clean-merge output (#307).
    if let Some(result) = &result {
        let current = repo
            .index()
            .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
        for p in &non_conflicted {
            let path = Path::new(p);
            let cur = current.get_path(path, 0).map(|e| e.id);
            let res = result.get_path(path, 0).map(|e| e.id);
            if cur != res {
                edited.push((*p).to_string());
            }
        }
    }

    edited.sort();
    edited.dedup();
    // ponytail: the review also asked to drop paths whose index AND working
    // tree already equal `orig_tree` (their restore is a no-op). Unreachable
    // as written: `touched` is `diff(orig_tree → index) ∪ session.files`, so a
    // path whose index entry equals `orig_tree` is only in it while it is
    // still unmerged — and unmerged paths are excluded above. Add the
    // exclusion if `op_touched_paths` ever widens.
    Ok(edited)
}

/// First-parent tree of a commit, or `None` for a root commit (no parents).
fn first_parent_tree<'r>(
    repo: &'r Repository,
    oid: git2::Oid,
) -> Result<Option<git2::Tree<'r>>, GitError> {
    let commit = repo
        .find_commit(oid)
        .map_err(|e| GitError::Other(format!("commit lookup failed: {}", e.message())))?;
    match commit.parent(0) {
        Ok(parent) => Ok(Some(parent.tree().map_err(|e| {
            GitError::Other(format!("parent tree lookup failed: {}", e.message()))
        })?)),
        Err(_) => Ok(None),
    }
}

/// Commit's own tree.
fn commit_tree<'r>(repo: &'r Repository, oid: git2::Oid) -> Result<git2::Tree<'r>, GitError> {
    repo.find_commit(oid)
        .and_then(|c| c.tree())
        .map_err(|e| GitError::Other(format!("commit tree lookup failed: {}", e.message())))
}

/// Reconstruct the clean-merge output index of the in-flight operation, so the
/// staged-edit abort guard (#307) can tell an operation-produced staged entry
/// from a user's mid-conflict edit to a non-conflicted file. Extended in #369
/// to the sequencer ops (rebase / cherry-pick / revert), not just merge.
///
/// Rebase replays onto current HEAD (onto + applied commits), not ORIG_HEAD,
/// which is only the abort restoration target. Other ops retain their pre-op tree.
/// - **merge**: base = merge-base(HEAD, MERGE_HEAD), theirs = MERGE_HEAD tree.
/// - **cherry-pick / rebase** (replay commit `C`): base = `C^` tree, theirs = `C` tree.
/// - **revert** (undo commit `C`): base = `C` tree, theirs = `C^` tree.
fn reconstruct_op_result<'r>(
    repo: &'r Repository,
    session: &ConflictSession,
    orig_oid: git2::Oid,
    orig_tree: &git2::Tree<'r>,
) -> Result<Option<git2::Index>, GitError> {
    // (base_tree, theirs_tree) for the 3-way; `None` base = 2-way / empty base.
    let (base_tree, theirs_tree): (Option<git2::Tree<'r>>, git2::Tree<'r>) = match session.op {
        ConflictOp::Merge { .. } => {
            let Some(merge_oid) = read_head_oid(repo, "MERGE_HEAD") else {
                return Ok(None);
            };
            let base = match repo.merge_base(orig_oid, merge_oid) {
                Ok(b) => Some(commit_tree(repo, b)?),
                // Unrelated histories: base-less 2-way merge.
                Err(_) => None,
            };
            (base, commit_tree(repo, merge_oid)?)
        }
        // Rebase and cherry-pick both *replay* commit C onto HEAD: base = C^, theirs = C.
        ConflictOp::CherryPick { .. } | ConflictOp::Rebase { .. } => {
            let head = if matches!(session.op, ConflictOp::CherryPick { .. }) {
                "CHERRY_PICK_HEAD"
            } else {
                "REBASE_HEAD"
            };
            let Some(oid) = read_head_oid(repo, head) else {
                return Ok(None);
            };
            (first_parent_tree(repo, oid)?, commit_tree(repo, oid)?)
        }
        // Revert *undoes* commit C: base = C, theirs = C^ (the inverse patch).
        ConflictOp::Revert { .. } => {
            let Some(oid) = read_head_oid(repo, "REVERT_HEAD") else {
                return Ok(None);
            };
            let Some(parent) = first_parent_tree(repo, oid)? else {
                // Reverting a root commit has no parent tree to move toward.
                return Ok(None);
            };
            (Some(commit_tree(repo, oid)?), parent)
        }
        // StashConflict has no commit-producing output to reconstruct.
        ConflictOp::StashConflict => return Ok(None),
    };
    let rebase_ours = if matches!(session.op, ConflictOp::Rebase { .. }) {
        Some(
            repo.head()
                .and_then(|head| head.peel_to_tree())
                .map_err(|e| {
                    GitError::Other(format!("rebase HEAD tree lookup failed: {}", e.message()))
                })?,
        )
    } else {
        None
    };
    let index = repo
        .merge_trees(
            base_tree.as_ref().unwrap_or(orig_tree),
            rebase_ours.as_ref().unwrap_or(orig_tree),
            &theirs_tree,
            None,
        )
        .map_err(|e| GitError::Other(format!("merge_trees reconstruct failed: {}", e.message())))?;
    Ok(Some(index))
}

/// Every path the in-progress operation wrote into the working tree.
///
/// That is the diff between the pre-op `tree` and the operation-result index
/// (cleanly-merged modifications, additions and deletions), plus the session's
/// conflicting paths — those carry only stage 1/2/3 entries, so they can be
/// reported inconsistently by a tree↔index diff and are added explicitly.
///
/// Non-UTF-8 paths are dropped: they cannot be expressed as a libgit2
/// pathspec.  (Tracked separately as issue #293 — the whole codebase loses
/// non-UTF-8 paths today; this function does not make that worse.)
pub(crate) fn op_touched_paths(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    session: &ConflictSession,
) -> Result<Vec<String>, GitError> {
    let mut paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    let diff = repo
        .diff_tree_to_index(Some(tree), None, None)
        .map_err(|e| {
            GitError::Other(format!("diff pre-op tree → index failed: {}", e.message()))
        })?;
    for delta in diff.deltas() {
        for file in [delta.old_file(), delta.new_file()] {
            if let Some(p) = file.path().and_then(|p| p.to_str()) {
                paths.insert(p.to_string());
            }
        }
    }

    for file in &session.files {
        if let Some(p) = file.path.to_str() {
            paths.insert(p.to_string());
        }
    }

    Ok(paths.into_iter().collect())
}
