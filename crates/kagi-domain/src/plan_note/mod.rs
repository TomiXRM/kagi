//! Structured plan text (ADR-0129) — `PlanTitle` / `PlanNote` / `PlanRecovery`
//! / `PlanDisposition`.
//!
//! The ops layer produces these structured values instead of English prose;
//! the display layer localizes them. The migration ran in three phases
//! (ADR-0129 §4): Phase 1 introduced the types behind a `Verbatim` escape
//! hatch, Phase 2 fanned each op category out to a typed producer, and
//! Phase 3 deleted `Verbatim` and every String-conversion shim — the
//! compiling workspace is the completeness proof, not a grep.
//!
//! [`PlanNote::message_en`] is the **single English renderer** and the
//! oplog/klog boundary format, byte-identical to the pre-migration strings
//! (golden-tested in this module and in each category's own file).
//!
//! One file per category (ADR-0129 Phase 2 foundation): each op's producer
//! fills its own `plan_note/<category>.rs` enum and never touches this
//! dispatch file.

pub mod branch;
pub mod checklist;
pub mod checkout;
pub mod cherry_revert;
pub mod cleanup;
pub mod commit;
pub mod common;
pub mod conflicts;
pub mod discard;
pub mod force_lease;
pub mod github;
pub mod history;
pub mod merge;
pub mod pull;
pub mod push;
pub mod rebase;
pub mod remote_branch;
pub mod reset;
pub mod snapshot;
pub mod stash;
pub mod switch;
pub mod tag;
pub mod worktree;

pub use branch::{BranchNote, BranchRecovery, BranchTitle};
pub use checklist::ChecklistNote;
pub use checkout::{CheckoutNote, CheckoutRecovery, CheckoutTitle};
pub use cherry_revert::{CherryRevertNote, CherryRevertRecovery, CherryRevertTitle};
pub use cleanup::{CleanupNote, CleanupRecovery, CleanupTitle};
pub use commit::{CommitNote, CommitRecovery, CommitTitle};
pub use common::{CommonNote, DirtyParts, OpPhrase, PlanOp, UntrackedCtx};
pub use conflicts::{ConflictsNote, ConflictsRecovery, ConflictsTitle};
pub use discard::DiscardNote;
pub use force_lease::{ForceLeaseNote, ForceLeaseRecovery, ForceLeaseTitle};
pub use github::{GithubNote, GithubRecovery, GithubTitle};
pub use history::{HistoryMoveDir, HistoryNote, HistoryOp, HistoryRecovery, HistoryTitle};
pub use merge::{InProgressOp, MergeNote, MergeRecovery, MergeTitle};
pub use pull::{PullNote, PullRecovery, PullTitle};
pub use push::{PushNote, PushRecovery, PushTitle};
pub use rebase::{RebaseNote, RebaseRecovery, RebaseTitle};
pub use remote_branch::{RemoteBranchNote, RemoteBranchRecovery, RemoteBranchTitle};
pub use reset::{ResetNote, ResetRecovery, ResetTitle};
pub use snapshot::{SnapshotNote, SnapshotRecovery, SnapshotTitle};
pub use stash::{StashNote, StashRecovery, StashTitle};
pub use switch::{SwitchNote, SwitchRecovery, SwitchTitle};
pub use tag::{TagNote, TagRecovery, TagTitle};
pub use worktree::{WorktreeNote, WorktreeRecovery, WorktreeTitle};

/// Category-nested note shown in the plan modal's blockers/warnings lists
/// (ADR-0129 §1). Flat 100+-variant enums are forbidden — one variant space
/// per op category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanNote {
    /// Cross-op notes (dirty WT / conflicted / untracked / HEAD state — §A).
    Common(CommonNote),
    /// Discard-file notes (first structured producer, ADR-0129 Phase 1).
    Discard(DiscardNote),
    Branch(BranchNote),
    Stash(StashNote),
    History(HistoryNote),
    Pull(PullNote),
    Push(PushNote),
    Switch(SwitchNote),
    Checkout(CheckoutNote),
    Merge(MergeNote),
    Worktree(WorktreeNote),
    CherryRevert(CherryRevertNote),
    Cleanup(CleanupNote),
    Conflicts(ConflictsNote),
    Commit(CommitNote),
    /// Commit-checklist findings (ADR-0043 rules 4/5/6 — no title/recovery).
    Checklist(ChecklistNote),
    Tag(TagNote),
    RemoteBranch(RemoteBranchNote),
    Reset(ResetNote),
    ForceLease(ForceLeaseNote),
    Github(GithubNote),
    Rebase(RebaseNote),
    Snapshot(SnapshotNote),
}

impl PlanNote {
    /// The **sole** English renderer (ADR-0129 §3): used for EN display, the
    /// oplog boundary, and klog. Byte-identical to the pre-migration strings
    /// (golden-tested below and in each category's own file).
    pub fn message_en(&self) -> String {
        match self {
            PlanNote::Common(n) => n.message_en(),
            PlanNote::Discard(n) => n.message_en(),
            PlanNote::Branch(n) => n.message_en(),
            PlanNote::Stash(n) => n.message_en(),
            PlanNote::History(n) => n.message_en(),
            PlanNote::Pull(n) => n.message_en(),
            PlanNote::Push(n) => n.message_en(),
            PlanNote::Switch(n) => n.message_en(),
            PlanNote::Checkout(n) => n.message_en(),
            PlanNote::Merge(n) => n.message_en(),
            PlanNote::Worktree(n) => n.message_en(),
            PlanNote::CherryRevert(n) => n.message_en(),
            PlanNote::Cleanup(n) => n.message_en(),
            PlanNote::Conflicts(n) => n.message_en(),
            PlanNote::Commit(n) => n.message_en(),
            PlanNote::Checklist(n) => n.message_en(),
            PlanNote::Tag(n) => n.message_en(),
            PlanNote::RemoteBranch(n) => n.message_en(),
            PlanNote::Reset(n) => n.message_en(),
            PlanNote::ForceLease(n) => n.message_en(),
            PlanNote::Github(n) => n.message_en(),
            PlanNote::Rebase(n) => n.message_en(),
            PlanNote::Snapshot(n) => n.message_en(),
        }
    }
}

impl std::fmt::Display for PlanNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message_en())
    }
}

/// The plan modal's one required title (ADR-0129 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanTitle {
    /// Discard: `Discard changes to '<file>'` / `Discard changes to N file(s)`.
    Discard {
        /// The single target's repo-relative path when exactly one file.
        single: Option<String>,
        /// Total selected targets.
        count: usize,
    },
    Branch(BranchTitle),
    Stash(StashTitle),
    History(HistoryTitle),
    Pull(PullTitle),
    Push(PushTitle),
    Switch(SwitchTitle),
    Checkout(CheckoutTitle),
    Merge(MergeTitle),
    Worktree(WorktreeTitle),
    CherryRevert(CherryRevertTitle),
    Cleanup(CleanupTitle),
    Conflicts(ConflictsTitle),
    Commit(CommitTitle),
    Tag(TagTitle),
    RemoteBranch(RemoteBranchTitle),
    Reset(ResetTitle),
    ForceLease(ForceLeaseTitle),
    Github(GithubTitle),
    Rebase(RebaseTitle),
    Snapshot(SnapshotTitle),
}

impl PlanTitle {
    /// Sole English renderer — see [`PlanNote::message_en`].
    pub fn message_en(&self) -> String {
        match self {
            PlanTitle::Branch(t) => t.message_en(),
            PlanTitle::Stash(t) => t.message_en(),
            PlanTitle::History(t) => t.message_en(),
            PlanTitle::Pull(t) => t.message_en(),
            PlanTitle::Push(t) => t.message_en(),
            PlanTitle::Switch(t) => t.message_en(),
            PlanTitle::Checkout(t) => t.message_en(),
            PlanTitle::Merge(t) => t.message_en(),
            PlanTitle::Worktree(t) => t.message_en(),
            PlanTitle::CherryRevert(t) => t.message_en(),
            PlanTitle::Cleanup(t) => t.message_en(),
            PlanTitle::Conflicts(t) => t.message_en(),
            PlanTitle::Commit(t) => t.message_en(),
            PlanTitle::Tag(t) => t.message_en(),
            PlanTitle::RemoteBranch(t) => t.message_en(),
            PlanTitle::Reset(t) => t.message_en(),
            PlanTitle::ForceLease(t) => t.message_en(),
            PlanTitle::Github(t) => t.message_en(),
            PlanTitle::Rebase(t) => t.message_en(),
            PlanTitle::Snapshot(t) => t.message_en(),
            PlanTitle::Discard {
                single: Some(path), ..
            } => format!("Discard changes to '{}'", path),
            PlanTitle::Discard {
                single: None,
                count,
            } => {
                format!("Discard changes to {} file(s)", count)
            }
        }
    }
}

impl std::fmt::Display for PlanTitle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message_en())
    }
}

/// Recovery guidance: display text + the machine-usable command list
/// (ADR-0129 appendix §D). `commands` is what consumers use instead of
/// parsing the display text (kills the delete-branch `lines().nth(1)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRecovery {
    /// The displayed recovery block.
    pub kind: RecoveryKind,
    /// Structured, copy/paste-able commands referenced by the text, in the
    /// order they appear. `commands.first()` is the primary restore command.
    pub commands: Vec<String>,
}

impl PlanRecovery {
    /// Sole English renderer — see [`PlanNote::message_en`].
    pub fn message_en(&self) -> String {
        match &self.kind {
            RecoveryKind::Branch(r) => r.message_en(),
            RecoveryKind::Stash(r) => r.message_en(),
            RecoveryKind::History(r) => r.message_en(),
            RecoveryKind::Pull(r) => r.message_en(),
            RecoveryKind::Push(r) => r.message_en(),
            RecoveryKind::Switch(r) => r.message_en(),
            RecoveryKind::Checkout(r) => r.message_en(),
            RecoveryKind::Merge(r) => r.message_en(),
            RecoveryKind::Worktree(r) => r.message_en(),
            RecoveryKind::CherryRevert(r) => r.message_en(),
            RecoveryKind::Cleanup(r) => r.message_en(),
            RecoveryKind::Conflicts(r) => r.message_en(),
            RecoveryKind::Commit(r) => r.message_en(),
            RecoveryKind::Tag(r) => r.message_en(),
            RecoveryKind::RemoteBranch(r) => r.message_en(),
            RecoveryKind::Reset(r) => r.message_en(),
            RecoveryKind::ForceLease(r) => r.message_en(),
            RecoveryKind::Github(r) => r.message_en(),
            RecoveryKind::Rebase(r) => r.message_en(),
            RecoveryKind::Snapshot(r) => r.message_en(),
            RecoveryKind::Discard => {
                "This discards your unstaged changes to the selected file(s): \
                 tracked files are restored from the index, untracked files are deleted from \
                 disk. Either way a backup blob of each file's current content is recorded in \
                 the oplog (op=\"discard\") first; recover with `git cat-file -p <blob-sha>`."
                    .to_string()
            }
        }
    }
}

/// Which recovery text to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryKind {
    /// Discard: index-restore / delete-with-backup explanation.
    Discard,
    Branch(BranchRecovery),
    Stash(StashRecovery),
    History(HistoryRecovery),
    Pull(PullRecovery),
    Push(PushRecovery),
    Switch(SwitchRecovery),
    Checkout(CheckoutRecovery),
    Merge(MergeRecovery),
    Worktree(WorktreeRecovery),
    CherryRevert(CherryRevertRecovery),
    Cleanup(CleanupRecovery),
    Conflicts(ConflictsRecovery),
    Commit(CommitRecovery),
    Tag(TagRecovery),
    RemoteBranch(RemoteBranchRecovery),
    Reset(ResetRecovery),
    ForceLease(ForceLeaseRecovery),
    Github(GithubRecovery),
    Rebase(RebaseRecovery),
    Snapshot(SnapshotRecovery),
}

/// Semantic plan state (ADR-0129 §2). Replaces every place the UI used to
/// *parse display strings* to decide behavior. Invariant: no-op detection,
/// recovery handling, and safety decisions never look at display text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanDisposition {
    /// Executable as planned.
    Ready,
    /// Nothing to do — the UI may skip or soften the confirm step.
    NoOp(NoOpKind),
    /// Blockers present; the execute button must be hidden.
    Blocked,
}

impl PlanDisposition {
    /// Default derivation for producers with no explicit no-op state.
    pub fn for_blockers<T>(blockers: &[T]) -> Self {
        if blockers.is_empty() {
            PlanDisposition::Ready
        } else {
            PlanDisposition::Blocked
        }
    }
}

/// What kind of nothing-to-do (Phase 1 covers the two UI string-matches;
/// Phase 2 producers add their kinds as they structure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoOpKind {
    /// pull: local knowledge says already up to date (behind == 0).
    PullUpToDate,
    /// push: ahead == 0 — nothing to push.
    PushUpToDate,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── message_en golden tests (ADR-0129 §3): dynamic values, newlines,
    //    quotes, and paths must render byte-identically to the legacy
    //    producer strings. ──

    #[test]
    fn discard_notes_match_legacy_strings() {
        assert_eq!(
            DiscardNote::NothingSelected.message_en(),
            "Nothing to discard: no files selected."
        );
        assert_eq!(
            DiscardNote::TargetConflicted {
                path: "src/a b.rs".into()
            }
            .message_en(),
            "'src/a b.rs' is conflicted. Resolve the conflict instead of discarding it."
        );
        assert_eq!(
            DiscardNote::NoUnstagedChanges {
                path: "dir/file.txt".into()
            }
            .message_en(),
            "'dir/file.txt' has no unstaged changes to discard."
        );
        assert_eq!(
            DiscardNote::TargetSubmodule {
                path: "vendor/sub".into()
            }
            .message_en(),
            "'vendor/sub' is a submodule. Discard cannot operate on submodules; \
             manage the change from inside the submodule instead."
        );
        assert_eq!(
            DiscardNote::UntrackedWillBeDeleted { count: 3 }.message_en(),
            "⚠️ 3 untracked file(s) will be PERMANENTLY DELETED from disk (and any \
             now-empty folders removed). A backup blob is saved to the oplog first — \
             recover with `git cat-file -p <blob-sha>`."
        );
    }

    #[test]
    fn discard_title_matches_legacy_strings() {
        assert_eq!(
            PlanTitle::Discard {
                single: Some("src/main.rs".into()),
                count: 1
            }
            .message_en(),
            "Discard changes to 'src/main.rs'"
        );
        assert_eq!(
            PlanTitle::Discard {
                single: None,
                count: 4
            }
            .message_en(),
            "Discard changes to 4 file(s)"
        );
    }

    #[test]
    fn discard_recovery_matches_legacy_string() {
        let r = PlanRecovery {
            kind: RecoveryKind::Discard,
            commands: vec!["git cat-file -p <blob-sha>".into()],
        };
        assert_eq!(
            r.message_en(),
            "This discards your unstaged changes to the selected file(s): \
             tracked files are restored from the index, untracked files are deleted from \
             disk. Either way a backup blob of each file's current content is recorded in \
             the oplog (op=\"discard\") first; recover with `git cat-file -p <blob-sha>`."
        );
    }

    /// Every `PlanNote` category arm must forward to its own enum's
    /// `message_en()`. Without this, a mis-wired (or stubbed) dispatch arm is
    /// invisible: the category enums are golden-tested in their own files,
    /// but nothing checks that `PlanNote` actually calls them.
    ///
    /// Add a row here when a `PlanNote` variant is added — the `match` below
    /// is exhaustive, so the compiler fails until you do.
    #[test]
    fn every_plan_note_arm_delegates_to_its_category() {
        let cases: Vec<PlanNote> = vec![
            PlanNote::Common(CommonNote::SuggestStashPush),
            PlanNote::Discard(DiscardNote::NothingSelected),
            PlanNote::Branch(BranchNote::RenameRefOnlyDirty),
            PlanNote::Stash(StashNote::NothingToStash),
            PlanNote::History(HistoryNote::EmptyMessage),
            PlanNote::Pull(PullNote::MergePrediction),
            PlanNote::Push(PushNote::UpstreamFormatInvalid),
            PlanNote::Switch(SwitchNote::NameEmpty),
            PlanNote::Checkout(CheckoutNote::CommitAlreadyHead),
            PlanNote::Merge(MergeNote::NoChanges {
                target: "main".into(),
            }),
            PlanNote::Worktree(WorktreeNote::WorktreeMissing { name: "wt".into() }),
            PlanNote::CherryRevert(CherryRevertNote::NotInCurrentBranch { sha: "abc".into() }),
            PlanNote::Cleanup(CleanupNote::NoSelection),
            PlanNote::Conflicts(ConflictsNote::EmptyMergeMessage),
            PlanNote::Commit(CommitNote::EmptyMessage),
            PlanNote::Checklist(ChecklistNote::ConflictMarkerFound {
                path: "src/a.rs".into(),
            }),
            PlanNote::Tag(TagNote::NoRemote),
            PlanNote::RemoteBranch(RemoteBranchNote::LocalBranchUntouched {
                local_name: "feat".into(),
            }),
            PlanNote::Reset(ResetNote::DetachedHead),
            PlanNote::ForceLease(ForceLeaseNote::NoUpstream {
                branch: "feat".into(),
            }),
            PlanNote::Github(GithubNote::RemoteSideEffect),
            PlanNote::Rebase(RebaseNote::DetachedHead),
            PlanNote::Snapshot(SnapshotNote::SavepointFirst),
        ];

        for note in &cases {
            // The arm must return exactly what the category enum renders …
            let inner = match note {
                PlanNote::Common(n) => n.message_en(),
                PlanNote::Discard(n) => n.message_en(),
                PlanNote::Branch(n) => n.message_en(),
                PlanNote::Stash(n) => n.message_en(),
                PlanNote::History(n) => n.message_en(),
                PlanNote::Pull(n) => n.message_en(),
                PlanNote::Push(n) => n.message_en(),
                PlanNote::Switch(n) => n.message_en(),
                PlanNote::Checkout(n) => n.message_en(),
                PlanNote::Merge(n) => n.message_en(),
                PlanNote::Worktree(n) => n.message_en(),
                PlanNote::CherryRevert(n) => n.message_en(),
                PlanNote::Cleanup(n) => n.message_en(),
                PlanNote::Conflicts(n) => n.message_en(),
                PlanNote::Commit(n) => n.message_en(),
                PlanNote::Checklist(n) => n.message_en(),
                PlanNote::Tag(n) => n.message_en(),
                PlanNote::RemoteBranch(n) => n.message_en(),
                PlanNote::Reset(n) => n.message_en(),
                PlanNote::ForceLease(n) => n.message_en(),
                PlanNote::Github(n) => n.message_en(),
                PlanNote::Rebase(n) => n.message_en(),
                PlanNote::Snapshot(n) => n.message_en(),
            };
            assert_eq!(note.message_en(), inner, "dispatch arm for {note:?}");
            // … which is never empty, and reaches Display unchanged.
            assert!(!inner.trim().is_empty(), "empty message for {note:?}");
            assert_eq!(note.to_string(), inner, "Display for {note:?}");
        }

        // One fixture per `PlanNote` variant — bump this when a category is
        // added (and add its row above).
        assert_eq!(cases.len(), 23, "one fixture per PlanNote variant");
    }

    #[test]
    fn disposition_for_blockers() {
        assert_eq!(
            PlanDisposition::for_blockers::<PlanNote>(&[]),
            PlanDisposition::Ready
        );
        assert_eq!(
            PlanDisposition::for_blockers(&[PlanNote::Discard(DiscardNote::NothingSelected)]),
            PlanDisposition::Blocked
        );
    }
}
