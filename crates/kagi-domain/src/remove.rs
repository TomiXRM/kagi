//! Window-free values for the first application-layer family (#484).
use crate::commit::CommitId;
use crate::plan::DiscardBackup;
use std::path::PathBuf;

/// Backend-resolved canonical shared Git resource (not a UI locator).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RepoId(pub PathBuf);
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorktreeId {
    pub repo: RepoId,
    pub git_dir: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RemoveStage {
    #[default]
    NotStarted,
    PreRemove {
        step: usize,
    },
    BackupCaptured,
    DeletionStarted,
    AdminPruned,
    BranchDeleted,
    Done,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RemoveVerification {
    #[default]
    NotRun,
    Verified,
    Failed(String),
    Unknown,
}

/// Owned outside the executor's unwind stack. No persistence or I/O here.
#[derive(Clone, Debug, Default)]
pub struct RemoveProgress {
    pub verification: RemoveVerification,
    pub stage: RemoveStage,
    pub backups: Vec<DiscardBackup>,
    pub branch_tip: Option<CommitId>,
    pub observations: Vec<String>,
    pub termination_unknown: bool,
    pub config_granted: bool,
    /// A pre-remove command was refused before it could begin. This is distinct
    /// from owner trust: no mutation was admitted, so its receipt is Failed.
    pub policy_rejected: bool,
}

impl RemoveProgress {
    pub fn started(&self) -> bool {
        self.stage != RemoveStage::NotStarted || self.config_granted
    }
}

/// Finite fault points; only integration tests may select one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoveFaultPoint {
    PanicBeforeMutation,
    PanicAfterDeletionStarted,
    FailAfterBackupBeforeDelete,
    FailAfterDirectoryDelete,
    PreRemoveTerminationUnknown,
    /// Test seam: move the checked-out branch after its tip was captured.
    MoveBranchBeforeDelete,
    /// Test seams for the fresh-open owner-trust boundary.
    UntrustedMain,
    UntrustedTarget,
}

/// Why a removal-safety question could not be answered.
///
/// Carried by every `Unknown`, so the UI localizes a concrete cause instead of
/// echoing a raw backend error string. The backend keeps its underlying error
/// detail separately for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorktreeUnknownReason {
    /// The worktree's HEAD is unborn, so there is no commit to reason about.
    Unborn,
    /// An upstream is configured, but its remote-tracking ref could not be
    /// resolved. Distinct from "no upstream configured", which is evidence
    /// [`WorktreeEvidence::No`], not ignorance.
    UpstreamUnavailable,
    /// A read of the repository or working tree failed (or was cancelled).
    ObservationFailed,
}

/// Three-valued evidence for one removal-safety question.
///
/// The Git backend answers each question by *observation*. A question it could
/// not answer is [`Unknown`] with the reason why — never [`No`], and never
/// silently [`Yes`]. [`No`] is a positive observation of the negative case
/// (e.g. an upstream that is genuinely not configured), so only [`Unknown`]
/// means "we do not know".
///
/// [`Unknown`]: WorktreeEvidence::Unknown
/// [`No`]: WorktreeEvidence::No
/// [`Yes`]: WorktreeEvidence::Yes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorktreeEvidence {
    Yes,
    No,
    Unknown(WorktreeUnknownReason),
}

/// Observed facts about one worktree, reduced to plain data so the advisory
/// removal verdict stays pure. Collecting the evidence is the Git backend's
/// responsibility; nothing here performs I/O or allocates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorktreeRemovalFacts {
    /// The repository's main worktree, which is never removable.
    pub is_main: bool,
    /// No staged, unstaged, or untracked changes in the working tree.
    /// Absence of a WIP record is *not* evidence of cleanliness — that is
    /// [`WorktreeEvidence::Unknown`], not [`WorktreeEvidence::Yes`].
    pub clean: WorktreeEvidence,
    /// The head commit is contained in an actual remote-tracking ref. A local
    /// `branch.<name>.remote = .` is never proof of being pushed.
    pub pushed: WorktreeEvidence,
    /// The head commit is an ancestor of the repository's default branch.
    pub merged: WorktreeEvidence,
    /// The worktree carries no administrative lock.
    pub unlocked: WorktreeEvidence,
}

/// Advisory classification of one worktree's removal safety.
///
/// Purely informational: this never changes what removal does, it only tells
/// the user what the repository currently supports claiming.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorktreeRemovalVerdict {
    /// Clean, unlocked, and its history is contained in the default branch.
    SafeMerged,
    /// Clean, unlocked, and its history exists on a remote-tracking ref.
    SafePushed,
    /// The main worktree — excluded regardless of every other fact.
    MainWorktree,
    /// Observed working-tree changes (staged, unstaged, or untracked).
    Dirty,
    /// Observed administrative lock.
    Locked,
    /// Clean and unlocked, but the history is provably nowhere else.
    Unpublished,
    /// Something needed for a positive claim could not be observed, with the
    /// reason carried over from the evidence that blocked the claim.
    Unknown(WorktreeUnknownReason),
}

/// Classify removal safety from observed facts (advisory only).
///
/// A positive verdict requires, without exception,
/// `clean == Yes && unlocked == Yes && (merged == Yes || pushed == Yes)`,
/// and the main worktree is excluded before anything else is considered.
///
/// Precedence, conservative by construction:
///
/// | Order | Condition | Verdict |
/// |---|---|---|
/// | 1 | `is_main` | `MainWorktree` |
/// | 2 | `clean == No` | `Dirty` |
/// | 3 | `unlocked == No` | `Locked` |
/// | 4 | `clean`/`unlocked` unobserved | `Unknown(reason)` |
/// | 5 | `merged == Yes` | `SafeMerged` |
/// | 6 | `pushed == Yes` | `SafePushed` |
/// | 7 | either history side unobserved | `Unknown(reason)` |
/// | 8 | both history sides `No` | `Unpublished` |
///
/// Known blockers (2, 3) are reported ahead of the generic `Unknown` so the
/// user sees the actionable reason instead of a shrug. The two history
/// questions are independent proofs of the same thing, so an unobserved side
/// can never negate a positive observation on the other (5 and 6 precede 7).
/// Each `Unknown` propagates the reason of the first evidence that blocked the
/// claim, in that same precedence order.
pub fn worktree_removal_verdict(facts: &WorktreeRemovalFacts) -> WorktreeRemovalVerdict {
    use WorktreeEvidence::{No, Unknown, Yes};

    if facts.is_main {
        return WorktreeRemovalVerdict::MainWorktree;
    }
    if facts.clean == No {
        return WorktreeRemovalVerdict::Dirty;
    }
    if facts.unlocked == No {
        return WorktreeRemovalVerdict::Locked;
    }
    if let Unknown(reason) = facts.clean {
        return WorktreeRemovalVerdict::Unknown(reason);
    }
    if let Unknown(reason) = facts.unlocked {
        return WorktreeRemovalVerdict::Unknown(reason);
    }
    if facts.merged == Yes {
        return WorktreeRemovalVerdict::SafeMerged;
    }
    if facts.pushed == Yes {
        return WorktreeRemovalVerdict::SafePushed;
    }
    if let Unknown(reason) = facts.merged {
        return WorktreeRemovalVerdict::Unknown(reason);
    }
    if let Unknown(reason) = facts.pushed {
        return WorktreeRemovalVerdict::Unknown(reason);
    }
    WorktreeRemovalVerdict::Unpublished
}

#[cfg(test)]
mod worktree_removal_tests {
    use super::WorktreeEvidence::{No, Unknown, Yes};
    use super::WorktreeRemovalVerdict as V;
    use super::WorktreeUnknownReason::{ObservationFailed, Unborn, UpstreamUnavailable};
    use super::*;

    /// Removable-looking baseline: not main, clean, unlocked, merged, pushed.
    fn safe() -> WorktreeRemovalFacts {
        WorktreeRemovalFacts {
            is_main: false,
            clean: Yes,
            pushed: Yes,
            merged: Yes,
            unlocked: Yes,
        }
    }

    /// Every distinguishable evidence value, reasons included.
    const EVIDENCE: [WorktreeEvidence; 5] = [
        Yes,
        No,
        Unknown(Unborn),
        Unknown(UpstreamUnavailable),
        Unknown(ObservationFailed),
    ];

    #[test]
    fn main_worktree_is_excluded_even_with_perfect_evidence() {
        let mut f = safe();
        f.is_main = true;
        assert_eq!(worktree_removal_verdict(&f), V::MainWorktree);
    }

    #[test]
    fn main_exclusion_outranks_every_blocker() {
        // The main worktree is never reported as Dirty/Locked/Unknown: its
        // exclusion is structural, not a condition the user can clear.
        for clean in EVIDENCE {
            for unlocked in EVIDENCE {
                let f = WorktreeRemovalFacts {
                    is_main: true,
                    clean,
                    unlocked,
                    ..safe()
                };
                assert_eq!(worktree_removal_verdict(&f), V::MainWorktree);
            }
        }
    }

    #[test]
    fn observed_working_tree_changes_block_a_merged_worktree() {
        // Staged, unstaged, and untracked changes all arrive as clean == No;
        // full history proof must not talk over them.
        let f = WorktreeRemovalFacts {
            clean: No,
            ..safe()
        };
        assert_eq!(worktree_removal_verdict(&f), V::Dirty);
    }

    #[test]
    fn lock_blocks_a_clean_merged_worktree() {
        let f = WorktreeRemovalFacts {
            unlocked: No,
            ..safe()
        };
        assert_eq!(worktree_removal_verdict(&f), V::Locked);
    }

    #[test]
    fn known_blockers_are_reported_ahead_of_unobserved_state() {
        // Dirty beats an unreadable lock, and a real lock beats unreadable
        // status — an actionable reason is better than a shrug.
        let dirty = WorktreeRemovalFacts {
            clean: No,
            unlocked: Unknown(ObservationFailed),
            ..safe()
        };
        assert_eq!(worktree_removal_verdict(&dirty), V::Dirty);
        let locked = WorktreeRemovalFacts {
            clean: Unknown(ObservationFailed),
            unlocked: No,
            ..safe()
        };
        assert_eq!(worktree_removal_verdict(&locked), V::Locked);
    }

    #[test]
    fn unobserved_status_is_never_treated_as_clean() {
        let f = WorktreeRemovalFacts {
            clean: Unknown(ObservationFailed),
            ..safe()
        };
        assert_eq!(worktree_removal_verdict(&f), V::Unknown(ObservationFailed));
    }

    #[test]
    fn unobserved_lock_state_is_never_treated_as_unlocked() {
        let f = WorktreeRemovalFacts {
            unlocked: Unknown(ObservationFailed),
            ..safe()
        };
        assert_eq!(worktree_removal_verdict(&f), V::Unknown(ObservationFailed));
    }

    #[test]
    fn merged_proof_is_preferred_to_pushed_proof() {
        let f = WorktreeRemovalFacts {
            merged: Yes,
            pushed: Yes,
            ..safe()
        };
        assert_eq!(worktree_removal_verdict(&f), V::SafeMerged);
    }

    #[test]
    fn pushed_proof_alone_is_positive() {
        let f = WorktreeRemovalFacts {
            merged: No,
            pushed: Yes,
            ..safe()
        };
        assert_eq!(worktree_removal_verdict(&f), V::SafePushed);
    }

    #[test]
    fn positive_history_proof_survives_an_unobserved_other_side() {
        // The two history questions are independent proofs of the same thing,
        // so ignorance on one side cannot cancel proof on the other.
        let merged_only = WorktreeRemovalFacts {
            merged: Yes,
            pushed: Unknown(UpstreamUnavailable),
            ..safe()
        };
        assert_eq!(worktree_removal_verdict(&merged_only), V::SafeMerged);
        let pushed_only = WorktreeRemovalFacts {
            merged: Unknown(ObservationFailed),
            pushed: Yes,
            ..safe()
        };
        assert_eq!(worktree_removal_verdict(&pushed_only), V::SafePushed);
    }

    #[test]
    fn history_nowhere_else_is_unpublished() {
        // No upstream configured and not merged is an observation, not
        // ignorance: the commits provably exist only here.
        let f = WorktreeRemovalFacts {
            merged: No,
            pushed: No,
            ..safe()
        };
        assert_eq!(worktree_removal_verdict(&f), V::Unpublished);
    }

    #[test]
    fn unobserved_history_is_not_downgraded_to_unpublished() {
        // Unpublished is a claim about the repository; when a history side
        // failed to be observed we have no claim to make, and the reason of
        // the unobserved side is what the UI gets to localize.
        let unreachable_upstream = WorktreeRemovalFacts {
            merged: No,
            pushed: Unknown(UpstreamUnavailable),
            ..safe()
        };
        assert_eq!(
            worktree_removal_verdict(&unreachable_upstream),
            V::Unknown(UpstreamUnavailable)
        );
        let unborn = WorktreeRemovalFacts {
            merged: Unknown(Unborn),
            pushed: No,
            ..safe()
        };
        assert_eq!(worktree_removal_verdict(&unborn), V::Unknown(Unborn));
    }

    #[test]
    fn unknown_reason_comes_from_the_first_blocking_evidence() {
        // Precedence also decides which reason is shown: worktree state is
        // asked before history, and merged before pushed.
        let state_first = WorktreeRemovalFacts {
            clean: Unknown(ObservationFailed),
            merged: Unknown(Unborn),
            pushed: Unknown(UpstreamUnavailable),
            ..safe()
        };
        assert_eq!(
            worktree_removal_verdict(&state_first),
            V::Unknown(ObservationFailed)
        );
        let lock_before_history = WorktreeRemovalFacts {
            unlocked: Unknown(ObservationFailed),
            merged: Unknown(Unborn),
            pushed: Unknown(UpstreamUnavailable),
            ..safe()
        };
        assert_eq!(
            worktree_removal_verdict(&lock_before_history),
            V::Unknown(ObservationFailed)
        );
        let merged_before_pushed = WorktreeRemovalFacts {
            merged: Unknown(Unborn),
            pushed: Unknown(UpstreamUnavailable),
            ..safe()
        };
        assert_eq!(
            worktree_removal_verdict(&merged_before_pushed),
            V::Unknown(Unborn)
        );
    }

    #[test]
    fn positive_verdicts_hold_exactly_where_the_formula_does() {
        // Exhaustive truth table: a verdict is positive iff the worktree is
        // not main, observed clean, observed unlocked, and has at least one
        // positive history proof. Nothing else may ever be called safe, and
        // no `Unknown` reason may ever leak into a positive verdict.
        for is_main in [false, true] {
            for clean in EVIDENCE {
                for pushed in EVIDENCE {
                    for merged in EVIDENCE {
                        for unlocked in EVIDENCE {
                            let f = WorktreeRemovalFacts {
                                is_main,
                                clean,
                                pushed,
                                merged,
                                unlocked,
                            };
                            let verdict = worktree_removal_verdict(&f);
                            let positive = matches!(verdict, V::SafeMerged | V::SafePushed);
                            let expected = !is_main
                                && clean == Yes
                                && unlocked == Yes
                                && (merged == Yes || pushed == Yes);
                            assert_eq!(positive, expected, "{f:?} -> {verdict:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_unknown_verdict_carries_a_reason_some_evidence_actually_reported() {
        // The reason is propagated, never invented: an Unknown verdict must
        // quote a reason one of the facts supplied.
        for is_main in [false, true] {
            for clean in EVIDENCE {
                for pushed in EVIDENCE {
                    for merged in EVIDENCE {
                        for unlocked in EVIDENCE {
                            let f = WorktreeRemovalFacts {
                                is_main,
                                clean,
                                pushed,
                                merged,
                                unlocked,
                            };
                            let V::Unknown(reason) = worktree_removal_verdict(&f) else {
                                continue;
                            };
                            assert!(
                                [clean, pushed, merged, unlocked].contains(&Unknown(reason)),
                                "{f:?} -> Unknown({reason:?})"
                            );
                        }
                    }
                }
            }
        }
    }
}
