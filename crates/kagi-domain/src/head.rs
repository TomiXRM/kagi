//! HEAD state domain model -- pure data, no git2.
//!
//! The git2-backed `resolve_head` function that populates this model lives in
//! the git-backend layer (`kagi::git`).

/// The HEAD state of a repository, as defined in architecture.md section 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// HEAD points to a branch (normal state).
    Attached {
        /// Short branch name, e.g. `"main"`.
        branch: String,
        /// Hex SHA of the commit HEAD -> branch tip resolves to.
        target: String,
    },
    /// HEAD is a detached commit reference.
    Detached {
        /// Hex SHA of the commit HEAD points to.
        target: String,
    },
    /// HEAD points to a branch that has no commits yet (`git init` fresh repo).
    Unborn {
        /// Short branch name from `.git/HEAD`, e.g. `"main"`.
        branch: String,
    },
}

impl Head {
    /// Human-readable one-liner for display in the UI.
    pub fn display(&self) -> String {
        match self {
            Head::Attached { branch, .. } => format!("branch: {}", branch),
            Head::Detached { target } => {
                format!("detached: {}", target.get(..8).unwrap_or(target))
            }
            Head::Unborn { branch } => format!("unborn ({})", branch),
        }
    }
}
