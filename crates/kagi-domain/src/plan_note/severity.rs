//! Presentation semantics are independent of warning-list membership and copy.

use super::{PlanNote, RebaseNote, StashNote, WorktreeNote};

/// A caution symbol is reserved for unexpected loss, not deliberate removal,
/// conflict-resolution progress, or a promise that work is preserved.
/// This does not change whether a note blocks execution or requires confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanNoteKind {
    Information,
    UnexpectedDataLoss,
}

impl PlanNote {
    pub fn kind(&self) -> PlanNoteKind {
        match self {
            // These losses are collateral to restoring work, removing a Git
            // worktree, or replaying commits, rather than the requested change.
            Self::Stash(StashNote::IndexOnlyNotRestored { .. })
            | Self::Worktree(WorktreeNote::RemoveIgnoredFiles { .. })
            | Self::Rebase(RebaseNote::ReplayDropsSignatures { .. }) => {
                PlanNoteKind::UnexpectedDataLoss
            }
            Self::Common(_)
            | Self::Discard(_)
            | Self::Branch(_)
            | Self::Stash(_)
            | Self::History(_)
            | Self::Pull(_)
            | Self::Push(_)
            | Self::Switch(_)
            | Self::Checkout(_)
            | Self::Merge(_)
            | Self::Worktree(_)
            | Self::CherryRevert(_)
            | Self::Cleanup(_)
            | Self::Conflicts(_)
            | Self::Commit(_)
            | Self::Checklist(_)
            | Self::Tag(_)
            | Self::RemoteBranch(_)
            | Self::Reset(_)
            | Self::ForceLease(_)
            | Self::Github(_)
            | Self::Rebase(_)
            | Self::Snapshot(_)
            | Self::Sync(_)
            | Self::Ruleset(_)
            | Self::Maintenance(_)
            | Self::OplogRestore(_)
            | Self::Clone(_) => PlanNoteKind::Information,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_note::*;

    #[test]
    fn display_symbols_do_not_leak_from_legacy_contract_copy() {
        for note in [
            PlanNote::Discard(DiscardNote::UntrackedWillBeDeleted { count: 2 }),
            PlanNote::Worktree(WorktreeNote::PostCreateSteps {
                steps: vec![],
                trust_required: true,
                sha256: "digest".into(),
            }),
            PlanNote::Worktree(WorktreeNote::PreRemoveSteps {
                steps: vec![],
                trust_required: true,
                sha256: "digest".into(),
            }),
        ] {
            assert_eq!(note.kind(), PlanNoteKind::Information);
            assert!(note.message_en().contains('⚠'));
            assert!(!note.message_display_en().contains('⚠'));
        }
    }

    #[test]
    fn collateral_data_loss_requires_caution() {
        for note in [
            PlanNote::Stash(StashNote::IndexOnlyNotRestored { files: vec![] }),
            PlanNote::Worktree(WorktreeNote::RemoveIgnoredFiles {
                path: "worktree".into(),
                files: 1,
                folders: 2,
            }),
            PlanNote::Rebase(RebaseNote::ReplayDropsSignatures { count: 1 }),
        ] {
            assert_eq!(note.kind(), PlanNoteKind::UnexpectedDataLoss, "{note:?}");
        }
    }

    #[test]
    fn deliberate_removal_and_preserved_work_are_not_cautions() {
        for note in [
            PlanNote::Conflicts(ConflictsNote::PartialResolutionsPreserved),
            PlanNote::Conflicts(ConflictsNote::NoConflictingFilesDetected),
            PlanNote::Conflicts(ConflictsNote::ContinuesToNextStep { remaining: 2 }),
            PlanNote::Conflicts(ConflictsNote::SkipDiscardsStep),
            PlanNote::Common(CommonNote::DirtyStashFirst),
            PlanNote::Common(CommonNote::MergeConflictWarning),
            PlanNote::History(HistoryNote::SoftMovePreservesChanges),
            PlanNote::Snapshot(SnapshotNote::SavepointFirst),
            PlanNote::Snapshot(SnapshotNote::RewritesWorkingTree),
            PlanNote::Discard(DiscardNote::UntrackedWillBeDeleted { count: 2 }),
            PlanNote::Stash(StashNote::RemoteDropIrreversible),
            PlanNote::ForceLease(ForceLeaseNote::RewritesRemoteHistory {
                branch: "unexpected loss ⚠".into(),
            }),
            PlanNote::Worktree(WorktreeNote::RemovesWorktree {
                path: "loss".into(),
                branch: None,
                delete_branch: true,
            }),
            PlanNote::Reset(ResetNote::AbandonsCommits {
                branch: "main".into(),
                count: 2,
            }),
            PlanNote::Pull(PullNote::RestoreConflict {
                paths: vec!["file".into()],
            }),
        ] {
            assert_eq!(note.kind(), PlanNoteKind::Information, "{note:?}");
        }
    }
}
