//! RepoSnapshot — T005
//!
//! Aggregates all read-only repository state into a single [`RepoSnapshot`]
//! that the App Layer can consume without holding a `git2::Repository` handle.
//!
//! [`snapshot`] is the sole entry point.  It re-uses the existing
//! [`resolve_head`], [`commit_log`], and [`working_tree_status`] functions;
//! no duplication of their logic.

use git2::{BranchType, Repository};

use super::{
    GitError, Head,
    log::{CommitId, Commit, commit_log},
    refs::{Branch, RemoteBranch, Stash, Tag, UpstreamInfo},
    resolve_head,
    status::{WorkingTreeStatus, working_tree_status},
};

// ────────────────────────────────────────────────────────────
// Public type
// ────────────────────────────────────────────────────────────

/// A complete, immutable snapshot of repository state.
///
/// This is the unit passed from the Git backend to the App layer.
/// All fields are pure-Rust types with no `git2` dependency.
///
/// # Construction
///
/// Use [`snapshot`] to populate this from a live `git2::Repository`.
#[derive(Debug, Clone)]
pub struct RepoSnapshot {
    /// Current HEAD state (attached / detached / unborn).
    pub head: Head,
    /// All commits reachable from any ref, in topological order.
    pub commits: Vec<Commit>,
    /// Local branches, ordered by name.
    pub branches: Vec<Branch>,
    /// Remote-tracking branches (excluding `origin/HEAD` symbolic refs).
    pub remote_branches: Vec<RemoteBranch>,
    /// Tags (both lightweight and annotated, peeled to commit).
    pub tags: Vec<Tag>,
    /// Working tree status.
    pub status: WorkingTreeStatus,
    /// Stash entries, ordered by index (newest first, i.e. `stash@{0}` first).
    pub stashes: Vec<Stash>,
}

// ────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────

/// Read all repository state and return a [`RepoSnapshot`].
///
/// # Arguments
///
/// * `repo`         — A mutable reference to an already-opened [`Repository`].
///   `&mut` is required because `stash_foreach` mutably borrows the repo.
/// * `commit_limit` — Maximum number of commits to include in `commits`.
///   Pass `10_000` for the MVP.
///
/// # Unborn / empty repositories
///
/// If `HEAD` is unborn (no commits yet), `branches`, `remote_branches`,
/// `tags`, and `stashes` will all be empty, and `commits` will be an empty
/// `Vec`.  The function does **not** return an error in this case.
///
/// # Errors
///
/// Returns [`GitError::Other`] on unexpected `git2` failures.
pub fn snapshot(repo: &mut Repository, commit_limit: usize) -> Result<RepoSnapshot, GitError> {
    let head = resolve_head(repo)?;
    let commits = commit_log(repo, commit_limit)?;
    let status = working_tree_status(repo)?;
    let branches = collect_branches(repo, &head)?;
    let remote_branches = collect_remote_branches(repo)?;
    let tags = collect_tags(repo)?;
    let stashes = collect_stashes(repo)?;

    Ok(RepoSnapshot {
        head,
        commits,
        status,
        branches,
        remote_branches,
        tags,
        stashes,
    })
}

// ────────────────────────────────────────────────────────────
// Internal helpers
// ────────────────────────────────────────────────────────────

/// Collect all local branches with upstream ahead/behind information.
fn collect_branches(repo: &Repository, _head: &Head) -> Result<Vec<Branch>, GitError> {
    let iter = repo
        .branches(Some(BranchType::Local))
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let mut branches = Vec::new();

    for item in iter {
        let (branch_ref, _) = item.map_err(|e| GitError::Other(e.message().to_string()))?;

        // Branch name (short, e.g. "main").
        let name = match branch_ref.name() {
            Ok(Some(n)) => n.to_owned(),
            Ok(None) => continue, // non-UTF-8 name — skip
            Err(_) => continue,
        };

        // Target commit OID.
        let target_oid = match branch_ref.get().target() {
            Some(oid) => oid,
            None => continue, // symbolic ref — skip
        };
        let target = CommitId(target_oid.to_string());

        // Upstream tracking info (optional).
        let upstream: Option<UpstreamInfo> = match branch_ref.upstream() {
            Ok(upstream_ref) => {
                let upstream_name = upstream_ref
                    .name()
                    .ok()
                    .flatten()
                    .map(|n| n.to_owned())
                    .unwrap_or_default();

                // upstream_ref.get().target() returns None for symbolic refs
                // (e.g. upstream is origin/HEAD). In that case skip ahead/behind.
                if let Some(up_oid) = upstream_ref.get().target() {
                    match repo.graph_ahead_behind(target_oid, up_oid) {
                        Ok((ahead, behind)) => Some(UpstreamInfo {
                            remote_branch: upstream_name,
                            ahead,
                            behind,
                        }),
                        Err(_) => None,
                    }
                } else {
                    None
                }
            }
            Err(_) => None, // no upstream configured
        };

        branches.push(Branch { name, target, upstream });
    }

    branches.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(branches)
}

/// Collect all remote-tracking branches, excluding symbolic refs like `origin/HEAD`.
fn collect_remote_branches(repo: &Repository) -> Result<Vec<RemoteBranch>, GitError> {
    let iter = repo
        .branches(Some(BranchType::Remote))
        .map_err(|e| GitError::Other(e.message().to_string()))?;

    let mut remote_branches = Vec::new();

    for item in iter {
        let (branch_ref, _) = item.map_err(|e| GitError::Other(e.message().to_string()))?;

        // Full short name, e.g. "origin/main" or "origin/HEAD".
        let full_name = match branch_ref.name() {
            Ok(Some(n)) => n.to_owned(),
            Ok(None) => continue,
            Err(_) => continue,
        };

        // Exclude names ending in "/HEAD" (symbolic ref aliases).
        if full_name.ends_with("/HEAD") {
            continue;
        }

        // Exclude symbolic refs: a symbolic ref has no direct OID target.
        let target_oid = match branch_ref.get().target() {
            Some(oid) => oid,
            None => continue,
        };

        // Split "origin/main" → remote="origin", name="main".
        // Use first '/' as separator.
        let (remote, name) = match full_name.split_once('/') {
            Some((r, n)) => (r.to_owned(), n.to_owned()),
            None => continue, // unexpected format — skip
        };

        let target = CommitId(target_oid.to_string());

        remote_branches.push(RemoteBranch { remote, name, target });
    }

    remote_branches.sort_by(|a, b| {
        a.remote.cmp(&b.remote).then(a.name.cmp(&b.name))
    });

    Ok(remote_branches)
}

/// Collect all tags, peeling annotated tags to their target commit.
///
/// Tags that cannot be peeled to a commit (e.g. pointing to a blob or tree)
/// are silently skipped.
fn collect_tags(repo: &Repository) -> Result<Vec<Tag>, GitError> {
    let mut tags = Vec::new();

    repo.tag_foreach(|oid, name_bytes| {
        // Name is "refs/tags/<name>" in bytes.
        let full = match std::str::from_utf8(name_bytes) {
            Ok(s) => s,
            Err(_) => return true, // skip non-UTF-8 tag names
        };
        let name = match full.strip_prefix("refs/tags/") {
            Some(n) => n.to_owned(),
            None => return true,
        };

        // Try to find the object and peel it to a commit.
        // For lightweight tags, `oid` is already a commit OID.
        // For annotated tags, `oid` is the tag object; we must peel.
        let obj = match repo.find_object(oid, None) {
            Ok(o) => o,
            Err(_) => return true, // object not found — skip
        };

        let commit = match obj.peel_to_commit() {
            Ok(c) => c,
            Err(_) => return true, // not peelable to commit — skip
        };

        let target = CommitId(commit.id().to_string());
        tags.push(Tag { name, target });
        true
    })
    .map_err(|e| GitError::Other(e.message().to_string()))?;

    tags.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(tags)
}

/// Collect all stash entries via `stash_foreach`.
///
/// Requires `&mut Repository` as `git2::Repository::stash_foreach` takes
/// `&mut self`.
fn collect_stashes(repo: &mut Repository) -> Result<Vec<Stash>, GitError> {
    let mut stashes = Vec::new();

    repo.stash_foreach(|index, message, oid| {
        stashes.push(Stash {
            index,
            message: message.to_owned(),
            target: CommitId(oid.to_string()),
        });
        true
    })
    .map_err(|e| GitError::Other(e.message().to_string()))?;

    Ok(stashes)
}
