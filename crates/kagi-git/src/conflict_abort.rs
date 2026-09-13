//! Abort of an in-progress merge / rebase / cherry-pick / revert / stash apply
//! — planning, the mid-conflict edit guard, and the pathspec-bounded restore.
//!
//! Split out of [`crate::conflicts`] (#704): abort is its own feature boundary
//! — it is the only conflict operation that rolls the repository *back*, and it
//! is the one the header operation strip offers whether or not a conflict
//! editor is open. Detection, terminology, continue and skip stay in
//! `conflicts.rs`; nothing here is a second detector.

use std::path::{Path, PathBuf};

use git2::Repository;

use kagi_domain::conflict_family::ConflictProgress;
use kagi_domain::plan_note::{
    ConflictsNote, ConflictsRecovery, ConflictsTitle, PlanDisposition, PlanNote, PlanRecovery,
    PlanTitle, RecoveryKind,
};

use super::conflicts::{
    current_state_summary, read_head_oid, short_sha, ConflictOp, ConflictSession,
};
use super::ops::{OperationPlan, StateSummary};
use super::resolution::ResolutionBuffer;
use super::{resolve_head, GitError};

/// Outcome of an executed conflict abort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbortOutcome {
    /// Sha HEAD was restored to (the pre-operation `ORIG_HEAD`), if known.
    pub restored_to: Option<String>,
    /// Path the resolution buffer was preserved at, if a buffer was saved.
    pub buffer_preserved_at: Option<PathBuf>,
}

/// Plan an `abort`: describe restoring the pre-operation state and preserving
/// the resolution buffer.  Always available (no blockers) per ADR-0056.
pub fn plan_conflict_abort(
    repo: &Repository,
    session: &ConflictSession,
) -> Result<OperationPlan, GitError> {
    let head = resolve_head(repo)?;
    let current = current_state_summary(repo)?;

    let orig = read_orig_head(repo);
    let predicted_head = match &orig {
        Some(sha) => format!("restored to {}", short_sha(sha)),
        None => current.head.clone(),
    };

    let warnings = vec![PlanNote::Conflicts(
        ConflictsNote::PartialResolutionsPreserved,
    )];

    let op = session.op.slug().to_string();
    let recovery = PlanRecovery {
        kind: RecoveryKind::Conflicts(ConflictsRecovery::Abort { op: op.clone() }),
        commands: Vec::new(),
    };

    Ok(OperationPlan {
        disposition: PlanDisposition::Ready,
        title: PlanTitle::Conflicts(ConflictsTitle::Abort { op }),
        current,
        predicted: StateSummary {
            head: predicted_head,
            dirty: "clean".to_string(),
        },
        warnings,
        blockers: Vec::new(),
        recovery: Some(recovery),
        head_at_plan: head,
        stash_count_at_plan: 0,
        stash_identity: None,
        worktree_digest: None,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
        destructive: false,
        equivalent_command: None,
    })
}

/// Execute an `abort`: clean the operation state, restore HEAD's working tree to
/// the pre-operation `ORIG_HEAD`, and preserve the resolution buffer.
///
/// Restoration is a `checkout_tree` of the ORIG_HEAD tree **restricted to the
/// paths the aborted operation itself wrote** (no `reset --hard`, no `clean`):
/// the index is read back to the pre-op tree, those paths are rewritten from
/// it (files the operation added are removed), then `cleanup_state` removes the
/// `MERGE_HEAD` / sequencer metadata.  Paths the operation never touched are
/// outside the pathspec and are never looked at, so unrelated local work
/// survives.  The branch ref is moved back to ORIG_HEAD so the aborted commit
/// chain is detached (recoverable via reflog).
///
/// The `buffer` is flushed to the autosave directory first so a partial
/// resolution is never lost (ADR-0057); its path is returned for the oplog
/// entry the caller writes.
pub(crate) fn execute_conflict_abort(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
) -> Result<AbortOutcome, GitError> {
    execute_conflict_abort_with_progress(repo, session, buffer, |_| {})
}

/// [`execute_conflict_abort`], reporting how far the mutation got.
///
/// ADR-0196: the family report needs the *evidence*, not only the result, and
/// each stage names what has actually happened when it fires —
/// [`ConflictProgress::IndexWritten`] after the index is written,
/// [`ConflictProgress::IndexAndWorktreeWritten`] only once the checkout has
/// returned, [`ConflictProgress::StateCleanupStarted`] when the `.git/`
/// operation state is about to go — which on the no-`ORIG_HEAD` path is the
/// only stage there is, because clearing that state is the whole mutation.
/// Anything past `NotStarted` settles as `Unknown` (reconcile) rather than as
/// a retryable `Failed`.
///
/// `Verified` is deliberately absent: verification is the Backend's
/// `verify_abort`, which runs after this returns.
pub fn execute_conflict_abort_with_progress(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
    mut progress: impl FnMut(ConflictProgress),
) -> Result<AbortOutcome, GitError> {
    // 1. Preserve the buffer BEFORE touching the repo (never lose partial
    //    work) — but only when there IS live work to preserve.
    //
    // With nothing unmerged — the #704 state, where the resolution has already
    // been staged — a buffer built from the index is empty, and `autosave`
    // would write that emptiness over the drafts already on disk: the exact
    // opposite of the ADR-0057 promise this step exists to keep (#707 review).
    // Nothing new to save, so the existing autosave is left alone.
    let buffer_preserved_at = if session.files.is_empty() {
        None
    } else {
        buffer.autosave().ok()
    };

    // 2. Resolve ORIG_HEAD (the pre-operation HEAD).
    //
    // #369: `git cherry-pick` and `git revert` do NOT write ORIG_HEAD (only
    // merge / rebase / reset do), yet HEAD has not moved — they fail before
    // committing — so the pre-op tree is exactly the current HEAD. Fall back to
    // it for those ops, otherwise the whole restore-and-guard block below is
    // skipped and a mid-conflict staged edit to a non-conflicted file slips
    // through unprotected (and the working tree keeps its conflict markers).
    let orig_sha = read_orig_head(repo).or_else(|| {
        matches!(
            session.op,
            ConflictOp::CherryPick { .. } | ConflictOp::Revert { .. }
        )
        .then(|| {
            repo.head()
                .ok()
                .and_then(|h| h.target())
                .map(|o| o.to_string())
        })
        .flatten()
    });

    // 3. If we know ORIG_HEAD, restore the working tree + index to its tree,
    //    then move the branch ref back.
    if let Some(ref sha) = orig_sha {
        let oid = git2::Oid::from_str(sha)
            .map_err(|e| GitError::Other(format!("bad ORIG_HEAD {}: {}", sha, e.message())))?;
        let commit = repo.find_commit(oid).map_err(|e| {
            GitError::Other(format!("ORIG_HEAD commit lookup failed: {}", e.message()))
        })?;
        let tree = commit.tree().map_err(|e| {
            GitError::Other(format!("ORIG_HEAD tree lookup failed: {}", e.message()))
        })?;

        if repo.workdir().is_none() {
            return Err(GitError::Other(
                "repository has no working tree".to_string(),
            ));
        }

        // Restore the working tree + index to the pre-operation tree.
        //
        // The old implementation only rewrote `session.files` (the *conflicting*
        // paths) and left every cleanly-merged incoming file — and every file
        // the operation added — on disk (issue #278: 9,997 stray "modified"
        // files after a 10k-file merge).  The operation checked out its whole
        // result tree, so the whole result tree has to be rolled back.
        //
        // The path set is computed *before* anything is mutated, so a failure
        // there leaves the conflicted state intact rather than half-restored.
        let touched = op_touched_paths(repo, &tree, session)?;

        // Refuse if the user edited a *cleanly-merged* file while in Conflict
        // Mode.  The force-checkout below is justified by "whatever stands at
        // a touched path is the operation's own output" — which is true when
        // the conflict state was entered, but not necessarily at abort time:
        // Conflict Mode is an editing session, and nothing stops the user
        // from changing a non-conflicted file through the Editor meanwhile.
        // Real git refuses exactly here ("Entry 'b.txt' not uptodate. Cannot
        // merge.") and this matches it.  Conflicted paths are exempt: abort
        // discards resolution progress by design.
        //
        // Checked BEFORE any mutation, so the refusal leaves the conflicted
        // state fully intact.
        let edited = mid_conflict_edits(repo, session, &touched, oid, &tree)?;
        if !edited.is_empty() {
            return Err(GitError::Other(format!(
                "abort refused: {} file(s) were edited during conflict resolution and would be overwritten: {}. Commit, stash or revert those edits first.",
                edited.len(),
                edited.join(", ")
            )));
        }

        {
            let mut index = repo
                .index()
                .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
            index
                .read_tree(&tree)
                .map_err(|e| GitError::Other(format!("index.read_tree failed: {}", e.message())))?;
            index
                .write()
                .map_err(|e| GitError::Other(format!("index.write failed: {}", e.message())))?;
        }
        // Past this point the repository has been changed: everything that
        // follows must be reported as a started mutation, never as "nothing
        // happened" (ADR-0196). The stage names what has *actually* been
        // written — a checkout that then fails must not leave evidence
        // claiming the working tree was rewritten.
        progress(ConflictProgress::IndexWritten);

        checkout_paths_from_tree(repo, &tree, &touched)?;
        progress(ConflictProgress::IndexAndWorktreeWritten);

        // Restore the branch ref back to ORIG_HEAD and reattach HEAD to it.
        //
        // #302: during a rebase HEAD is DETACHED, so `repo.head().name()` is
        // literally "HEAD" — the old code then wrote `HEAD` as a *direct* ref
        // pointing at ORIG_HEAD, stranding the user in detached HEAD. Real
        // `git rebase --abort` returns to the branch. The pre-op branch name is
        // recorded in `.git/rebase-merge/head-name` (merge backend) or
        // `.git/rebase-apply/head-name` (apply backend); read it, point that
        // branch at ORIG_HEAD, and `set_head` to it. Merge / cherry-pick /
        // revert keep a symbolic HEAD, so `repo.head().name()` already yields
        // the branch — that path is unchanged. The error is no longer swallowed.
        let reflog = format!("abort {}: restore ORIG_HEAD", session.op.slug());
        let branch_ref: Option<String> = match session.op {
            ConflictOp::Rebase { .. } => read_rebase_head_name(repo.path()),
            _ => repo
                .head()
                .ok()
                .and_then(|h| h.name().map(str::to_string).ok())
                .filter(|n| n != "HEAD"),
        };
        match branch_ref {
            Some(name) => {
                repo.reference(&name, oid, true, &reflog).map_err(|e| {
                    GitError::Other(format!(
                        "restore {} to ORIG_HEAD failed: {}",
                        name,
                        e.message()
                    ))
                })?;
                repo.set_head(&name).map_err(|e| {
                    GitError::Other(format!("reattach HEAD to {} failed: {}", name, e.message()))
                })?;
            }
            None => {
                // Genuinely detached (no branch to return to): point HEAD at
                // ORIG_HEAD directly, as before.
                repo.set_head_detached(oid).map_err(|e| {
                    GitError::Other(format!(
                        "set detached HEAD to ORIG_HEAD failed: {}",
                        e.message()
                    ))
                })?;
            }
        }
    }

    // 4. Clear merge / sequencer metadata (MERGE_HEAD, CHERRY_PICK_HEAD, etc.).
    // With no ORIG_HEAD there was nothing to restore, so this is the first
    // thing the abort touches and the only stage that describes it.
    progress(ConflictProgress::StateCleanupStarted);
    repo.cleanup_state()
        .map_err(|e| GitError::Other(format!("cleanup_state failed: {}", e.message())))?;

    // libgit2 clears the rebase directory but leaves Git's replay pseudoref.
    if matches!(session.op, ConflictOp::Rebase { .. }) {
        match repo.find_reference("REBASE_HEAD") {
            Ok(mut head) => head.delete().map_err(|e| {
                GitError::Other(format!("remove REBASE_HEAD failed: {}", e.message()))
            })?,
            Err(e) if e.code() == git2::ErrorCode::NotFound => {}
            Err(e) => {
                return Err(GitError::Other(format!(
                    "read REBASE_HEAD failed: {}",
                    e.message()
                )))
            }
        }
    }

    // No `Verified` here: verification is `verify_abort`, which the Backend
    // runs after this returns. Claiming it from the executor would record a
    // verified abort for a restore that verification then rejected.
    Ok(AbortOutcome {
        restored_to: orig_sha,
        buffer_preserved_at,
    })
}

/// Abort a stash-conflict (#309 / ADR-0148): restore HEAD's content for the
/// **conflicted paths only** and clear their unmerged index entries, leaving the
/// stash entry intact.
///
/// A conflicted `git stash apply`/`pop` writes **no** `ORIG_HEAD`, `MERGE_HEAD`
/// or sequencer state — the branch never moved and the pre-apply tree is exactly
/// HEAD (apply requires a clean tree). So this must NOT reuse
/// [`execute_conflict_abort`]'s ORIG_HEAD path: there is no ref to move and no
/// operation state to clean up. It only:
///
/// 1. preserves the resolution buffer (ADR-0057),
/// 2. drops the stage 1/2/3 conflict entries for the session's files, and
/// 3. force-checks-out HEAD over exactly those paths (pathspec-bounded — never a
///    repo-wide `reset --hard`), which repopulates them at stage 0 and rewrites
///    the working tree.
///
/// The stash entry is left untouched (dropping it, if wanted, is a separate,
/// explicit stash-drop op).
pub(crate) fn execute_stash_conflict_abort(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
) -> Result<AbortOutcome, GitError> {
    execute_stash_conflict_abort_with_progress(repo, session, buffer, |_| {})
}

/// [`execute_stash_conflict_abort`], reporting how far the mutation got — see
/// [`execute_conflict_abort_with_progress`].
pub(crate) fn execute_stash_conflict_abort_with_progress(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
    mut progress: impl FnMut(ConflictProgress),
) -> Result<AbortOutcome, GitError> {
    // 1. Preserve the buffer BEFORE touching the repo (never lose partial
    //    work) — but only when there IS live work to preserve.
    //
    // With nothing unmerged — the #704 state, where the resolution has already
    // been staged — a buffer built from the index is empty, and `autosave`
    // would write that emptiness over the drafts already on disk: the exact
    // opposite of the ADR-0057 promise this step exists to keep (#707 review).
    // Nothing new to save, so the existing autosave is left alone.
    let buffer_preserved_at = if session.files.is_empty() {
        None
    } else {
        buffer.autosave().ok()
    };

    if repo.workdir().is_none() {
        return Err(GitError::Other(
            "repository has no working tree".to_string(),
        ));
    }

    // 2. Resolve the HEAD commit + tree (the pre-apply state).
    let head_commit = repo
        .head()
        .ok()
        .and_then(|h| h.target())
        .and_then(|oid| repo.find_commit(oid).ok())
        .ok_or_else(|| GitError::Other("HEAD commit lookup failed".to_string()))?;
    let tree = head_commit
        .tree()
        .map_err(|e| GitError::Other(format!("HEAD tree lookup failed: {}", e.message())))?;

    // 3. Conflicted paths only (pathspec-bounded restore).
    let paths: Vec<String> = session
        .files
        .iter()
        .filter_map(|f| f.path.to_str().map(str::to_string))
        .collect();

    // 4. Drop the stage 1/2/3 conflict entries so the paths are no longer
    //    unmerged; the checkout below repopulates them at stage 0 from HEAD.
    {
        let mut index = repo
            .index()
            .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
        for f in &session.files {
            // Best effort: a path with no conflict entry is already resolved.
            let _ = index.conflict_remove(&f.path);
        }
        index
            .write()
            .map_err(|e| GitError::Other(format!("index.write failed: {}", e.message())))?;
    }
    progress(ConflictProgress::IndexWritten);

    // 5. Restore HEAD content for exactly those paths (force, pathspec-bounded).
    checkout_paths_from_tree(repo, &tree, &paths)?;
    progress(ConflictProgress::IndexAndWorktreeWritten);

    // NOTE: no `cleanup_state`, no ref move, no ORIG_HEAD — there is none. The
    // stash entry is deliberately left intact. `Verified` is the Backend's to
    // set, after `verify_abort`.
    Ok(AbortOutcome {
        restored_to: Some(head_commit.id().to_string()),
        buffer_preserved_at,
    })
}

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
fn mid_conflict_edits(
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
fn op_touched_paths(
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

/// Check `tree` out over exactly `paths` (and nothing else), removing paths the
/// tree does not contain.
///
/// # Why this is `force()` and still safe
///
/// A *safe* checkout cannot do this job: the index has just been read back to
/// `tree`, so libgit2 sees no index→tree diff for these paths, treats the
/// operation's own output on disk as a local modification and skips it —
/// exactly the residue this is meant to clear.  `force()` compares the working
/// tree against `tree` directly and rewrites it.
///
/// The force is bounded by the pathspec to the paths the aborted operation
/// itself wrote, and whatever wrote them refused to overwrite local
/// modifications: kagi's own merge/cherry-pick/revert use
/// `CheckoutBuilder::safe()`, and the operations that did NOT go through a
/// kagi checkout — rebase (shelled out to `git`, ops/rebase.rs) and conflicts
/// entered from the CLI and picked up by the watcher — were written by real
/// git, which equally refuses to clobber locally-modified files.  So the
/// content standing at these paths is the operation's own output, never
/// pre-operation user work — dropping it is precisely what
/// `git merge --abort` does.  (Edits made DURING Conflict Mode are the one
/// exception, and `mid_conflict_edits` blocks the abort on those first.)  Paths outside the pathspec (including unrelated
/// dirty and untracked files) are not candidates — with the caveat that
/// libgit2's `disable_pathspec_match` disables fnmatch but keeps dirname
/// prefix matching, so a pathspec entry that names a directory (e.g. a
/// gitlink delta) would cover its contents.  `remove_untracked` is
/// likewise pathspec-bounded: it removes the files the operation *added*
/// (untracked once the index is back at `tree`), which is again what real
/// `git merge --abort` does.
fn checkout_paths_from_tree(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    paths: &[String],
) -> Result<(), GitError> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.force();
    cb.remove_untracked(true);
    cb.disable_pathspec_match(true);
    for path in paths {
        cb.path(path.as_str());
    }
    repo.checkout_tree(tree.as_object(), Some(&mut cb))
        .map_err(|e| GitError::Other(format!("checkout_tree (abort) failed: {}", e.message())))
}

/// Read the pre-rebase branch ref name (e.g. `refs/heads/feature`) from
/// `.git/rebase-merge/head-name` (merge backend) or `.git/rebase-apply/head-name`
/// (apply backend). Returns `None` when neither file exists or it doesn't name a
/// ref — the caller then falls back to a detached restore (#302).
fn read_rebase_head_name(git_dir: &Path) -> Option<String> {
    for sub in ["rebase-merge", "rebase-apply"] {
        if let Ok(raw) = std::fs::read_to_string(git_dir.join(sub).join("head-name")) {
            let name = raw.trim();
            if name.starts_with("refs/") {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Read `ORIG_HEAD` as a 40-char sha string, if present.
fn read_orig_head(repo: &Repository) -> Option<String> {
    let raw = std::fs::read_to_string(repo.path().join("ORIG_HEAD")).ok()?;
    let sha = raw.trim();
    if sha.is_empty() {
        None
    } else {
        Some(sha.to_string())
    }
}
