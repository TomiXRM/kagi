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
