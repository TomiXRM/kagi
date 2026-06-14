//! Commit log backend — T004
//!
//! Provides the [`CommitId`], [`Signature`], and [`Commit`] domain models
//! (architecture.md §3), and the [`commit_log`] function that walks all
//! refs reachable from `refs/heads/*`, `refs/remotes/*`, `refs/tags/*`, and
//! `HEAD` (for detached-HEAD support), returning a topologically-sorted
//! `Vec<Commit>` (newest/children first, parents always come after).

use git2::{Repository, Sort};

use super::GitError;

// ────────────────────────────────────────────────────────────
// Domain models (architecture.md §3)
// ────────────────────────────────────────────────────────────
//
// `CommitId`, `Signature`, and `Commit` now live in the pure `kagi-domain`
// crate (ADR-0072). They are re-exported here so the existing
// `kagi::git::{Commit, CommitId, Signature}` paths keep resolving and so the
// git2-backed `commit_log` below can construct them.
pub use kagi_domain::commit::{Commit, CommitId, Signature};

// ────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────

/// Walk all commits reachable from any local branch, remote-tracking branch,
/// or tag, and return them in topological order (child commits before their
/// parents).
///
/// # Parameters
///
/// * `repo`  — an already-opened [`Repository`].
/// * `limit` — maximum number of commits to return.  Pass `10_000` for the
///   MVP; there is no "load more" yet.
///
/// # Ordering
///
/// The walk uses `Sort::TOPOLOGICAL | Sort::TIME` so that:
/// - every parent commit appears **after** all of its children, and
/// - among commits at the same topological depth the one with the newer
///   committer timestamp comes first (stable, git-log-like ordering).
///
/// # Unborn / empty repository
///
/// If the repository has no commits yet (`HEAD` is unborn and no refs exist),
/// this function returns an empty `Vec` rather than an error.
///
/// # Non-UTF-8 content
///
/// Author names, email addresses, and commit messages that are not valid UTF-8
/// are converted with `String::from_utf8_lossy` (replacement characters
/// inserted) so the function never panics on exotic byte sequences.
///
/// # Errors
///
/// Returns [`GitError::Other`] on unexpected libgit2 failures (e.g. a
/// corrupted object database).
pub fn commit_log(repo: &Repository, limit: usize) -> Result<Vec<Commit>, GitError> {
    // NOTE: an unborn HEAD does NOT imply an empty repository — after
    // `git checkout --orphan` HEAD is unborn while other branches still hold
    // commits.  So there is deliberately no HEAD-based short-circuit here;
    // the ref globs below pick up whatever exists, and `push_head()`
    // tolerates the unborn case.  (`repo.is_empty()` is also unreliable: it
    // compares HEAD against the *system* default branch name and returns
    // `false` for `git init -b main` repos with no commits.)
    let mut walk = repo
        .revwalk()
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    // Track whether we pushed at least one starting point.
    let mut any_pushed = false;

    // Push all reachable commits from every local branch, remote-tracking
    // branch, and tag.  push_glob may return NotFound when there are no
    // matching refs (e.g. a brand-new repo with no commits); treat that as
    // "nothing to walk" rather than an error.
    for glob in &["refs/heads/*", "refs/remotes/*", "refs/tags/*"] {
        match walk.push_glob(glob) {
            Ok(()) => {
                any_pushed = true;
            }
            Err(ref e) if e.code() == git2::ErrorCode::NotFound => {
                // No refs match this glob — skip silently.
            }
            Err(e) => return Err(GitError::Other(e.message().to_string())),
        }
    }

    // Also push HEAD for detached-HEAD repositories.  Skip it when HEAD does
    // not resolve to a commit (unborn repo / orphan checkout): push_head()
    // would fail with a "reference not found" error in that state.
    if repo.head().is_ok() {
        walk.push_head()
            .map_err(|e| GitError::Other(e.message().to_string()))?;
        any_pushed = true;
    }

    // If nothing was pushed, there are no commits to walk.
    if !any_pushed {
        return Ok(Vec::new());
    }

    let mut commits = Vec::new();

    for oid_result in walk.by_ref().take(limit) {
        let oid = match oid_result {
            Ok(oid) => oid,
            Err(ref e) if e.code() == git2::ErrorCode::NotFound => {
                // Revwalk may yield NotFound when the starting ref resolved to
                // nothing (e.g. a just-initialised repo with no commits).
                break;
            }
            Err(e) => return Err(GitError::Other(e.message().to_string())),
        };

        let raw = repo
            .find_commit(oid)
            .map_err(|e| GitError::Other(e.message().to_string()))?;

        let id = CommitId(oid.to_string());

        // Collect parents preserving order (parents()[0] = first parent).
        let parents: Vec<CommitId> = raw.parent_ids().map(|p| CommitId(p.to_string())).collect();

        let author = sig_from_git2(raw.author());
        let committer = sig_from_git2(raw.committer());

        let message = String::from_utf8_lossy(raw.message_bytes()).into_owned();
        // Summary = first non-empty line of the message.
        let summary = message.lines().next().unwrap_or("").trim_end().to_string();

        commits.push(Commit {
            id,
            parents,
            author,
            committer,
            summary,
            message,
        });
    }

    Ok(commits)
}

// ────────────────────────────────────────────────────────────
// Internal helpers
// ────────────────────────────────────────────────────────────

/// Convert a `git2::Signature` to our domain [`Signature`], applying
/// lossy UTF-8 conversion so non-UTF-8 bytes do not cause a panic.
fn sig_from_git2(sig: git2::Signature<'_>) -> Signature {
    let name = String::from_utf8_lossy(sig.name_bytes()).into_owned();
    let email = String::from_utf8_lossy(sig.email_bytes()).into_owned();
    let time = sig.when().seconds();
    Signature { name, email, time }
}
