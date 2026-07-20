//! HistoryNote — ADR-0129 appendix §B-1 (undo / amend / undo·redo history-move).
//!
//! Covers `crates/kagi-git/src/ops/history.rs`'s `plan_undo_commit`,
//! `plan_amend`, and the shared `plan_history_move` (backing `plan_undo` /
//! `plan_redo`). Cross-op templates (conflicted / HEAD detached / HEAD
//! unborn) live in [`super::common::CommonNote`] (§A) — this module only
//! carries the history-specific templates.
//!
//! `plan_undo_commit` and `plan_amend` share several sentence *shapes* with
//! only the op word differing ("Undo"/"Amend", "undoing a commit"/"amending").
//! Where the shape matches an existing `CommonNote` (HEAD detached/unborn,
//! conflicted files) this module defers to it (`common::PlanOp::Undo` /
//! `Amend`). Where the sentence itself is history-specific (merge-commit,
//! root-commit, pushed-history-rewrite), this module carries one
//! [`HistoryOp`]-discriminated variant per template instead of duplicating
//! near-identical variants for undo and amend.

use crate::plan::AmendMode;

/// Discriminates the op word in the history-specific sentence shapes shared
/// by undo-commit and amend (§B-1). A dedicated two-value enum rather than
/// reusing `common::PlanOp`: these templates only ever fire for undo/amend,
/// and `PlanOp` also carries cherry-pick/revert/pull/push/merge, which have
/// no sentence here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryOp {
    Undo,
    Amend,
}

/// Plan notes for the history op family (undo-commit / amend / undo·redo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryNote {
    /// blocker — HEAD is a merge commit; op-specific "not supported" tail.
    MergeCommitUnsupported {
        sha: String,
        parents: usize,
        op: HistoryOp,
    },
    /// blocker — HEAD is the root commit; op-specific tail.
    RootCommit { sha: String, op: HistoryOp },
    /// blocker — HEAD has been pushed to its upstream; op-specific tail.
    PushedHistoryRewrite { sha: String, op: HistoryOp },
    /// blocker (amend) — the new commit message is empty.
    EmptyMessage,
    /// blocker (amend) — nothing staged to fold into the commit.
    NothingStagedForAmend,
    /// blocker (undo/redo) — a different branch than the op's is checked out.
    WrongBranch {
        branch: String,
        current: String,
        /// Already lowercased ("undo"/"redo").
        label: String,
    },
    /// blocker (undo/redo, §A16) — HEAD is detached, so there is no branch to
    /// move. Constructed like a `common` template (op-generic shape) but the
    /// wording ("… requires the operation's branch to be checked out.") is
    /// history-specific, so it lives here rather than in `CommonNote`.
    HeadNotOnBranch {
        /// "Undo" / "Redo" (capitalized, as written by the caller).
        label: String,
    },
    /// blocker (undo/redo) — the branch has moved since the entry was recorded.
    EntryStaleBranchMoved {
        branch: String,
        now: String,
        expected: String,
    },
    /// blocker (undo/redo) — the branch has no target commit.
    BranchNoTarget { branch: String },
    /// blocker (undo/redo) — the branch no longer exists.
    BranchGone { branch: String },
    /// blocker (undo/redo) — the target commit is unreachable in the ODB.
    EntryStaleUnreachable { sha: String },
    /// warning (undo/redo) — the working tree is dirty; changes are preserved
    /// verbatim by the soft ref move.
    SoftMovePreservesChanges,
}

impl HistoryNote {
    /// Byte-identical to the legacy `ops/history.rs` strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            HistoryNote::MergeCommitUnsupported { sha, parents, op } => match op {
                HistoryOp::Undo => format!(
                    "Commit {} is a merge commit ({} parents). Undoing merge commits is not supported in MVP.",
                    sha, parents
                ),
                HistoryOp::Amend => format!(
                    "Commit {} is a merge commit ({} parents). Amending merge commits is not supported.",
                    sha, parents
                ),
            },
            HistoryNote::RootCommit { sha, op } => match op {
                HistoryOp::Undo => format!(
                    "Commit {} is the root commit (no parent). There is nothing to go back to.",
                    sha
                ),
                HistoryOp::Amend => format!(
                    "Commit {} is the root commit (no parent). Amending the root commit is not supported in MVP.",
                    sha
                ),
            },
            HistoryNote::PushedHistoryRewrite { sha, op } => match op {
                HistoryOp::Undo => format!(
                    "Commit {} has been pushed to the upstream tracking branch. Undoing a pushed commit would rewrite published history, which is not allowed. Use `git revert` to create an inverse commit instead.",
                    sha
                ),
                HistoryOp::Amend => format!(
                    "Commit {} has been pushed to its upstream tracking branch. Amending published history is not allowed (ADR-0040). Create a new commit to make the correction instead.",
                    sha
                ),
            },
            HistoryNote::EmptyMessage => "Commit message must not be empty.".to_string(),
            HistoryNote::NothingStagedForAmend => {
                "Nothing staged to fold into the commit. Stage changes first, or use \
                 message-only amend."
                    .to_string()
            }
            HistoryNote::WrongBranch {
                branch,
                current,
                label,
            } => format!(
                "Operation was on branch '{}', but the current branch is '{}'. Switch back to '{}' to {} it.",
                branch, current, branch, label
            ),
            HistoryNote::HeadNotOnBranch { label } => format!(
                "HEAD is not on a branch. {} requires the operation's branch to be checked out.",
                label
            ),
            HistoryNote::EntryStaleBranchMoved {
                branch,
                now,
                expected,
            } => format!(
                "Branch '{}' has moved since this operation (now at {}, expected {}). \
                 This history entry is stale and will be skipped.",
                branch, now, expected
            ),
            HistoryNote::BranchNoTarget { branch } => {
                format!("Branch '{}' has no target commit.", branch)
            }
            HistoryNote::BranchGone { branch } => format!("Branch '{}' no longer exists.", branch),
            HistoryNote::EntryStaleUnreachable { sha } => format!(
                "Target commit {} is no longer reachable in the object store. \
                 This history entry is stale and will be skipped.",
                sha
            ),
            HistoryNote::SoftMovePreservesChanges => {
                "You have uncommitted changes. They will be preserved verbatim; \
                 only the branch ref moves (soft reset — index and working tree untouched)."
                    .to_string()
            }
        }
    }
}

/// Plan titles for the history op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryTitle {
    /// Undo last commit: normal or blocked form.
    UndoCommit {
        sha: String,
        summary: String,
        blocked: bool,
    },
    /// Amend last commit: normal or blocked form; carries the fold-mode label.
    Amend {
        sha: String,
        summary: String,
        mode: AmendMode,
        blocked: bool,
    },
    /// Undo/redo ref move: `{label} {kind_slug} on '{branch}' — {from} → {to}`.
    HistoryMove {
        label: String,
        kind_slug: String,
        branch: String,
        from: String,
        to: String,
    },
}

impl HistoryTitle {
    /// Byte-identical to the legacy `ops/history.rs` title strings.
    pub fn message_en(&self) -> String {
        match self {
            HistoryTitle::UndoCommit {
                sha,
                summary,
                blocked,
            } => {
                if *blocked {
                    "Undo last commit (cannot proceed — see blockers)".to_string()
                } else {
                    format!("Undo commit {} '{}' — changes will be staged", sha, summary)
                }
            }
            HistoryTitle::Amend {
                sha,
                summary,
                mode,
                blocked,
            } => {
                if *blocked {
                    "Amend last commit (cannot proceed — see blockers)".to_string()
                } else {
                    let mode_label = match mode {
                        AmendMode::MessageOnly => "message only",
                        AmendMode::Staged => "fold staged",
                        AmendMode::Both => "fold staged + message",
                    };
                    format!(
                        "Amend commit {} '{}' ({}) — SHA will change",
                        sha, summary, mode_label
                    )
                }
            }
            HistoryTitle::HistoryMove {
                label,
                kind_slug,
                branch,
                from,
                to,
            } => format!(
                "{} {} on '{}' — {} → {}",
                label, kind_slug, branch, from, to
            ),
        }
    }
}

/// Recovery kinds for the history op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryRecovery {
    /// undo-commit: normal or blocked form.
    Undo { sha: String, blocked: bool },
    /// amend: normal or blocked form.
    Amend { sha: String, blocked: bool },
    /// undo/redo: the safe ref-move explanation.
    HistoryMove {
        label: String,
        branch: String,
        from_short: String,
        to_short: String,
        kind_slug: String,
        /// The full (untruncated) `from` SHA used in `git update-ref`.
        from_full: String,
    },
}

impl HistoryRecovery {
    /// Byte-identical to the legacy `ops/history.rs` recovery strings.
    pub fn message_en(&self) -> String {
        match self {
            HistoryRecovery::Undo { blocked: true, .. } => {
                "Undo commit cannot proceed (see blockers above).".to_string()
            }
            HistoryRecovery::Undo { sha, blocked: false } => format!(
                "The undone commit is NOT deleted — it remains in the object store and reflog.\n\
                 To fully restore (re-commit with the same SHA):\n  git reset --soft {}\n\
                 Changes from the undone commit will be staged immediately after undo.\n\
                 The reflog records every HEAD movement:\n  git reflog",
                sha
            ),
            HistoryRecovery::Amend { blocked: true, .. } => {
                "Amend cannot proceed (see blockers above).".to_string()
            }
            HistoryRecovery::Amend { sha, blocked: false } => format!(
                "Amend rewrites history: the new commit gets a NEW SHA and the old commit \
                 {} becomes unreachable from the branch (but stays in the reflog).\n\
                 To restore the original commit:\n  git reset --hard {}\n\
                 The reflog records every HEAD movement:\n  git reflog",
                sha, sha
            ),
            HistoryRecovery::HistoryMove {
                label,
                branch,
                from_short,
                to_short,
                kind_slug,
                from_full,
            } => format!(
                "{} moves branch '{}' from {} to {} via a safe ref move (no reset --hard, no clean). \
                 The {} commit is NOT deleted — it stays in the object store and reflog:\n  git reflog\n\
                 To restore manually:\n  git update-ref refs/heads/{} {}",
                label, branch, from_short, to_short, kind_slug, branch, from_full
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // message_en golden tests (ADR-0129 §3) — byte-exact vs the legacy
    // `ops/history.rs` producer strings (appendix §B-1 / §C / §D).

    #[test]
    fn merge_commit_unsupported_undo_and_amend() {
        assert_eq!(
            HistoryNote::MergeCommitUnsupported {
                sha: "a1b2c3d4".into(),
                parents: 2,
                op: HistoryOp::Undo
            }
            .message_en(),
            "Commit a1b2c3d4 is a merge commit (2 parents). Undoing merge commits is not supported in MVP."
        );
        assert_eq!(
            HistoryNote::MergeCommitUnsupported {
                sha: "a1b2c3d4".into(),
                parents: 3,
                op: HistoryOp::Amend
            }
            .message_en(),
            "Commit a1b2c3d4 is a merge commit (3 parents). Amending merge commits is not supported."
        );
    }

    #[test]
    fn root_commit_undo_and_amend() {
        assert_eq!(
            HistoryNote::RootCommit {
                sha: "a1b2c3d4".into(),
                op: HistoryOp::Undo
            }
            .message_en(),
            "Commit a1b2c3d4 is the root commit (no parent). There is nothing to go back to."
        );
        assert_eq!(
            HistoryNote::RootCommit {
                sha: "a1b2c3d4".into(),
                op: HistoryOp::Amend
            }
            .message_en(),
            "Commit a1b2c3d4 is the root commit (no parent). Amending the root commit is not supported in MVP."
        );
    }

    #[test]
    fn pushed_history_rewrite_undo_and_amend() {
        assert_eq!(
            HistoryNote::PushedHistoryRewrite {
                sha: "a1b2c3d4".into(),
                op: HistoryOp::Undo
            }
            .message_en(),
            "Commit a1b2c3d4 has been pushed to the upstream tracking branch. Undoing a pushed commit would rewrite published history, which is not allowed. Use `git revert` to create an inverse commit instead."
        );
        assert_eq!(
            HistoryNote::PushedHistoryRewrite {
                sha: "a1b2c3d4".into(),
                op: HistoryOp::Amend
            }
            .message_en(),
            "Commit a1b2c3d4 has been pushed to its upstream tracking branch. Amending published history is not allowed (ADR-0040). Create a new commit to make the correction instead."
        );
    }

    #[test]
    fn empty_message() {
        assert_eq!(
            HistoryNote::EmptyMessage.message_en(),
            "Commit message must not be empty."
        );
    }

    #[test]
    fn nothing_staged_for_amend() {
        assert_eq!(
            HistoryNote::NothingStagedForAmend.message_en(),
            "Nothing staged to fold into the commit. Stage changes first, or use message-only amend."
        );
    }

    #[test]
    fn wrong_branch() {
        assert_eq!(
            HistoryNote::WrongBranch {
                branch: "feat/x".into(),
                current: "main".into(),
                label: "undo".into()
            }
            .message_en(),
            "Operation was on branch 'feat/x', but the current branch is 'main'. Switch back to 'feat/x' to undo it."
        );
        assert_eq!(
            HistoryNote::WrongBranch {
                branch: "feat/x".into(),
                current: "main".into(),
                label: "redo".into()
            }
            .message_en(),
            "Operation was on branch 'feat/x', but the current branch is 'main'. Switch back to 'feat/x' to redo it."
        );
    }

    #[test]
    fn head_not_on_branch() {
        assert_eq!(
            HistoryNote::HeadNotOnBranch {
                label: "Undo".into()
            }
            .message_en(),
            "HEAD is not on a branch. Undo requires the operation's branch to be checked out."
        );
        assert_eq!(
            HistoryNote::HeadNotOnBranch {
                label: "Redo".into()
            }
            .message_en(),
            "HEAD is not on a branch. Redo requires the operation's branch to be checked out."
        );
    }

    #[test]
    fn entry_stale_branch_moved() {
        assert_eq!(
            HistoryNote::EntryStaleBranchMoved {
                branch: "feat/x".into(),
                now: "aaaaaaaa".into(),
                expected: "bbbbbbbb".into()
            }
            .message_en(),
            "Branch 'feat/x' has moved since this operation (now at aaaaaaaa, expected bbbbbbbb). This history entry is stale and will be skipped."
        );
    }

    #[test]
    fn branch_no_target() {
        assert_eq!(
            HistoryNote::BranchNoTarget {
                branch: "feat/x".into()
            }
            .message_en(),
            "Branch 'feat/x' has no target commit."
        );
    }

    #[test]
    fn branch_gone() {
        assert_eq!(
            HistoryNote::BranchGone {
                branch: "feat/x".into()
            }
            .message_en(),
            "Branch 'feat/x' no longer exists."
        );
    }

    #[test]
    fn entry_stale_unreachable() {
        assert_eq!(
            HistoryNote::EntryStaleUnreachable {
                sha: "cccccccc".into()
            }
            .message_en(),
            "Target commit cccccccc is no longer reachable in the object store. This history entry is stale and will be skipped."
        );
    }

    #[test]
    fn soft_move_preserves_changes() {
        assert_eq!(
            HistoryNote::SoftMovePreservesChanges.message_en(),
            "You have uncommitted changes. They will be preserved verbatim; only the branch ref moves (soft reset — index and working tree untouched)."
        );
    }

    #[test]
    fn title_undo_commit() {
        assert_eq!(
            HistoryTitle::UndoCommit {
                sha: "a1b2c3d4".into(),
                summary: "fix: quote's \"tricky\" bit".into(),
                blocked: false
            }
            .message_en(),
            "Undo commit a1b2c3d4 'fix: quote's \"tricky\" bit' — changes will be staged"
        );
        assert_eq!(
            HistoryTitle::UndoCommit {
                sha: String::new(),
                summary: String::new(),
                blocked: true
            }
            .message_en(),
            "Undo last commit (cannot proceed — see blockers)"
        );
    }

    #[test]
    fn title_amend_all_modes() {
        assert_eq!(
            HistoryTitle::Amend {
                sha: "a1b2c3d4".into(),
                summary: "fix: typo".into(),
                mode: AmendMode::MessageOnly,
                blocked: false
            }
            .message_en(),
            "Amend commit a1b2c3d4 'fix: typo' (message only) — SHA will change"
        );
        assert_eq!(
            HistoryTitle::Amend {
                sha: "a1b2c3d4".into(),
                summary: "fix: typo".into(),
                mode: AmendMode::Staged,
                blocked: false
            }
            .message_en(),
            "Amend commit a1b2c3d4 'fix: typo' (fold staged) — SHA will change"
        );
        assert_eq!(
            HistoryTitle::Amend {
                sha: "a1b2c3d4".into(),
                summary: "fix: typo".into(),
                mode: AmendMode::Both,
                blocked: false
            }
            .message_en(),
            "Amend commit a1b2c3d4 'fix: typo' (fold staged + message) — SHA will change"
        );
        assert_eq!(
            HistoryTitle::Amend {
                sha: String::new(),
                summary: String::new(),
                mode: AmendMode::MessageOnly,
                blocked: true
            }
            .message_en(),
            "Amend last commit (cannot proceed — see blockers)"
        );
    }

    #[test]
    fn title_history_move() {
        assert_eq!(
            HistoryTitle::HistoryMove {
                label: "Undo".into(),
                kind_slug: "commit".into(),
                branch: "feat/x".into(),
                from: "aaaaaaaa".into(),
                to: "bbbbbbbb".into()
            }
            .message_en(),
            "Undo commit on 'feat/x' — aaaaaaaa → bbbbbbbb"
        );
    }

    #[test]
    fn recovery_undo() {
        assert_eq!(
            HistoryRecovery::Undo {
                sha: "a1b2c3d4".into(),
                blocked: false
            }
            .message_en(),
            "The undone commit is NOT deleted — it remains in the object store and reflog.\nTo fully restore (re-commit with the same SHA):\n  git reset --soft a1b2c3d4\nChanges from the undone commit will be staged immediately after undo.\nThe reflog records every HEAD movement:\n  git reflog"
        );
        assert_eq!(
            HistoryRecovery::Undo {
                sha: String::new(),
                blocked: true
            }
            .message_en(),
            "Undo commit cannot proceed (see blockers above)."
        );
    }

    #[test]
    fn recovery_amend() {
        assert_eq!(
            HistoryRecovery::Amend {
                sha: "a1b2c3d4".into(),
                blocked: false
            }
            .message_en(),
            "Amend rewrites history: the new commit gets a NEW SHA and the old commit a1b2c3d4 becomes unreachable from the branch (but stays in the reflog).\nTo restore the original commit:\n  git reset --hard a1b2c3d4\nThe reflog records every HEAD movement:\n  git reflog"
        );
        assert_eq!(
            HistoryRecovery::Amend {
                sha: String::new(),
                blocked: true
            }
            .message_en(),
            "Amend cannot proceed (see blockers above)."
        );
    }

    #[test]
    fn recovery_history_move() {
        assert_eq!(
            HistoryRecovery::HistoryMove {
                label: "Undo".into(),
                branch: "feat/x".into(),
                from_short: "aaaaaaaa".into(),
                to_short: "bbbbbbbb".into(),
                kind_slug: "commit".into(),
                from_full: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()
            }
            .message_en(),
            "Undo moves branch 'feat/x' from aaaaaaaa to bbbbbbbb via a safe ref move (no reset --hard, no clean). The commit commit is NOT deleted — it stays in the object store and reflog:\n  git reflog\nTo restore manually:\n  git update-ref refs/heads/feat/x aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
    }
}
