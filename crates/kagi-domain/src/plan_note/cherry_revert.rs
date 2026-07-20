//! CherryRevertNote / CherryRevertTitle / CherryRevertRecovery — ADR-0129
//! appendix §B-3 (+ §A, §C, §D).
//!
//! Covers `crates/kagi-git/src/ops/cherry_revert.rs`'s two plan producers:
//! `plan_cherry_pick` and `plan_revert`. Cross-op notes (HEAD detached/unborn,
//! conflicted-repo, dirty-working-tree, untracked-remain) are NOT duplicated
//! here — they map to the existing [`crate::plan_note::CommonNote`] variants
//! (appendix §A) from the ops file directly. [`PlanOp::CherryPick`] /
//! [`PlanOp::Revert`] (already defined for the HEAD-state notes) double as the
//! op discriminant for the templates this file shares between the two ops.

use super::{DirtyParts, PlanOp};

/// Plan notes for the cherry_revert op family (`plan_cherry_pick` /
/// `plan_revert`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CherryRevertNote {
    /// blocker — `id` is a merge commit; mainline selection is not supported
    /// in MVP. `op` selects the cherry-pick/revert wording.
    MergeCommitNeedsMainline {
        sha: String,
        parents: usize,
        op: PlanOp,
    },
    /// blocker (no-op family; `plan_cherry_pick` only) — `id` is already the
    /// current HEAD commit, so there is nothing to cherry-pick.
    NothingToCherryPickHead { sha: String },
    /// blocker — the in-memory cherry-pick/revert predicts conflicts. `op`
    /// selects the cherry-pick/revert wording.
    WouldConflict {
        count: usize,
        files: Vec<String>,
        op: PlanOp,
    },
    /// blocker (no-op family) — the cherry-pick/revert would produce no
    /// changes. `op` selects the wording — cherry-pick's sentence has an
    /// extra "already applied" tail that revert's does not.
    NoChanges { sha: String, op: PlanOp },
    /// blocker (`plan_revert` only) — `id` is not reachable from the current
    /// branch; revert only operates on current-branch commits.
    NotInCurrentBranch { sha: String },
    /// warning (`plan_revert` only) — dirty working tree may cause safe
    /// checkout to refuse the revert.
    DirtyMayRefuse { parts: DirtyParts },
}

impl CherryRevertNote {
    /// Byte-identical to the legacy `ops/cherry_revert.rs` strings
    /// (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            CherryRevertNote::MergeCommitNeedsMainline { sha, parents, op } => match op {
                PlanOp::CherryPick => format!(
                    "Commit {} is a merge commit ({} parents). Cherry-picking merge commits \
                     requires explicit mainline selection, which is not supported in MVP.",
                    sha, parents
                ),
                PlanOp::Revert => format!(
                    "Commit {} is a merge commit ({} parents). Reverting merge commits requires \
                     explicit mainline selection, which is not supported in MVP.",
                    sha, parents
                ),
                _ => unreachable!(
                    "CherryRevertNote::MergeCommitNeedsMainline only uses CherryPick/Revert"
                ),
            },
            CherryRevertNote::NothingToCherryPickHead { sha } => format!(
                "Commit {} is the current HEAD commit. Nothing to cherry-pick.",
                sha
            ),
            CherryRevertNote::WouldConflict { count, files, op } => {
                let joined = files.join(", ");
                match op {
                    PlanOp::CherryPick => format!(
                        "Cherry-pick would produce {} conflict(s): {}. Resolve divergence before \
                         cherry-picking.",
                        count, joined
                    ),
                    PlanOp::Revert => format!(
                        "Revert would produce {} conflict(s): {}. Resolve divergence before \
                         reverting.",
                        count, joined
                    ),
                    _ => {
                        unreachable!("CherryRevertNote::WouldConflict only uses CherryPick/Revert")
                    }
                }
            }
            CherryRevertNote::NoChanges { sha, op } => match op {
                PlanOp::CherryPick => format!(
                    "Cherry-picking {} would produce no changes — it appears to have been \
                     applied already.",
                    sha
                ),
                PlanOp::Revert => format!("Reverting {} would produce no changes.", sha),
                _ => unreachable!("CherryRevertNote::NoChanges only uses CherryPick/Revert"),
            },
            CherryRevertNote::NotInCurrentBranch { sha } => format!(
                "Commit {} is not contained in the current branch. Revert only operates on \
                 current-branch commits.",
                sha
            ),
            CherryRevertNote::DirtyMayRefuse { parts } => format!(
                "Working tree has {}. Safe checkout may refuse if those files overlap the revert.",
                parts.parts_en()
            ),
        }
    }
}

/// Plan titles for the cherry_revert op family (appendix §C `cherry-pick` /
/// `revert` rows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CherryRevertTitle {
    /// `plan_cherry_pick`: `Cherry-pick {sha} onto {branch}` (no commit
    /// summary yet — used at every early-return site, before/without the
    /// commit summary line being computed) / `Cherry-pick {sha} '{summary}'
    /// onto {branch}` (the final, non-blocked plan).
    CherryPick {
        sha: String,
        summary: Option<String>,
        branch: String,
    },
    /// `plan_revert`: `Revert {sha} '{summary}' on {branch}` — used
    /// identically at every blocked and non-blocked return site (the summary
    /// line is always available by the time the title is built).
    Revert {
        sha: String,
        summary: String,
        branch: String,
    },
}

impl CherryRevertTitle {
    /// Byte-identical to the legacy `ops/cherry_revert.rs` title strings.
    pub fn message_en(&self) -> String {
        match self {
            CherryRevertTitle::CherryPick {
                sha,
                summary: Some(summary),
                branch,
            } => format!("Cherry-pick {} '{}' onto {}", sha, summary, branch),
            CherryRevertTitle::CherryPick {
                sha,
                summary: None,
                branch,
            } => format!("Cherry-pick {} onto {}", sha, branch),
            CherryRevertTitle::Revert {
                sha,
                summary,
                branch,
            } => format!("Revert {} '{}' on {}", sha, summary, branch),
        }
    }
}

/// Recovery kinds for the cherry_revert op family (appendix §D `cherry-pick`
/// / `revert` rows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CherryRevertRecovery {
    /// `plan_cherry_pick`'s sole recovery template — identical text at every
    /// return site (early blocker, predicted conflict, no-changes, success).
    AfterCherryPick,
    /// `plan_revert`'s sole recovery template — identical text at every
    /// return site (blocked and non-blocked).
    AfterRevert,
}

impl CherryRevertRecovery {
    /// Byte-identical to the legacy `ops/cherry_revert.rs` recovery strings.
    pub fn message_en(&self) -> String {
        match self {
            CherryRevertRecovery::AfterCherryPick => {
                "To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\n\
                 The previous HEAD sha is recorded in the reflog:\n  git reflog"
                    .to_string()
            }
            CherryRevertRecovery::AfterRevert => {
                "To undo this revert after execution, revert the new revert commit:\n  git \
                 revert <new-revert-commit-sha>\nThe previous HEAD sha is recorded in the \
                 reflog:\n  git reflog"
                    .to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // message_en golden tests (ADR-0129 §3) — byte-exact vs the legacy
    // `ops/cherry_revert.rs` producer strings (appendix §B-3 / §C / §D).

    #[test]
    fn merge_commit_needs_mainline_both_ops() {
        assert_eq!(
            CherryRevertNote::MergeCommitNeedsMainline {
                sha: "abc1234".into(),
                parents: 2,
                op: PlanOp::CherryPick,
            }
            .message_en(),
            "Commit abc1234 is a merge commit (2 parents). Cherry-picking merge commits \
             requires explicit mainline selection, which is not supported in MVP."
        );
        assert_eq!(
            CherryRevertNote::MergeCommitNeedsMainline {
                sha: "abc1234".into(),
                parents: 3,
                op: PlanOp::Revert,
            }
            .message_en(),
            "Commit abc1234 is a merge commit (3 parents). Reverting merge commits requires \
             explicit mainline selection, which is not supported in MVP."
        );
    }

    #[test]
    fn nothing_to_cherry_pick_head() {
        assert_eq!(
            CherryRevertNote::NothingToCherryPickHead {
                sha: "de4d10e".into()
            }
            .message_en(),
            "Commit de4d10e is the current HEAD commit. Nothing to cherry-pick."
        );
    }

    #[test]
    fn would_conflict_both_ops() {
        assert_eq!(
            CherryRevertNote::WouldConflict {
                count: 2,
                files: vec!["src/a b.rs".to_string(), "src/c.rs".to_string()],
                op: PlanOp::CherryPick,
            }
            .message_en(),
            "Cherry-pick would produce 2 conflict(s): src/a b.rs, src/c.rs. Resolve divergence \
             before cherry-picking."
        );
        assert_eq!(
            CherryRevertNote::WouldConflict {
                count: 1,
                files: vec!["README.md".to_string()],
                op: PlanOp::Revert,
            }
            .message_en(),
            "Revert would produce 1 conflict(s): README.md. Resolve divergence before reverting."
        );
    }

    #[test]
    fn no_changes_both_ops_have_different_tails() {
        assert_eq!(
            CherryRevertNote::NoChanges {
                sha: "abc1234".into(),
                op: PlanOp::CherryPick,
            }
            .message_en(),
            "Cherry-picking abc1234 would produce no changes — it appears to have been applied \
             already."
        );
        assert_eq!(
            CherryRevertNote::NoChanges {
                sha: "abc1234".into(),
                op: PlanOp::Revert,
            }
            .message_en(),
            "Reverting abc1234 would produce no changes."
        );
    }

    #[test]
    fn not_in_current_branch() {
        assert_eq!(
            CherryRevertNote::NotInCurrentBranch {
                sha: "abc1234".into()
            }
            .message_en(),
            "Commit abc1234 is not contained in the current branch. Revert only operates on \
             current-branch commits."
        );
    }

    #[test]
    fn dirty_may_refuse() {
        assert_eq!(
            CherryRevertNote::DirtyMayRefuse {
                parts: DirtyParts {
                    staged: 2,
                    modified: 1
                }
            }
            .message_en(),
            "Working tree has 2 staged, 1 modified. Safe checkout may refuse if those files \
             overlap the revert."
        );
        assert_eq!(
            CherryRevertNote::DirtyMayRefuse {
                parts: DirtyParts {
                    staged: 0,
                    modified: 3
                }
            }
            .message_en(),
            "Working tree has 3 modified. Safe checkout may refuse if those files overlap the \
             revert."
        );
    }

    #[test]
    fn cherry_pick_title_both_forms() {
        assert_eq!(
            CherryRevertTitle::CherryPick {
                sha: "abc1234".into(),
                summary: None,
                branch: "main".into(),
            }
            .message_en(),
            "Cherry-pick abc1234 onto main"
        );
        assert_eq!(
            CherryRevertTitle::CherryPick {
                sha: "abc1234".into(),
                summary: Some("fix the thing".into()),
                branch: "main".into(),
            }
            .message_en(),
            "Cherry-pick abc1234 'fix the thing' onto main"
        );
    }

    #[test]
    fn revert_title() {
        assert_eq!(
            CherryRevertTitle::Revert {
                sha: "abc1234".into(),
                summary: "fix the thing".into(),
                branch: "main".into(),
            }
            .message_en(),
            "Revert abc1234 'fix the thing' on main"
        );
    }

    #[test]
    fn recovery_after_cherry_pick() {
        assert_eq!(
            CherryRevertRecovery::AfterCherryPick.message_en(),
            "To undo a cherry-pick after execution, use:\n  git revert <new-commit-sha>\nThe \
             previous HEAD sha is recorded in the reflog:\n  git reflog"
        );
    }

    #[test]
    fn recovery_after_revert() {
        assert_eq!(
            CherryRevertRecovery::AfterRevert.message_en(),
            "To undo this revert after execution, revert the new revert commit:\n  git revert \
             <new-revert-commit-sha>\nThe previous HEAD sha is recorded in the reflog:\n  git \
             reflog"
        );
    }
}
