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
    /// blocker (`plan_replay_onto`, #344) — the range contains a merge
    /// commit; `git replay` cannot replay merges (`fatal: replaying merge
    /// commits is not supported yet!`). Use the ordinary rebase.
    ReplayRangeHasMerges { branch: String, count: usize },
    /// blocker (`plan_replay_onto`) — the branch is checked out in a
    /// worktree whose working tree is dirty; moving its ref would leave that
    /// index and tree behind (ADR-0211 §5).
    ReplayWorktreeDirty { branch: String, path: String },
    /// blocker (`plan_replay_onto`) — `git replay` reported a conflict
    /// (exit 1): the commits cannot be replayed onto `onto` without manual
    /// resolution, which replay does not offer.
    ReplayConflicts { branch: String, onto: String },
    /// blocker (`plan_replay_onto`) — `git replay` printed nothing: there is
    /// nothing to move (already on `onto`, or an empty range).
    ReplayNothingToDo { branch: String, onto: String },
    /// warning (`plan_replay_onto`) — the refs `git replay` will move, as it
    /// printed them. `sample` holds the first few `<ref> <old7>→<new7>`,
    /// `more` is how many are not shown.
    ReplayUpdates {
        count: usize,
        sample: Vec<String>,
        more: usize,
    },
    /// warning (`plan_replay_onto`) — the branch is checked out in another
    /// worktree; its HEAD follows the ref, but its index and working tree are
    /// not updated by this operation (ADR-0211 §1 iii).
    ReplayWorktreeStale { branch: String, path: String },
    /// warning (`plan_replay_onto`, #356) — `count` commits in the range are
    /// signed; replay recreates them unsigned.
    ReplayDropsSignatures { count: usize },
}

impl RebaseNote {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            RebaseNote::DetachedHead => {
                "HEAD is detached. Rebase requires an attached branch.".to_string()
            }
            RebaseNote::DirtyWorkingTree => {
                crate::advice_template_en!(RebaseDirtyWorkingTree).to_string()
            }
            RebaseNote::InvalidOnto { onto } => {
                format!("'{}' does not resolve to a branch or commit.", onto)
            }
            RebaseNote::AlreadyUpToDate { branch, onto } => format!(
                "'{}' is already up to date with '{}'. Nothing to rebase.",
                branch, onto
            ),
            RebaseNote::MayConflict => crate::advice_template_en!(RebaseMayConflict).to_string(),
            RebaseNote::ReplayRangeHasMerges { branch, count } => format!(
                "'{}' contains {} merge commit(s); git replay cannot replay merges. Use Rebase onto from that worktree instead.",
                branch, count
            ),
            RebaseNote::ReplayWorktreeDirty { branch, path } => format!(
                "'{}' is checked out in {} with uncommitted changes. Commit or stash them there first; moving the branch would leave that working tree behind.",
                branch, path
            ),
            RebaseNote::ReplayConflicts { branch, onto } => format!(
                "Replaying '{}' onto '{}' would conflict. git replay cannot resolve conflicts; rebase from the branch's worktree instead.",
                branch, onto
            ),
            RebaseNote::ReplayNothingToDo { branch, onto } => format!(
                "Nothing to replay: '{}' is already on '{}'.",
                branch, onto
            ),
            RebaseNote::ReplayUpdates { count, sample, more } => {
                let mut list = sample.join("\n  ");
                if *more > 0 {
                    list = format!("{} (+{} more)", list, more);
                }
                format!(
                    "Updates {} ref(s) exactly as git replay printed them:\n  {}",
                    count, list
                )
            }
            RebaseNote::ReplayDropsSignatures { count } => format!(
                "{} signed commit(s) will be recreated without their signatures.",
                count
            ),
            RebaseNote::ReplayWorktreeStale { branch, path } => format!(
                "'{}' is checked out in {}. Its HEAD will follow the branch, but that worktree's index and files are not updated by this operation; run `git reset --keep` there afterwards or rebase from that worktree.",
                branch, path
            ),
        }
    }
}

/// Plan titles for the rebase-current-onto op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebaseTitle {
    /// `plan_rebase_current_onto` — `Rebase '<branch>' onto '<onto>'`.
    RebaseCurrentOnto { branch: String, onto: String },
    /// `plan_replay_onto` (#344) — `Replay '<branch>' onto '<onto>'` — the
    /// branch is moved by ref update without touching any working tree.
    ReplayOnto { branch: String, onto: String },
}

impl RebaseTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            RebaseTitle::RebaseCurrentOnto { branch, onto } => {
                format!("Rebase '{}' onto '{}'", branch, onto)
            }
            RebaseTitle::ReplayOnto { branch, onto } => {
                format!("Replay '{}' onto '{}'", branch, onto)
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
    /// `plan_replay_onto` (#344) — the pre-replay tip of every ref moved;
    /// `from` is the branch's own tip (also retained under `refs/kagi/backups/`).
    ReplayOnto { branch: String, from: String },
}

impl RebaseRecovery {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            RebaseRecovery::RebaseCurrentOnto { branch, from } => format!(
                "While the rebase is in progress, abort it from the conflict banner (equivalent to `git rebase --abort`) to restore '{branch}' to {from} exactly. If it already finished, restore the pre-rebase tip with:\n  git update-ref refs/heads/{branch} {from}"
            ),
            RebaseRecovery::ReplayOnto { branch, from } => format!(
                "Replay moves refs only; nothing is checked out. Restore the pre-replay tip with:\n  git update-ref refs/heads/{branch} {from}\n(the same tip is retained under refs/kagi/backups/ until this entry is forgotten)"
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
