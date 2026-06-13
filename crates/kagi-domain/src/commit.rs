//! Commit domain models — pure data, no git2.
//!
//! [`CommitId`], [`Signature`], and [`Commit`] (architecture.md §3). The
//! git2-backed `commit_log` walk that *produces* these lives in the git-backend
//! layer (`kagi::git::log`), which depends on this module for the types.

// ────────────────────────────────────────────────────────────
// Domain models (architecture.md §3)
// ────────────────────────────────────────────────────────────

/// A 40-hex SHA-1 commit identifier.
///
/// Wraps the full hex string so callers do not have to manage raw `Oid`
/// conversions.  Use [`CommitId::short`] to obtain an 8-character prefix
/// suitable for display.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommitId(pub String);

impl CommitId {
    /// Returns the first 8 hex characters of the SHA, e.g. `"a1b2c3d4"`.
    pub fn short(&self) -> &str {
        self.0.get(..8).unwrap_or(&self.0)
    }
}

impl std::fmt::Display for CommitId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Author or committer identity with a Unix-epoch timestamp.
///
/// Display formatting of the timestamp (e.g. "2 days ago") is the
/// responsibility of the UI layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// Human-readable name, e.g. `"Alice"`.
    pub name: String,
    /// Email address, e.g. `"alice@example.com"`.
    pub email: String,
    /// Commit timestamp in seconds since the Unix epoch (UTC).
    pub time: i64,
}

/// A single Git commit in the domain model.
///
/// # Parent ordering guarantee
///
/// `parents[0]` is always the **first parent** (the branch being committed
/// onto).  For merge commits `parents[1..]` are the merged-in commits, in the
/// same order as `git log --pretty=%P`.  The slice may be empty for root
/// commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Unique identifier of this commit.
    pub id: CommitId,
    /// Parent commit identifiers.
    ///
    /// `parents[0]` is guaranteed to be the first parent.  Empty for root
    /// commits.
    pub parents: Vec<CommitId>,
    /// Commit author.
    pub author: Signature,
    /// Committer (may differ from author after rebase/cherry-pick).
    pub committer: Signature,
    /// First line of the commit message, stripped of trailing whitespace.
    pub summary: String,
    /// Full commit message, including the summary line.
    pub message: String,
}
