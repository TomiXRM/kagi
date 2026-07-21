//! ResetNote — reset-current-to-head (branch-menu "Advanced / Dangerous"
//! group, "Reset current to this HEAD...").
//!
//! Ref-only: moves the current branch's ref to point at a different commit,
//! exactly like `execute_undo_commit` — the index and working tree are never
//! touched (no `reset --hard`, ever; see AGENTS.md invariant #3). This is
//! semantically a `git reset --soft`, restricted to the current branch.

/// Plan notes for the reset-current-to-head op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResetNote {
    /// blocker (`plan_reset_current_to_head`) — HEAD is detached; there is no
    /// current branch to move.
    DetachedHead,
    /// blocker (`plan_reset_current_to_head`) — the target commit does not
    /// exist.
    CommitMissing { sha: String },
    /// warning (unconditional) — ref-only move; the working tree and index
    /// are left exactly as they are, so this will surface as a large diff
    /// against the new HEAD rather than lost files.
    RefOnlySoftReset,
    /// warning — moving backward past commits that become unreachable from
    /// `branch` (still recoverable via reflog until GC).
    AbandonsCommits { branch: String, count: usize },
    /// warning — `target` is not an ancestor of the branch's current tip;
    /// this reassigns the branch to unrelated history rather than "going
    /// back in time" on the same line.
    TargetNotAncestor { branch: String },
}

impl ResetNote {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            ResetNote::DetachedHead => {
                "HEAD is detached. Reset current-to-HEAD requires an attached branch.".to_string()
            }
            ResetNote::CommitMissing { sha } => {
                format!("Commit '{}' does not exist in this repository.", sha)
            }
            ResetNote::RefOnlySoftReset => {
                "This only moves the branch pointer (like `git reset --soft`): the working tree \
                 and staged changes are left exactly as they are, so this will show up as a \
                 large diff against the new HEAD, not lost files."
                    .to_string()
            }
            ResetNote::AbandonsCommits { branch, count } => format!(
                "{} commit(s) will no longer be reachable from '{}' (still recoverable via reflog until GC).",
                count, branch
            ),
            ResetNote::TargetNotAncestor { branch } => format!(
                "The target commit is not an ancestor of '{}'. This reassigns the branch to unrelated history rather than moving it back along its own line.",
                branch
            ),
        }
    }
}

/// Plan titles for the reset-current-to-head op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResetTitle {
    /// `plan_reset_current_to_head` — `Reset '<branch>' to <sha>`.
    ResetCurrentToHead { branch: String, to: String },
}

impl ResetTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            ResetTitle::ResetCurrentToHead { branch, to } => {
                format!("Reset '{}' to {}", branch, to)
            }
        }
    }
}

/// Recovery kinds for the reset-current-to-head op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResetRecovery {
    /// `plan_reset_current_to_head` — move the branch ref back to its
    /// pre-reset tip.
    ResetCurrentToHead { branch: String, from: String },
}

impl ResetRecovery {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            ResetRecovery::ResetCurrentToHead { branch, from } => format!(
                "To undo, move the branch back to its previous tip:\n  git update-ref refs/heads/{} {}\n(Ref-only — the working tree and index are untouched either way.)",
                branch, from
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detached_head() {
        assert_eq!(
            ResetNote::DetachedHead.message_en(),
            "HEAD is detached. Reset current-to-HEAD requires an attached branch."
        );
    }

    #[test]
    fn commit_missing() {
        assert_eq!(
            ResetNote::CommitMissing {
                sha: "a1b2c3d4".into()
            }
            .message_en(),
            "Commit 'a1b2c3d4' does not exist in this repository."
        );
    }

    #[test]
    fn abandons_commits() {
        assert_eq!(
            ResetNote::AbandonsCommits {
                branch: "main".into(),
                count: 3
            }
            .message_en(),
            "3 commit(s) will no longer be reachable from 'main' (still recoverable via reflog until GC)."
        );
    }

    #[test]
    fn target_not_ancestor() {
        assert_eq!(
            ResetNote::TargetNotAncestor {
                branch: "main".into()
            }
            .message_en(),
            "The target commit is not an ancestor of 'main'. This reassigns the branch to unrelated history rather than moving it back along its own line."
        );
    }

    #[test]
    fn reset_title() {
        assert_eq!(
            ResetTitle::ResetCurrentToHead {
                branch: "main".into(),
                to: "a1b2c3d4".into()
            }
            .message_en(),
            "Reset 'main' to a1b2c3d4"
        );
    }

    #[test]
    fn reset_recovery() {
        assert_eq!(
            ResetRecovery::ResetCurrentToHead {
                branch: "main".into(),
                from: "e5f6a7b8".into()
            }
            .message_en(),
            "To undo, move the branch back to its previous tip:\n  git update-ref refs/heads/main e5f6a7b8\n(Ref-only — the working tree and index are untouched either way.)"
        );
    }
}
