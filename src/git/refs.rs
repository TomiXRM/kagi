//! Branch, remote branch, tag, and stash domain models — T005
//!
//! Each type maps directly to the `architecture.md §3` data model.
//! All types are pure-Rust (no `git2` dependency) so the Domain Layer
//! stays independent of the Git backend.

use super::log::CommitId;

// ────────────────────────────────────────────────────────────
// Branch / UpstreamInfo
// ────────────────────────────────────────────────────────────

/// Tracking relationship between a local branch and its upstream.
///
/// Both `ahead` and `behind` are computed with `graph_ahead_behind` so they
/// reflect the true number of commits reachable from one side but not the
/// other (not just linear counting).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamInfo {
    /// Full remote-tracking ref name, e.g. `"origin/main"`.
    pub remote_branch: String,
    /// Commits in the local branch not yet in the upstream.
    pub ahead: usize,
    /// Commits in the upstream not yet in the local branch.
    pub behind: usize,
}

/// A local Git branch.
///
/// If the branch has an upstream configured (via `branch.<name>.remote` +
/// `branch.<name>.merge`), `upstream` is `Some` with the ahead/behind counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// Short branch name, e.g. `"main"` or `"feature/x"`.
    pub name: String,
    /// The commit this branch tip points to.
    pub target: CommitId,
    /// Upstream tracking info, if configured.
    pub upstream: Option<UpstreamInfo>,
}

// ────────────────────────────────────────────────────────────
// RemoteBranch
// ────────────────────────────────────────────────────────────

/// A remote-tracking branch, e.g. `origin/main`.
///
/// The full ref name `"refs/remotes/origin/main"` is split into
/// `remote = "origin"` and `name = "main"`.
///
/// Symbolic refs like `origin/HEAD` are **excluded** by the snapshot function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBranch {
    /// Remote name, e.g. `"origin"`.
    pub remote: String,
    /// Branch name on the remote, e.g. `"main"` or `"feature/x"`.
    pub name: String,
    /// The commit this remote-tracking ref resolves to.
    pub target: CommitId,
}

// ────────────────────────────────────────────────────────────
// Tag
// ────────────────────────────────────────────────────────────

/// A Git tag.
///
/// For annotated tags the target is the **commit** the tag object points to
/// (i.e. peeled through the tag object).  Lightweight tags already point
/// directly to a commit.  Tags that cannot be peeled to a commit (e.g. tags
/// pointing to a blob or tree) are **skipped** by the snapshot function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// Tag name without the `refs/tags/` prefix, e.g. `"v0.1.0"`.
    pub name: String,
    /// The commit this tag resolves to.
    pub target: CommitId,
}

// ────────────────────────────────────────────────────────────
// Stash
// ────────────────────────────────────────────────────────────

/// A single stash entry.
///
/// Stash **operations** (push / apply / drop) are out of scope for T005 and
/// will be handled in T015.  This type is read-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    /// Zero-based stash index (`stash@{N}`).
    pub index: usize,
    /// Message associated with the stash entry (from `git stash push -m`).
    pub message: String,
    /// Commit OID of the stash entry.
    pub target: CommitId,
}

// ────────────────────────────────────────────────────────────
// Worktree
// ────────────────────────────────────────────────────────────

/// A registered Git worktree shown in the Repository Navigator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// Worktree registry name. The main worktree is reported as `"main"`.
    pub name: String,
    /// Top-level working tree path.
    pub path: std::path::PathBuf,
    /// True for the repository currently opened by kagi.
    pub is_current: bool,
    /// True for the primary worktree rather than a linked worktree.
    pub is_main: bool,
}
