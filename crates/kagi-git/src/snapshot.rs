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
    log::{commit_log, Commit, CommitId},
    refs::{Branch, RemoteBranch, Stash, Tag, UpstreamInfo, Worktree, WorktreeWip},
    resolve_head,
    status::{working_tree_status, WorkingTreeStatus},
    GitError, Head,
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
    /// Main + linked worktrees registered for this repository.
    pub worktrees: Vec<Worktree>,
    /// Wall-clock time (Unix seconds) of the last `git fetch`, from the
    /// `FETCH_HEAD` mtime — written on every fetch (including no-op ones, and
    /// by CLI fetches outside kagi). `None` when the repo has never fetched
    /// (no FETCH_HEAD). Drives the status-bar fetch-age indicator (ADR-0127).
    pub last_fetch_secs: Option<i64>,
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
    let worktrees = collect_worktrees(repo)?;

    Ok(RepoSnapshot {
        head,
        commits,
        status,
        branches,
        remote_branches,
        tags,
        stashes,
        worktrees,
        last_fetch_secs: last_fetch_secs(repo),
    })
}

// ────────────────────────────────────────────────────────────
// Internal helpers
// ────────────────────────────────────────────────────────────

/// `FETCH_HEAD` mtime as Unix seconds (ADR-0127) — git rewrites the file on
/// every fetch, no-op or not, so its mtime is "when this repo last talked to
/// a remote". Checked in both the worktree's private gitdir and the common
/// dir (a fetch run from another worktree writes the latter); the newest
/// wins. `None` when the repo has never fetched.
fn last_fetch_secs(repo: &Repository) -> Option<i64> {
    let mut best: Option<i64> = None;
    for dir in [repo.path(), repo.commondir()] {
        let secs = std::fs::metadata(dir.join("FETCH_HEAD"))
            .ok()
            .and_then(|md| md.modified().ok())
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        if let Some(s) = secs {
            if best.is_none_or(|b| s > b) {
                best = Some(s);
            }
        }
    }
    best
}

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

        branches.push(Branch {
            name,
            target,
            upstream,
        });
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

        remote_branches.push(RemoteBranch {
            remote,
            name,
            target,
        });
    }

    remote_branches.sort_by(|a, b| a.remote.cmp(&b.remote).then(a.name.cmp(&b.name)));

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
    // First pass: collect (index, message, oid). `stash_foreach` borrows `repo`
    // mutably, so we can't look up the base commit inside the closure.
    let mut raw: Vec<(usize, String, git2::Oid)> = Vec::new();
    repo.stash_foreach(|index, message, oid| {
        raw.push((index, message.to_owned(), *oid));
        true
    })
    .map_err(|e| GitError::Other(e.message().to_string()))?;

    // Second pass: resolve the base commit (stash commit's first parent) so the
    // graph can draw the stash's branch line down to where it sprouted.
    let stashes = raw
        .into_iter()
        .map(|(index, message, oid)| {
            let base = repo
                .find_commit(oid)
                .ok()
                .and_then(|c| c.parent_id(0).ok())
                .map(|p| CommitId(p.to_string()));
            Stash {
                index,
                message,
                target: CommitId(oid.to_string()),
                base,
            }
        })
        .collect();

    Ok(stashes)
}

/// Collect the primary worktree plus registered linked worktrees.
fn collect_worktrees(repo: &Repository) -> Result<Vec<Worktree>, GitError> {
    let current_path = repo.workdir().map(|p| p.to_path_buf()).unwrap_or_default();
    let main_path = if repo.is_worktree() {
        repo.commondir()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| current_path.clone())
    } else {
        current_path.clone()
    };

    // Compare canonicalized paths: `repo.workdir()` is canonical while the
    // registered worktree path may differ by symlink (e.g. macOS /var →
    // /private/var) or trailing slash, which would otherwise leave NO worktree
    // flagged `is_current` and break the interactive WIP row.
    let current_canon = canon(&current_path);

    let mut worktrees = Vec::new();
    let main_branch = worktree_branch_name(&main_path);
    worktrees.push(Worktree {
        name: "main".to_string(),
        path: main_path.clone(),
        branch: main_branch,
        is_current: canon(&main_path) == current_canon,
        is_main: true,
        wip: worktree_wip(&main_path),
    });

    let names = repo
        .worktrees()
        .map_err(|e| GitError::Other(e.message().to_string()))?;
    for name in names.iter() {
        let Ok(Some(name)) = name else {
            continue;
        };
        let wt = match repo.find_worktree(name) {
            Ok(wt) => wt,
            Err(_) => continue,
        };
        let path = wt.path().to_path_buf();
        let branch = worktree_branch_name(&path);
        let wip = worktree_wip(&path);
        worktrees.push(Worktree {
            name: name.to_string(),
            is_current: canon(&path) == current_canon,
            path,
            branch,
            is_main: false,
            wip,
        });
    }

    worktrees.sort_by(|a, b| {
        a.is_main
            .cmp(&b.is_main)
            .reverse()
            .then(a.name.cmp(&b.name))
    });
    Ok(worktrees)
}

/// Canonicalize `p` for identity comparison, falling back to the path as-is
/// when it cannot be resolved (e.g. a stale/pruned worktree).
fn canon(p: &std::path::Path) -> std::path::PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn worktree_branch_name(path: &std::path::Path) -> Option<String> {
    let repo = Repository::open(path).ok()?;
    let head = repo.head().ok()?;
    head.shorthand().ok().map(str::to_string)
}

/// Read pending-change counts for the worktree rooted at `path`.
///
/// Opens the worktree as its own repository so the status reflects *that*
/// working tree (each linked worktree has an independent index + workdir).
/// Returns `None` when the worktree is clean or its status cannot be read, so
/// the UI only draws a WIP row for worktrees that actually have changes.
fn worktree_wip(path: &std::path::Path) -> Option<WorktreeWip> {
    let repo = Repository::open(path).ok()?;
    let status = working_tree_status(&repo).ok()?;
    let wip = WorktreeWip {
        staged: status.staged.len(),
        unstaged: status.unstaged.len(),
        untracked: status.untracked.len(),
    };
    wip.is_dirty().then_some(wip)
}
