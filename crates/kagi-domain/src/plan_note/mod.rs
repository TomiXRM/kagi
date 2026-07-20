//! Structured plan text (ADR-0129) — `PlanTitle` / `PlanNote` / `PlanRecovery`
//! / `PlanDisposition`.
//!
//! The ops layer stops producing English prose and returns these structured
//! values instead; the display layer localizes them. During the migration
//! (Phase 1/2) most producers wrap their existing strings in the
//! [`PlanNote::Verbatim`] variant — Phase 3 deletes `Verbatim` and every
//! String-conversion shim, proving the migration complete by compilation
//! (ADR-0129 §4).
//!
//! [`PlanNote::message_en`] is the **single English renderer** and the
//! oplog/klog boundary format. While the migration is in flight its output is
//! byte-identical to the legacy strings (a *migration contract*, not an
//! eternal freeze — EN copy improvements unlock after Phase 3). Golden tests
//! live in this module.

//!
//! One file per category (ADR-0129 Phase 2 foundation): the per-op fan-out
//! PRs each fill their own `plan_note/<category>.rs` enum and never touch
//! this dispatch file, so they can land in parallel without conflicts.

pub mod branch;
pub mod checkout;
pub mod cherry_revert;
pub mod cleanup;
pub mod commit;
pub mod common;
pub mod conflicts;
pub mod discard;
pub mod history;
pub mod merge;
pub mod pull;
pub mod push;
pub mod stash;
pub mod switch;
pub mod worktree;

pub use branch::{BranchNote, BranchRecovery, BranchTitle};
pub use checkout::{CheckoutNote, CheckoutRecovery, CheckoutTitle};
pub use cherry_revert::{CherryRevertNote, CherryRevertRecovery, CherryRevertTitle};
pub use cleanup::{CleanupNote, CleanupRecovery, CleanupTitle};
pub use commit::{CommitNote, CommitRecovery, CommitTitle};
pub use common::{CommonNote, DirtyParts, OpPhrase, PlanOp, UntrackedCtx};
pub use conflicts::{ConflictsNote, ConflictsRecovery, ConflictsTitle};
pub use discard::DiscardNote;
pub use history::{HistoryNote, HistoryOp, HistoryRecovery, HistoryTitle};
pub use merge::{MergeNote, MergeRecovery, MergeTitle};
pub use pull::{PullNote, PullRecovery, PullTitle};
pub use push::{PushNote, PushRecovery, PushTitle};
pub use stash::{StashNote, StashRecovery, StashTitle};
pub use switch::{SwitchNote, SwitchRecovery, SwitchTitle};
pub use worktree::{WorktreeNote, WorktreeRecovery, WorktreeTitle};

/// Category-nested note shown in the plan modal's blockers/warnings lists
/// (ADR-0129 §1). Flat 100+-variant enums are forbidden — one variant space
/// per op category, plus the migration-only `Verbatim`.
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
    /// Migration-only escape hatch: the legacy English prose, byte-for-byte.
    /// Deleted in Phase 3 — do not add new uses outside mechanical wrapping.
    Verbatim(String),
}

impl PlanNote {
    /// Wrap a legacy English string (migration helper, Phase 3 deletes).
    pub fn verbatim(s: impl Into<String>) -> Self {
        PlanNote::Verbatim(s.into())
    }

    /// Wrap a whole legacy `Vec<String>` (migration helper, Phase 3 deletes).
    pub fn wrap_all(strings: Vec<String>) -> Vec<PlanNote> {
        strings.into_iter().map(PlanNote::Verbatim).collect()
    }

    /// The **sole** English renderer (ADR-0129 §3): used for EN display, the
    /// oplog boundary, and klog. Byte-identical to the legacy strings while
    /// the migration is in flight (golden-tested below).
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
            PlanNote::Verbatim(s) => s.clone(),
        }
    }
}

impl std::fmt::Display for PlanNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message_en())
    }
}

/// Migration shims so string-pinning tests keep compiling unchanged.
/// Deleted in Phase 3 together with `Verbatim`.
impl PartialEq<str> for PlanNote {
    fn eq(&self, other: &str) -> bool {
        self.message_en() == other
    }
}
impl PartialEq<&str> for PlanNote {
    fn eq(&self, other: &&str) -> bool {
        self.message_en() == *other
    }
}
impl PartialEq<String> for PlanNote {
    fn eq(&self, other: &String) -> bool {
        self.message_en() == *other
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
    /// Migration-only legacy title (Phase 3 deletes).
    Verbatim(String),
}

impl PlanTitle {
    /// Wrap a legacy English title (migration helper, Phase 3 deletes).
    pub fn verbatim(s: impl Into<String>) -> Self {
        PlanTitle::Verbatim(s.into())
    }

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
            PlanTitle::Verbatim(s) => s.clone(),
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

impl PartialEq<str> for PlanTitle {
    fn eq(&self, other: &str) -> bool {
        self.message_en() == other
    }
}
impl PartialEq<&str> for PlanTitle {
    fn eq(&self, other: &&str) -> bool {
        self.message_en() == *other
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
    /// Wrap a legacy recovery string with no structured commands
    /// (migration helper, Phase 3 deletes).
    pub fn verbatim(s: impl Into<String>) -> Self {
        PlanRecovery {
            kind: RecoveryKind::Verbatim(s.into()),
            commands: Vec::new(),
        }
    }

    /// Wrap a legacy recovery string, recording its structured commands.
    pub fn verbatim_with_commands(s: impl Into<String>, commands: Vec<String>) -> Self {
        PlanRecovery {
            kind: RecoveryKind::Verbatim(s.into()),
            commands,
        }
    }

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
            RecoveryKind::Verbatim(s) => s.clone(),
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
    /// Migration-only legacy text (Phase 3 deletes).
    Verbatim(String),
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
    use super::common::{DirtyParts, OpPhrase, PlanOp, UntrackedCtx};
    use super::*;

    // ── message_en golden tests (ADR-0129 §3): dynamic values, newlines,
    //    quotes, and paths must render byte-identically to the legacy
    //    producer strings. ──

    #[test]
    fn verbatim_note_roundtrips_bytes() {
        let s = "Working tree has 2 staged, 1 modified — stash or commit changes before merging.\n  git stash push -u\n\"quoted\" and path/to/file.rs";
        assert_eq!(PlanNote::verbatim(s).message_en(), s);
    }

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

    #[test]
    fn note_eq_str_shim_compares_rendered_text() {
        let n = PlanNote::verbatim("Branch 'x' does not exist.");
        assert_eq!(n, "Branch 'x' does not exist.");
        assert_eq!(n, "Branch 'x' does not exist.".to_string());
    }

    #[test]
    fn disposition_for_blockers() {
        assert_eq!(
            PlanDisposition::for_blockers::<PlanNote>(&[]),
            PlanDisposition::Ready
        );
        assert_eq!(
            PlanDisposition::for_blockers(&[PlanNote::verbatim("x")]),
            PlanDisposition::Blocked
        );
    }
}
