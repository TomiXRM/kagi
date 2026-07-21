//! RebaseNote — rebase-current-onto (branch-menu "Integrate" group, "Rebase
//! current branch onto <target>"). A dedicated category rather than
//! overloading `CommonNote`/`PlanOp` (ADR-0129 §1: one variant space per op
//! category).

/// Plan notes for the rebase-current-onto op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebaseNote {
    /// blocker (`plan_rebase_current_onto`) — HEAD is detached; there is no
    /// current branch to rebase.
    DetachedHead,
    /// blocker (`plan_rebase_current_onto`) — the working tree has
    /// uncommitted changes. Rebase replays commits onto a new base and
    /// refuses to start with a dirty tree (mirrors checkout's Guarded class).
    DirtyWorkingTree,
    /// blocker (`plan_rebase_current_onto`) — `onto` does not resolve to a
    /// valid ref or commit.
    InvalidOnto { onto: String },
    /// blocker (`plan_rebase_current_onto`) — `onto` is the branch's own
    /// current tip; nothing to replay.
    AlreadyUpToDate { branch: String, onto: String },
    /// warning (unconditional) — rebase may produce conflicts partway
    /// through; the user resolves each in kagi's conflict editor, one commit
    /// at a time, same as merge/cherry-pick/revert conflicts.
    MayConflict,
}

impl RebaseNote {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            RebaseNote::DetachedHead => {
                "HEAD is detached. Rebase requires an attached branch.".to_string()
            }
            RebaseNote::DirtyWorkingTree => {
                "Working tree has uncommitted changes. Commit, stash, or discard them before rebasing.".to_string()
            }
            RebaseNote::InvalidOnto { onto } => {
                format!("'{}' does not resolve to a branch or commit.", onto)
            }
            RebaseNote::AlreadyUpToDate { branch, onto } => format!(
                "'{}' is already up to date with '{}'. Nothing to rebase.",
                branch, onto
            ),
            RebaseNote::MayConflict => {
                "Rebase may stop partway through with a conflict. Resolve each conflicted commit in the conflict editor, then Continue; the sequence keeps replaying until it finishes."
                    .to_string()
            }
        }
    }
}

/// Plan titles for the rebase-current-onto op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebaseTitle {
    /// `plan_rebase_current_onto` — `Rebase '<branch>' onto '<onto>'`.
    RebaseCurrentOnto { branch: String, onto: String },
}

impl RebaseTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            RebaseTitle::RebaseCurrentOnto { branch, onto } => {
                format!("Rebase '{}' onto '{}'", branch, onto)
            }
        }
    }
}

/// Recovery kinds for the rebase-current-onto op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebaseRecovery {
    /// `plan_rebase_current_onto` — the branch's pre-rebase tip, recoverable
    /// via `git rebase --abort` while in progress, or a ref reset afterward.
    RebaseCurrentOnto { branch: String, from: String },
}

impl RebaseRecovery {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            RebaseRecovery::RebaseCurrentOnto { branch, from } => format!(
                "While the rebase is in progress, abort it from the conflict banner (equivalent to `git rebase --abort`) to restore '{branch}' to {from} exactly. If it already finished, restore the pre-rebase tip with:\n  git update-ref refs/heads/{branch} {from}"
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
            RebaseNote::DetachedHead.message_en(),
            "HEAD is detached. Rebase requires an attached branch."
        );
    }

    #[test]
    fn dirty_working_tree() {
        assert_eq!(
            RebaseNote::DirtyWorkingTree.message_en(),
            "Working tree has uncommitted changes. Commit, stash, or discard them before rebasing."
        );
    }

    #[test]
    fn invalid_onto() {
        assert_eq!(
            RebaseNote::InvalidOnto {
                onto: "no-such-branch".into()
            }
            .message_en(),
            "'no-such-branch' does not resolve to a branch or commit."
        );
    }

    #[test]
    fn already_up_to_date() {
        assert_eq!(
            RebaseNote::AlreadyUpToDate {
                branch: "feat/x".into(),
                onto: "main".into()
            }
            .message_en(),
            "'feat/x' is already up to date with 'main'. Nothing to rebase."
        );
    }

    #[test]
    fn may_conflict() {
        assert_eq!(
            RebaseNote::MayConflict.message_en(),
            "Rebase may stop partway through with a conflict. Resolve each conflicted commit in the conflict editor, then Continue; the sequence keeps replaying until it finishes."
        );
    }

    #[test]
    fn rebase_title() {
        assert_eq!(
            RebaseTitle::RebaseCurrentOnto {
                branch: "feat/x".into(),
                onto: "main".into()
            }
            .message_en(),
            "Rebase 'feat/x' onto 'main'"
        );
    }

    #[test]
    fn rebase_recovery() {
        assert_eq!(
            RebaseRecovery::RebaseCurrentOnto {
                branch: "feat/x".into(),
                from: "a1b2c3d4".into()
            }
            .message_en(),
            "While the rebase is in progress, abort it from the conflict banner (equivalent to `git rebase --abort`) to restore 'feat/x' to a1b2c3d4 exactly. If it already finished, restore the pre-rebase tip with:\n  git update-ref refs/heads/feat/x a1b2c3d4"
        );
    }
}
