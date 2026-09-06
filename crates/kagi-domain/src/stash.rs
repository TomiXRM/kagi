//! Pure stash requests and execution observations.
use crate::operation::Operation;
use crate::plan as ops;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StashAction {
    Push {
        message: Option<String>,
        include_untracked: bool,
    },
    Apply {
        index: usize,
    },
    Pop {
        index: usize,
    },
    Drop {
        index: usize,
    },
}
impl StashAction {
    pub fn operation(&self) -> Operation {
        match self {
            Self::Push {
                message,
                include_untracked,
            } => Operation::StashPush {
                message: message.clone(),
                include_untracked: *include_untracked,
            },
            Self::Apply { index } => Operation::StashApply { index: *index },
            Self::Pop { index } => Operation::StashPop { index: *index },
            Self::Drop { index } => Operation::StashDrop { index: *index },
        }
    }
    pub fn name(&self) -> &'static str {
        self.operation().oplog_name()
    }
    pub fn from_operation(op: &Operation) -> Option<Self> {
        Some(match op {
            Operation::StashPush {
                message,
                include_untracked,
            } => Self::Push {
                message: message.clone(),
                include_untracked: *include_untracked,
            },
            Operation::StashApply { index } => Self::Apply { index: *index },
            Operation::StashPop { index } => Self::Pop { index: *index },
            Operation::StashDrop { index } => Self::Drop { index: *index },
            _ => return None,
        })
    }
}
#[derive(Clone, Debug, Default)]
pub struct StashEvidence {
    pub started: bool,
    pub applied: bool,
    pub verified: bool,
    pub unknown: bool,
    pub oid: Option<String>,
    pub conflicts: Vec<String>,
    pub conflict_identity: Vec<String>,
    pub conflict_identity_before: Vec<String>,
    pub after: Option<ops::StateSummary>,
    pub observations: Vec<String>,
    pub worktree_before: Option<u64>,
    pub untracked_before: Vec<std::path::PathBuf>,
    pub snapshot: Option<String>,
    pub plan_blocked: bool,
    pub preflight_error: Option<String>,
    pub stop: Option<StashStopReason>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StashStopReason {
    OpenFailed,
    IdentityChanged,
    Untrusted,
    PlanBlocked,
    Preflight,
    Abandoned,
}
#[derive(Clone, Copy, Debug)]
pub enum StashFaultPoint {
    Untrusted,
    BeforeMutation,
    AfterMutation,
    VerifyFailure,
    ListChangeBeforeDrop,
}
#[derive(Clone, Debug)]
pub enum StashEvent {
    Started,
    PlanBlocked,
    VerifyFailed {
        error: String,
    },
    Executed {
        action: StashAction,
        oid: Option<String>,
        conflicts: usize,
    },
    Verified {
        dirty: bool,
        count: usize,
    },
}
