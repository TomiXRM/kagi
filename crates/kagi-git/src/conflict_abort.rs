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

use super::conflict_abort_guard::{mid_conflict_edits, op_touched_paths};
use super::conflicts::{current_state_summary, short_sha, ConflictOp, ConflictSession};
use super::ops::{OperationPlan, StateSummary};
use super::resolution::ResolutionBuffer;
use super::{resolve_head, GitError};

/// The ref an abort will move back to `ORIG_HEAD`, and where it pointed when
/// the operation was observed (#707 review).
///
/// A rebase leaves HEAD detached, so nothing else in the conflict fingerprint
/// says where `refs/heads/<pre-rebase-branch>` currently points — and the
/// restore used to be a `force` write. An external `git update-ref` on that
/// branch during the confirmation therefore passed preflight and was silently
/// overwritten. Both halves are fixed from here: the fingerprint folds this in
/// (so the plan is refused), and the write is a compare-and-swap against
/// `target` (so the window between preflight and the write is closed too).
///
/// Crate-private, and so is the executor that takes one: a caller able to name
/// the ref could hand the abort any branch whose current OID it knows and have
/// it moved to `ORIG_HEAD` with HEAD attached to it. The destination is the
/// *session's* to decide (#707 4th review).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RestoreRef {
    /// Full ref name, e.g. `refs/heads/side`.
    pub(crate) name: String,
    /// Its resolved target when observed. `None` = the ref did not exist, or
    /// is symbolic / unreadable — cases the restore must not silently create
    /// or clobber either.
    pub(crate) target: Option<String>,
}

/// Which ref this abort would rewrite, if any. The one place that decision is
/// made: the fingerprint and the executor must never disagree about it.
///
/// #302: during a rebase HEAD is DETACHED, so `repo.head().name()` is literally
/// "HEAD"; the pre-op branch name is in `.git/rebase-merge/head-name` (merge
/// backend) or `.git/rebase-apply/head-name` (apply backend). Merge /
/// cherry-pick / revert keep a symbolic HEAD, so `repo.head().name()` already
/// yields the branch. `None` = genuinely detached, nothing to move.
pub(crate) fn restore_ref(repo: &Repository, session: &ConflictSession) -> Option<RestoreRef> {
    let name = match session.op {
        ConflictOp::Rebase { .. } => read_rebase_head_name(repo.path()),
        _ => repo
            .head()
            .ok()
            .and_then(|head| head.name().map(str::to_string).ok())
            .filter(|name| name != "HEAD"),
    }?;
    let target = repo
        .find_reference(&name)
        .ok()
        .and_then(|reference| reference.target())
        .map(|oid| oid.to_string());
    Some(RestoreRef { name, target })
}

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
    progress: impl FnMut(ConflictProgress),
) -> Result<AbortOutcome, GitError> {
    // The destination is read here, so no caller can choose it. A plan that
    // froze one earlier goes through `execute_conflict_abort_expecting`, which
    // is crate-private for exactly that reason (#707 4th review).
    let expected = restore_ref(repo, session);
    execute_conflict_abort_expecting(repo, session, buffer, expected.as_ref(), progress)
}

/// [`execute_conflict_abort_with_progress`] against an expectation the plan
/// froze, so the compare-and-swap covers the window between the Backend's live
/// preflight and the write. Crate-private: `expected` supplies the *OID* to
/// swap against, never which ref is written — that is `restore_ref`'s, and a
/// mismatch is refused before anything is touched.
pub(crate) fn execute_conflict_abort_expecting(
    repo: &Repository,
    session: &ConflictSession,
    buffer: &ResolutionBuffer,
    expected: Option<&RestoreRef>,
    mut progress: impl FnMut(ConflictProgress),
) -> Result<AbortOutcome, GitError> {
    // Before anything at all, including the buffer autosave: an expectation
    // that names a ref this session does not restore is a caller trying to
    // choose the destination, and must leave the repository untouched.
    let destination = restore_ref(repo, session);
    if expected.map(|expected| &expected.name) != destination.as_ref().map(|live| &live.name) {
        return Err(GitError::Other(format!(
            "abort refused: {} is not the ref this operation restores",
            expected.map_or("a detached restore", |expected| expected.name.as_str())
        )));
    }
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
        // Real `git rebase --abort` returns to the branch rather than leaving
        // the user detached (#302); `restore_ref` is where that branch is
        // decided, for the fingerprint and for here alike.
        let reflog = format!("abort {}: restore ORIG_HEAD", session.op.slug());
        match &destination {
            // The name is the live one; only the OID to swap against came from
            // the plan, and the guard above proved the two name the same ref.
            Some(RestoreRef { name, .. }) => {
                // Compare-and-swap against the OID this ref held when the
                // operation was observed. The fingerprint covers that value, so
                // a plan built before an external `git update-ref` is already
                // refused — this closes the remaining window, between the live
                // preflight and this write (#707 review).
                let write = match expected.and_then(|expected| expected.target.as_ref()) {
                    Some(expected_target) => {
                        let expected_oid = git2::Oid::from_str(expected_target).map_err(|e| {
                            GitError::Other(format!(
                                "bad expected target {expected_target}: {}",
                                e.message()
                            ))
                        })?;
                        repo.reference_matching(name, oid, true, expected_oid, &reflog)
                    }
                    // It did not resolve when observed; only create it if that
                    // is still true, never overwrite what appeared meanwhile.
                    None => repo.reference(name, oid, false, &reflog),
                };
                write.map_err(|e| {
                    GitError::Other(format!(
                        "restore {} to ORIG_HEAD failed: {}",
                        name,
                        e.message()
                    ))
                })?;
                repo.set_head(name).map_err(|e| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::{Mutex, MutexGuard};

    /// The resolution-buffer autosave directory is process-global, so these
    /// serialize and point it at their own tempdir (the convention the crate's
    /// oplog tests already follow).
    static ENV: Mutex<()> = Mutex::new(());

    struct Isolated {
        _lock: MutexGuard<'static, ()>,
        _log: tempfile::TempDir,
        previous: Option<std::ffi::OsString>,
    }

    impl Isolated {
        fn new() -> Self {
            let lock = ENV.lock().unwrap_or_else(|error| error.into_inner());
            let log = tempfile::tempdir().unwrap();
            let previous = std::env::var_os("KAGI_LOG_DIR");
            std::env::set_var("KAGI_LOG_DIR", log.path());
            Self {
                _lock: lock,
                _log: log,
                previous,
            }
        }
    }

    impl Drop for Isolated {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var("KAGI_LOG_DIR", value),
                None => std::env::remove_var("KAGI_LOG_DIR"),
            }
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .status()
            .expect("git");
        // `rebase` exits non-zero on the conflict this fixture wants.
        let _ = status;
    }

    fn rev_parse(dir: &Path, rev: &str) -> String {
        let out = Command::new("git")
            .args(["rev-parse", rev])
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("rev-parse");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A rebase stopped on a conflict, plus an unrelated `refs/heads/protected`.
    fn rebase_conflict_with_a_bystander_branch() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main", "."]);
        git(dir, &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.join("file.txt"), "base\n").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-qm", "base"]);
        git(dir, &["branch", "protected"]);
        git(dir, &["checkout", "-q", "-b", "side"]);
        std::fs::write(dir.join("file.txt"), "SIDE\n").unwrap();
        git(dir, &["commit", "-qam", "side"]);
        git(dir, &["checkout", "-q", "main"]);
        std::fs::write(dir.join("file.txt"), "MAIN\n").unwrap();
        git(dir, &["commit", "-qam", "main"]);
        git(dir, &["checkout", "-q", "side"]);
        git(dir, &["rebase", "main"]);
        tmp
    }

    /// #707 4th review: the ref an abort rewrites is the session's, never the
    /// caller's. A well-formed expectation naming an unrelated branch — whose
    /// current OID the caller does know, so the compare-and-swap itself would
    /// succeed — must be refused before anything is written. (The type and this
    /// entry point are crate-private for the same reason; this is the belt.)
    #[test]
    fn a_forged_expectation_cannot_choose_which_ref_the_abort_rewrites() {
        let _isolated = Isolated::new();
        let tmp = rebase_conflict_with_a_bystander_branch();
        let dir = tmp.path();
        let repo = Repository::open(dir).unwrap();
        let Some(session) = crate::conflicts::detect_conflict_session(&repo) else {
            // git is too old / behaves differently: nothing to assert about.
            return;
        };
        assert!(matches!(session.op, ConflictOp::Rebase { .. }));
        let buffer = ResolutionBuffer::from_repo(&repo).unwrap();

        let protected_before = rev_parse(dir, "refs/heads/protected");
        let side_before = rev_parse(dir, "refs/heads/side");
        let head_before = rev_parse(dir, "HEAD");
        let worktree_before = std::fs::read_to_string(dir.join("file.txt")).unwrap();
        let index_before = std::fs::read(dir.join(".git/index")).unwrap();

        let forged = RestoreRef {
            name: "refs/heads/protected".to_string(),
            target: Some(protected_before.clone()),
        };
        let error =
            execute_conflict_abort_expecting(&repo, &session, &buffer, Some(&forged), |_| {})
                .expect_err("a caller must not name the ref an abort rewrites");
        assert!(
            format!("{error}").contains("refs/heads/protected"),
            "the refusal names the ref it would not touch: {error}"
        );

        assert_eq!(rev_parse(dir, "refs/heads/protected"), protected_before);
        assert_eq!(rev_parse(dir, "refs/heads/side"), side_before);
        assert_eq!(rev_parse(dir, "HEAD"), head_before);
        assert_eq!(
            std::fs::read_to_string(dir.join("file.txt")).unwrap(),
            worktree_before
        );
        assert_eq!(std::fs::read(dir.join(".git/index")).unwrap(), index_before);
        assert!(dir.join(".git/rebase-merge").exists());
    }

    /// The other half: an expectation for the right ref but a stale OID is a
    /// compare-and-swap failure, not a silent overwrite.
    #[test]
    fn a_stale_expected_target_refuses_rather_than_overwriting() {
        let _isolated = Isolated::new();
        let tmp = rebase_conflict_with_a_bystander_branch();
        let dir = tmp.path();
        let repo = Repository::open(dir).unwrap();
        let Some(session) = crate::conflicts::detect_conflict_session(&repo) else {
            return;
        };
        let buffer = ResolutionBuffer::from_repo(&repo).unwrap();
        let stale = RestoreRef {
            name: "refs/heads/side".to_string(),
            target: Some(rev_parse(dir, "refs/heads/protected")),
        };
        let side_before = rev_parse(dir, "refs/heads/side");

        let error =
            execute_conflict_abort_expecting(&repo, &session, &buffer, Some(&stale), |_| {})
                .expect_err("the swap must not succeed against the wrong old value");
        assert!(format!("{error}").contains("refs/heads/side"), "{error}");
        assert_eq!(rev_parse(dir, "refs/heads/side"), side_before);
    }
}
