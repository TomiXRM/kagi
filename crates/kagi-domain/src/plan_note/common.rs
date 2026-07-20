//! Cross-op plan notes (`CommonNote`) — ADR-0129 appendix §A.
//!
//! These are the templates that appear in several ops with only the op phrase
//! varying. Where the English sentences are structurally identical the phrase
//! is an enum argument ([`OpPhrase`]); where each op has its own sentence
//! (HEAD detached/unborn) the variant carries [`PlanOp`] and `message_en`
//! keeps a per-op sentence table so every string stays byte-identical to the
//! legacy producers (golden-tested in `plan_note::tests`).

use crate::plan::{BranchNameError, WorktreePathError};

/// The `…before {phrase}.` / `…before {phrase} if…` op phrase (§A1/A2/A11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpPhrase {
    /// "undoing a commit"
    UndoingACommit,
    /// "amending"
    Amending,
    /// "undo" (history-move label, lowercased)
    Undo,
    /// "redo" (history-move label, lowercased)
    Redo,
    /// "checkout"
    Checkout,
    /// "switching"
    Switching,
    /// "cherry-picking"
    CherryPicking,
    /// "reverting"
    Reverting,
    /// "pulling"
    Pulling,
    /// "merging"
    Merging,
    /// "switching branches"
    SwitchingBranches,
    /// "stashing"
    Stashing,
    /// "applying a stash"
    ApplyingAStash,
    /// "checking out the new branch"
    CheckingOutTheNewBranch,
}

impl OpPhrase {
    /// The exact legacy phrase embedded in the English sentences.
    pub fn phrase_en(self) -> &'static str {
        match self {
            OpPhrase::UndoingACommit => "undoing a commit",
            OpPhrase::Amending => "amending",
            OpPhrase::Undo => "undo",
            OpPhrase::Redo => "redo",
            OpPhrase::Checkout => "checkout",
            OpPhrase::Switching => "switching",
            OpPhrase::CherryPicking => "cherry-picking",
            OpPhrase::Reverting => "reverting",
            OpPhrase::Pulling => "pulling",
            OpPhrase::Merging => "merging",
            OpPhrase::SwitchingBranches => "switching branches",
            OpPhrase::Stashing => "stashing",
            OpPhrase::ApplyingAStash => "applying a stash",
            OpPhrase::CheckingOutTheNewBranch => "checking out the new branch",
        }
    }
}

/// The op discriminant for the per-op sentence tables (§A12/A13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanOp {
    Undo,
    Amend,
    CherryPick,
    Revert,
    Pull,
    Push,
    Merge,
}

/// The `"{n} staged, {n} modified"` fragment several dirty-working-tree
/// sentences embed. Only the non-zero parts are rendered, joined by `", "`,
/// exactly like the legacy per-site builders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyParts {
    pub staged: usize,
    pub modified: usize,
}

impl DirtyParts {
    /// `"2 staged, 1 modified"` / `"2 staged"` / `"1 modified"`.
    pub fn parts_en(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.staged > 0 {
            parts.push(format!("{} staged", self.staged));
        }
        if self.modified > 0 {
            parts.push(format!("{} modified", self.modified));
        }
        parts.join(", ")
    }
}

/// Which sentence tail the untracked-files warning uses (§A4–A10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UntrackedCtx {
    /// "…will remain after checkout."
    AfterCheckout,
    /// "…will remain after switching."
    AfterSwitching,
    /// "…will remain after switching branches."
    AfterSwitchingBranches,
    /// "…will remain untouched after cherry-pick."
    AfterCherryPick,
    /// "…will remain untouched after revert."
    AfterRevert,
    /// "…will remain untouched unless fetched changes need the same path."
    PullFetchMayTouch,
    /// "…will remain untouched."
    Untouched,
}

/// Cross-op notes (ADR-0129 appendix §A).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommonNote {
    /// §A1 — blocker: conflicted files must be resolved first.
    ConflictedFiles { count: usize, before: OpPhrase },
    /// §A2 — blocker: dirty working tree blocks the op.
    DirtyBlocksOp { parts: DirtyParts, before: OpPhrase },
    /// §A3 — warning: suggest `git stash push -u`.
    SuggestStashPush,
    /// §A4–A10 — warning: untracked files remain.
    UntrackedRemain { count: usize, ctx: UntrackedCtx },
    /// §A11 — warning (merge_dirty_warnings): dirty WT rollback hint.
    DirtyRollbackHint { parts: DirtyParts, op: OpPhrase },
    /// §A12 — blocker: HEAD is detached (per-op sentence).
    HeadDetached { op: PlanOp },
    /// §A13 — blocker: HEAD is unborn (per-op sentence).
    HeadUnborn { op: PlanOp },
    /// §A14/A15 — blocker: branch does not exist (two legacy tails).
    BranchMissing { name: String, in_repo: bool },
    /// §G-4 — blocker: a GitError/git2 message (or any other English-only
    /// text whose keying is out of scope for ADR-0129, e.g.
    /// `WorktreeValidationError::Other`) passed through verbatim.
    GitErrorPassthrough { message: String },
    /// §E — blocker: a keyed branch-name validation reason (create-branch /
    /// rename-branch). Localizes via the existing
    /// `kagi_ui_core::i18n::branch_name_error` mapping.
    BranchNameErrorKeyed(BranchNameError),
    /// §E — blocker: a keyed worktree-path validation reason
    /// (create-worktree). Localizes via the existing
    /// `kagi_ui_core::i18n::worktree_path_error` mapping.
    WorktreePathErrorKeyed(WorktreePathError),
    /// §F-6 — warning: the UI inserts this into `plan.warnings` when
    /// confirming a dirty checkout will stash first (was
    /// `Msg::DirtyStashFirst`, formerly a UI-language string mixed directly
    /// into the plan — now typed and localized like every other note).
    DirtyStashFirst,
    /// warning: the UI inserts this into `plan.warnings` for a
    /// conflict-producing merge (was `Msg::MergeConflictWarning`).
    MergeConflictWarning,
}

impl CommonNote {
    /// Byte-identical to the legacy producer strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            CommonNote::ConflictedFiles { count, before } => format!(
                "Repository has {} conflicted file(s). Resolve conflicts before {}.",
                count,
                before.phrase_en()
            ),
            CommonNote::DirtyBlocksOp { parts, before } => format!(
                "Working tree has {} — stash or commit changes before {}.",
                parts.parts_en(),
                before.phrase_en()
            ),
            CommonNote::SuggestStashPush => "Suggested command: git stash push -u".to_string(),
            CommonNote::UntrackedRemain { count, ctx } => match ctx {
                UntrackedCtx::AfterCheckout => {
                    format!("{} untracked file(s) will remain after checkout.", count)
                }
                UntrackedCtx::AfterSwitching => {
                    format!("{} untracked file(s) will remain after switching.", count)
                }
                UntrackedCtx::AfterSwitchingBranches => format!(
                    "{} untracked file(s) will remain after switching branches.",
                    count
                ),
                UntrackedCtx::AfterCherryPick => format!(
                    "{} untracked file(s) will remain untouched after cherry-pick.",
                    count
                ),
                UntrackedCtx::AfterRevert => format!(
                    "{} untracked file(s) will remain untouched after revert.",
                    count
                ),
                UntrackedCtx::PullFetchMayTouch => format!(
                    "{} untracked file(s) will remain untouched unless fetched changes need the same path.",
                    count
                ),
                UntrackedCtx::Untouched => {
                    format!("{} untracked file(s) will remain untouched.", count)
                }
            },
            CommonNote::DirtyRollbackHint { parts, op } => format!(
                "Working tree has {}. Stash or commit before {} if you want a clean rollback point.",
                parts.parts_en(),
                op.phrase_en()
            ),
            CommonNote::HeadDetached { op } => match op {
                PlanOp::Undo => {
                    "HEAD is detached. Undo commit requires HEAD to be on a branch.".to_string()
                }
                PlanOp::Amend => {
                    "HEAD is detached. Amend requires HEAD to be on a branch.".to_string()
                }
                PlanOp::CherryPick => {
                    "HEAD is detached. Cherry-pick is only supported when HEAD is on a branch."
                        .to_string()
                }
                PlanOp::Revert => {
                    "HEAD is detached. Revert is only supported when HEAD is on a branch."
                        .to_string()
                }
                PlanOp::Pull => {
                    "HEAD is detached. Pull is only supported when HEAD is on a branch.".to_string()
                }
                PlanOp::Push => {
                    "HEAD is detached. Push is only supported when HEAD is on a branch.".to_string()
                }
                PlanOp::Merge => "HEAD is detached. Merge is only supported on a branch.".to_string(),
            },
            CommonNote::HeadUnborn { op } => match op {
                PlanOp::Undo => {
                    "HEAD is unborn (no commits exist). There is nothing to undo.".to_string()
                }
                PlanOp::Amend => {
                    "HEAD is unborn (no commits exist). There is nothing to amend.".to_string()
                }
                PlanOp::CherryPick => {
                    "HEAD is unborn (no commits exist). Cannot cherry-pick onto an empty branch."
                        .to_string()
                }
                PlanOp::Revert => {
                    "HEAD is unborn (no commits exist). Cannot revert on an empty branch."
                        .to_string()
                }
                PlanOp::Pull => {
                    "HEAD is unborn (no commits exist). Cannot pull onto an empty branch."
                        .to_string()
                }
                PlanOp::Push => {
                    "HEAD is unborn (no commits exist). Cannot push an empty branch.".to_string()
                }
                PlanOp::Merge => "HEAD is unborn. Cannot merge into an empty branch.".to_string(),
            },
            CommonNote::BranchMissing { name, in_repo } => {
                if *in_repo {
                    format!("Branch '{}' does not exist in this repository.", name)
                } else {
                    format!("Branch '{}' does not exist.", name)
                }
            }
            CommonNote::GitErrorPassthrough { message } => message.clone(),
            CommonNote::BranchNameErrorKeyed(e) => e.to_string(),
            CommonNote::WorktreePathErrorKeyed(e) => e.to_string(),
            CommonNote::DirtyStashFirst => "Working tree is dirty: confirming will stash your \
                 changes first (saved to stash@{0}, restore with `git stash pop`)"
                .to_string(),
            CommonNote::MergeConflictWarning => "This merge will produce conflicts. It will \
                 leave conflict markers and enter Conflict Mode, where you resolve each file (or \
                 abort to restore the pre-merge state)."
                .to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // message_en golden tests (ADR-0129 §3) — every variant × discriminant a
    // Phase 2 producer will emit, byte-exact vs the appendix templates.

    #[test]
    fn conflicted_files_all_phrases() {
        let cases = [
            (OpPhrase::UndoingACommit, "undoing a commit"),
            (OpPhrase::Amending, "amending"),
            (OpPhrase::Undo, "undo"),
            (OpPhrase::Redo, "redo"),
            (OpPhrase::Checkout, "checkout"),
            (OpPhrase::Switching, "switching"),
            (OpPhrase::CherryPicking, "cherry-picking"),
            (OpPhrase::Reverting, "reverting"),
            (OpPhrase::Pulling, "pulling"),
            (OpPhrase::Merging, "merging"),
            (OpPhrase::SwitchingBranches, "switching branches"),
            (OpPhrase::Stashing, "stashing"),
            (OpPhrase::ApplyingAStash, "applying a stash"),
            (
                OpPhrase::CheckingOutTheNewBranch,
                "checking out the new branch",
            ),
        ];
        for (phrase, word) in cases {
            assert_eq!(
                CommonNote::ConflictedFiles {
                    count: 2,
                    before: phrase
                }
                .message_en(),
                format!(
                    "Repository has 2 conflicted file(s). Resolve conflicts before {}.",
                    word
                )
            );
        }
    }

    #[test]
    fn dirty_blocks_op_and_parts() {
        assert_eq!(
            CommonNote::DirtyBlocksOp {
                parts: DirtyParts {
                    staged: 2,
                    modified: 1
                },
                before: OpPhrase::Merging
            }
            .message_en(),
            "Working tree has 2 staged, 1 modified — stash or commit changes before merging."
        );
        assert_eq!(
            CommonNote::DirtyBlocksOp {
                parts: DirtyParts {
                    staged: 0,
                    modified: 3
                },
                before: OpPhrase::Switching
            }
            .message_en(),
            "Working tree has 3 modified — stash or commit changes before switching."
        );
        assert_eq!(
            CommonNote::DirtyBlocksOp {
                parts: DirtyParts {
                    staged: 1,
                    modified: 0
                },
                before: OpPhrase::Checkout
            }
            .message_en(),
            "Working tree has 1 staged — stash or commit changes before checkout."
        );
    }

    #[test]
    fn suggest_stash_push() {
        assert_eq!(
            CommonNote::SuggestStashPush.message_en(),
            "Suggested command: git stash push -u"
        );
    }

    #[test]
    fn untracked_remain_all_ctx() {
        let cases = [
            (
                UntrackedCtx::AfterCheckout,
                "3 untracked file(s) will remain after checkout.",
            ),
            (
                UntrackedCtx::AfterSwitching,
                "3 untracked file(s) will remain after switching.",
            ),
            (
                UntrackedCtx::AfterSwitchingBranches,
                "3 untracked file(s) will remain after switching branches.",
            ),
            (
                UntrackedCtx::AfterCherryPick,
                "3 untracked file(s) will remain untouched after cherry-pick.",
            ),
            (
                UntrackedCtx::AfterRevert,
                "3 untracked file(s) will remain untouched after revert.",
            ),
            (
                UntrackedCtx::PullFetchMayTouch,
                "3 untracked file(s) will remain untouched unless fetched changes need the same path.",
            ),
            (
                UntrackedCtx::Untouched,
                "3 untracked file(s) will remain untouched.",
            ),
        ];
        for (ctx, want) in cases {
            assert_eq!(
                CommonNote::UntrackedRemain { count: 3, ctx }.message_en(),
                want
            );
        }
    }

    #[test]
    fn dirty_rollback_hint() {
        assert_eq!(
            CommonNote::DirtyRollbackHint {
                parts: DirtyParts {
                    staged: 1,
                    modified: 2
                },
                op: OpPhrase::Merging
            }
            .message_en(),
            "Working tree has 1 staged, 2 modified. Stash or commit before merging if you want a clean rollback point."
        );
    }

    #[test]
    fn head_detached_all_ops() {
        let cases = [
            (
                PlanOp::Undo,
                "HEAD is detached. Undo commit requires HEAD to be on a branch.",
            ),
            (
                PlanOp::Amend,
                "HEAD is detached. Amend requires HEAD to be on a branch.",
            ),
            (
                PlanOp::CherryPick,
                "HEAD is detached. Cherry-pick is only supported when HEAD is on a branch.",
            ),
            (
                PlanOp::Revert,
                "HEAD is detached. Revert is only supported when HEAD is on a branch.",
            ),
            (
                PlanOp::Pull,
                "HEAD is detached. Pull is only supported when HEAD is on a branch.",
            ),
            (
                PlanOp::Push,
                "HEAD is detached. Push is only supported when HEAD is on a branch.",
            ),
            (
                PlanOp::Merge,
                "HEAD is detached. Merge is only supported on a branch.",
            ),
        ];
        for (op, want) in cases {
            assert_eq!(CommonNote::HeadDetached { op }.message_en(), want);
        }
    }

    #[test]
    fn head_unborn_all_ops() {
        let cases = [
            (
                PlanOp::Undo,
                "HEAD is unborn (no commits exist). There is nothing to undo.",
            ),
            (
                PlanOp::Amend,
                "HEAD is unborn (no commits exist). There is nothing to amend.",
            ),
            (
                PlanOp::CherryPick,
                "HEAD is unborn (no commits exist). Cannot cherry-pick onto an empty branch.",
            ),
            (
                PlanOp::Revert,
                "HEAD is unborn (no commits exist). Cannot revert on an empty branch.",
            ),
            (
                PlanOp::Pull,
                "HEAD is unborn (no commits exist). Cannot pull onto an empty branch.",
            ),
            (
                PlanOp::Push,
                "HEAD is unborn (no commits exist). Cannot push an empty branch.",
            ),
            (
                PlanOp::Merge,
                "HEAD is unborn. Cannot merge into an empty branch.",
            ),
        ];
        for (op, want) in cases {
            assert_eq!(CommonNote::HeadUnborn { op }.message_en(), want);
        }
    }

    #[test]
    fn branch_missing_two_tails() {
        assert_eq!(
            CommonNote::BranchMissing {
                name: "feat/x".into(),
                in_repo: true
            }
            .message_en(),
            "Branch 'feat/x' does not exist in this repository."
        );
        assert_eq!(
            CommonNote::BranchMissing {
                name: "feat/x".into(),
                in_repo: false
            }
            .message_en(),
            "Branch 'feat/x' does not exist."
        );
    }

    #[test]
    fn git_error_passthrough() {
        assert_eq!(
            CommonNote::GitErrorPassthrough {
                message: "revspec 'x' not found".into()
            }
            .message_en(),
            "revspec 'x' not found"
        );
    }

    #[test]
    fn branch_name_error_keyed_every_variant() {
        let cases = [
            (
                BranchNameError::EmptyCreate,
                "Branch name must not be empty.",
            ),
            (BranchNameError::Required, "Branch name is required."),
            (
                BranchNameError::Whitespace,
                "Branch name must not start or end with whitespace.",
            ),
            (BranchNameError::SameName, "Branch already has that name."),
            (
                BranchNameError::RenameExists("feat/x".into()),
                "Branch 'feat/x' already exists.",
            ),
            (
                BranchNameError::RenameInvalid("bad name".into()),
                "'bad name' is not a valid branch name.",
            ),
            (
                BranchNameError::CreateInvalidRef("bad..name".into()),
                "Branch name 'bad..name' is not a valid git ref name \
                 (no spaces, '..', or other invalid characters).",
            ),
            (
                BranchNameError::CreateLeadingDash("-feat".into()),
                "Branch name '-feat' must not start with '-'.",
            ),
            (
                BranchNameError::CreateExists("feat/x".into()),
                "A branch named 'feat/x' already exists in this repository.",
            ),
        ];
        for (e, want) in cases {
            assert_eq!(CommonNote::BranchNameErrorKeyed(e).message_en(), want);
        }
    }

    #[test]
    fn worktree_path_error_keyed_every_variant() {
        assert_eq!(
            CommonNote::WorktreePathErrorKeyed(WorktreePathError::Empty).message_en(),
            "Worktree path must not be empty."
        );
        assert_eq!(
            CommonNote::WorktreePathErrorKeyed(WorktreePathError::Exists("/repo/../wt".into()))
                .message_en(),
            "Worktree path '/repo/../wt' already exists."
        );
    }

    #[test]
    fn dirty_stash_first() {
        assert_eq!(
            CommonNote::DirtyStashFirst.message_en(),
            "Working tree is dirty: confirming will stash your changes first (saved to stash@{0}, restore with `git stash pop`)"
        );
    }

    #[test]
    fn merge_conflict_warning() {
        assert_eq!(
            CommonNote::MergeConflictWarning.message_en(),
            "This merge will produce conflicts. It will leave conflict markers and enter Conflict Mode, where you resolve each file (or abort to restore the pre-merge state)."
        );
    }
}
