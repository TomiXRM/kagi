//! Operation plan, validation, and outcome domain models -- pure data, no git2.
//!
//! The git2-backed functions that produce and execute plans live in the
//! git-backend layer (`kagi::git::ops`).

use crate::commit::CommitId;

/// One-line summary of repository state for display in the plan modal.
///
/// Example: `head = "branch: main"`, `dirty = "1 modified, 1 untracked"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSummary {
    /// Description of HEAD, e.g. `"branch: main"` or `"detached: a1b2c3d4"`.
    pub head: String,
    /// Description of working-tree cleanliness, e.g. `"clean"` or
    /// `"1 staged, 2 modified, 3 untracked"`.
    pub dirty: String,
}

/// Keyed, user-facing reason why a branch name is rejected (W29-I18N-WAVE2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchNameError {
    /// create-branch: name is empty. -> "Branch name must not be empty."
    EmptyCreate,
    /// rename-branch: name is empty/blank. -> "Branch name is required."
    Required,
    /// rename-branch: leading/trailing whitespace.
    Whitespace,
    /// rename-branch: new name equals the old name.
    SameName,
    /// rename-branch: a branch with this name already exists.
    RenameExists(String),
    /// rename-branch: not a valid git ref name.
    RenameInvalid(String),
    /// create-branch: not a valid git ref name.
    CreateInvalidRef(String),
    /// create-branch: name starts with `-`.
    CreateLeadingDash(String),
    /// create-branch: a branch with this name already exists.
    CreateExists(String),
}

impl std::fmt::Display for BranchNameError {
    /// Exact current English wording -- do not change without updating tests.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BranchNameError::EmptyCreate => write!(f, "Branch name must not be empty."),
            BranchNameError::Required => write!(f, "Branch name is required."),
            BranchNameError::Whitespace => {
                write!(f, "Branch name must not start or end with whitespace.")
            }
            BranchNameError::SameName => write!(f, "Branch already has that name."),
            BranchNameError::RenameExists(name) => write!(f, "Branch '{}' already exists.", name),
            BranchNameError::RenameInvalid(name) => {
                write!(f, "'{}' is not a valid branch name.", name)
            }
            BranchNameError::CreateInvalidRef(name) => write!(
                f,
                "Branch name '{}' is not a valid git ref name \
                 (no spaces, '..', or other invalid characters).",
                name
            ),
            BranchNameError::CreateLeadingDash(name) => {
                write!(f, "Branch name '{}' must not start with '-'.", name)
            }
            BranchNameError::CreateExists(name) => {
                write!(
                    f,
                    "A branch named '{}' already exists in this repository.",
                    name
                )
            }
        }
    }
}

/// Keyed, user-facing reason why a worktree path is rejected (W29-I18N-WAVE2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreePathError {
    /// Path was empty.
    Empty,
    /// The target path already exists (carries the display path).
    Exists(String),
}

impl std::fmt::Display for WorktreePathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorktreePathError::Empty => write!(f, "Worktree path must not be empty."),
            WorktreePathError::Exists(path) => {
                write!(f, "Worktree path '{}' already exists.", path)
            }
        }
    }
}

/// Result of pure branch rename input validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchRenameValidation {
    Valid,
    /// Rejected -- carries the keyed reason.
    Invalid(BranchNameError),
}

/// A worktree-path validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeValidationError {
    /// One of the two reasons the UI localizes (empty / already exists).
    Keyed(WorktreePathError),
    /// Any other reason -- stays English-only.
    Other(String),
}

impl std::fmt::Display for WorktreeValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorktreeValidationError::Keyed(e) => write!(f, "{}", e),
            WorktreeValidationError::Other(s) => write!(f, "{}", s),
        }
    }
}

/// What a planned merge will actually do once executed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeKind {
    /// HEAD is an ancestor of target: the branch ref simply fast-forwards.
    FastForward,
    /// Diverged but mergeable: a merge commit is created.
    MergeCommit,
    /// Diverged and the in-memory merge predicts conflicts in these file(s).
    Conflicts(Vec<String>),
}

/// The outcome of a successful pull.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullOutcome {
    /// The local branch was already at or ahead of the upstream tip.
    UpToDate,
    /// The upstream was a direct ancestor of HEAD.
    FastForward {
        /// The new HEAD commit SHA (the upstream tip).
        to: CommitId,
    },
    /// A true merge was performed.
    Merged {
        /// The new merge-commit SHA.
        commit: CommitId,
    },
}

/// Result of a fetch: which remote was fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchOutcome {
    /// The remote name that was fetched, or "--all" when every remote was fetched.
    pub remote: String,
    /// Whether the fetch actually moved any remote-tracking ref (a ref was
    /// added, removed, or repointed). `false` means the fetch was a no-op, so
    /// callers can skip the expensive graph reload it would otherwise trigger.
    pub changed: bool,
}

/// The outcome of a successful push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushOutcome {
    /// Number of commits that were in the push plan.
    pub pushed: usize,
    /// Whether `--set-upstream` (`-u`) was passed.
    pub set_upstream: bool,
}

/// The outcome of a successful undo-commit call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoOutcome {
    /// The commit that was taken off the branch tip.
    pub undone: CommitId,
    /// The new branch tip.
    pub now_at: CommitId,
}

/// Which parts of the HEAD commit an amend should rewrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmendMode {
    /// Replace only the commit message; the tree is the old HEAD tree.
    MessageOnly,
    /// Fold the current staged changes into the old HEAD tree; message kept.
    Staged,
    /// Fold staged changes and replace the message.
    Both,
}

impl AmendMode {
    /// Parse the `KAGI_AMEND=<mode>` headless value (`message` / `staged` / `both`).
    pub fn from_env_str(s: &str) -> Option<AmendMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "message" | "message-only" | "messageonly" | "msg" => Some(AmendMode::MessageOnly),
            "staged" => Some(AmendMode::Staged),
            "both" => Some(AmendMode::Both),
            _ => None,
        }
    }

    /// Whether this mode folds the staged index into the new tree.
    pub fn includes_staged(self) -> bool {
        matches!(self, AmendMode::Staged | AmendMode::Both)
    }

    /// Whether this mode replaces the commit message.
    pub fn replaces_message(self) -> bool {
        matches!(self, AmendMode::MessageOnly | AmendMode::Both)
    }
}

/// The outcome of a successful amend call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmendOutcome {
    /// The commit that was amended.
    pub old: CommitId,
    /// The new branch tip created by the amend.
    pub new: CommitId,
}

/// One backed-up working-tree file recorded before a discard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardBackup {
    /// Repository-relative path that was discarded.
    pub path: String,
    /// ODB blob SHA (40-hex) holding the pre-discard working-tree content.
    pub blob: String,
}

/// Outcome of a discard: the backup blobs written before discarding.
#[derive(Debug, Clone)]
pub struct DiscardOutcome {
    /// One entry per discarded file, in plan order.
    pub backups: Vec<DiscardBackup>,
}

impl DiscardOutcome {
    /// Render the path/blob backup list as a single oplog-friendly summary line.
    pub fn oplog_summary(&self) -> String {
        let pairs: Vec<String> = self
            .backups
            .iter()
            .map(|b| format!("{}={}", b.path, b.blob))
            .collect();
        format!(
            "discarded {} file(s); backup: {}",
            self.backups.len(),
            pairs.join(", ")
        )
    }
}

// ──────────────────────────────────────────────────────────────
// OperationPlan (ADR-0129 Phase 1: moved here from kagi-git::ops;
// kagi-git re-exports it per the naming convention)
// ──────────────────────────────────────────────────────────────

use crate::head::Head;
use crate::plan_note::{PlanDisposition, PlanNote, PlanRecovery, PlanTitle};
use crate::status::FileStatus;

/// A complete plan describing what an operation will do, including
/// any blockers that prevent execution and warnings that should be surfaced.
///
/// If `blockers` is non-empty the UI **must not** offer the Execute button.
///
/// ADR-0129: `title`/`warnings`/`blockers`/`recovery` are structured values
/// localized by the display layer; `disposition` carries the semantic state
/// the UI used to infer by parsing display strings.
#[derive(Debug, Clone)]
pub struct OperationPlan {
    /// The plan modal's title (one, required).
    pub title: PlanTitle,
    /// Repository state *before* the operation.
    pub current: StateSummary,
    /// Predicted repository state *after* the operation.
    pub predicted: StateSummary,
    /// Non-fatal observations (shown in yellow).  The operation can still
    /// proceed if there are warnings but no blockers.
    pub warnings: Vec<PlanNote>,
    /// Conditions that prevent execution (shown in red).  At least one blocker
    /// means the Execute button must be hidden.
    pub blockers: Vec<PlanNote>,
    /// Recovery guidance shown to the user before they confirm.
    pub recovery: Option<PlanRecovery>,
    /// Semantic plan state (ADR-0129 §2): Ready / NoOp(kind) / Blocked.
    /// Producers set NoOp explicitly; string-parsing no-op detection is gone.
    pub disposition: PlanDisposition,
    /// The HEAD state captured *at plan time*, used by `preflight_check` to
    /// detect whether the repo has changed between planning and execution.
    /// (Was `pub(crate)` in kagi-git; public since the struct crossed crates —
    /// treat as read-only outside the git layer.)
    pub head_at_plan: Head,
    /// Number of stash entries captured at plan time.  Used by
    /// `preflight_check_stash` to detect concurrent stash modifications.
    /// For non-stash operations this is always `0`.
    pub stash_count_at_plan: usize,
    /// Files that will be changed by the operation, as computed by an in-memory
    /// dry run.  Non-empty only for cherry-pick plans.  Used by the plan modal
    /// to render a preview file tree (T016).
    pub preview_files: Vec<FileStatus>,
    /// Commits that will be pushed, as `"<short>  <summary>"` strings.
    /// Non-empty only for push plans (T-HT-004).  Shown in the plan modal
    /// (newest first, capped at 100 entries at plan time).
    pub preview_commits: Vec<String>,
    /// History-rewriting flag (ADR-0023).  `true` for plans that rewrite
    /// history (e.g. amend), which the UI must gate behind a **two-stage
    /// confirmation**.  Defaults to `false` for every other plan.
    pub destructive: bool,
}

impl OperationPlan {
    /// Return the stash entry count captured at plan time.
    ///
    /// Pass this value to `preflight_check_stash` to verify that the stash
    /// list has not changed since the plan was generated.
    pub fn stash_count_at_plan(&self) -> usize {
        self.stash_count_at_plan
    }
}
