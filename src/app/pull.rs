//! The pull-workflow family (ADR-0196 Wave 3).
//!
//! Pull is the one write that is not one write: a dirty pull runs
//! StashPush → Pull → StashPop, and each child already writes its own durable
//! receipt. So the family carries every receipt it produced
//! ([`PullReport::steps`], in execution order) plus the one that *decided* the
//! workflow ([`PullTerminal::decisive`]) — which is not always the last: a pull
//! that failed and whose stash was then restored successfully is a failed pull,
//! not a successful pop. [`apply`](super::apply) settles on the decisive
//! receipt and the UI presents it.
//!
//! Nothing here synthesizes an oplog entry. A workflow that refuses before any
//! child could run still returns a recorded no-execute step, produced by the
//! blocking core.
use super::*;
use kagi_git::backend::recording::{self, RunReport};
use kagi_git::oplog::{OpLogEntry, OpOutcome};
use kagi_git::OperationPlan;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct PullRequest {
    /// The tab that approved the write. Frozen: completion is routed to it.
    pub owner: Attachment,
    /// The oplog `op` name; also the busy label. Always `"pull"`.
    pub name: &'static str,
    pub path: PathBuf,
    /// Frozen at approval — the lease scope.
    pub repo: RepoId,
    pub plan: Arc<OperationPlan>,
    /// The confirmed auto-stash: the workflow may run StashPush/StashPop.
    pub auto_stash: bool,
    /// The dirty set the confirmation named (#625 / ADR-0192). The blocking
    /// core refuses before stashing if it moved; a reconcile compares the
    /// working tree it finds against it.
    pub promised_dirty: Option<kagi_domain::status::WorktreeDigest>,
}

/// What the UI does with a finished workflow. `Partial` means the pull or the
/// stash restoration changed repository state but the confirmed workflow did
/// not finish.
#[derive(Clone, Debug)]
pub enum PullPresentation {
    Success { summary: String },
    Failed { error: String },
    Partial { error: String },
}

/// The step the workflow settles on, plus what the UI shows for it.
#[derive(Clone, Debug)]
pub struct PullTerminal {
    /// The decisive receipt — one of [`PullReport::steps`].
    pub decisive: RunReport,
    pub presentation: PullPresentation,
    /// An auto-stash this workflow created and did **not** restore: the
    /// recovery context a reconcile needs. A pull whose termination is
    /// unconfirmed never has its stash popped on the user's behalf.
    pub stash_oid: Option<String>,
}

/// Every receipt the pull workflow produced, in execution order.
#[derive(Clone, Debug)]
pub struct PullReport {
    pub steps: Vec<RunReport>,
    pub terminal: PullTerminal,
}
impl PullReport {
    /// Settle on `decisive`, which the caller took from `steps`.
    pub fn new(
        steps: Vec<RunReport>,
        decisive: RunReport,
        presentation: PullPresentation,
        stash_oid: Option<String>,
    ) -> Self {
        Self {
            steps,
            terminal: PullTerminal {
                decisive,
                presentation,
                stash_oid,
            },
        }
    }
    /// Settle on the last step run — the common case.
    ///
    /// # Panics
    /// `steps` must not be empty: every exit from the workflow records at
    /// least one receipt, including the ones that start no child.
    pub fn settled(
        steps: Vec<RunReport>,
        presentation: PullPresentation,
        stash_oid: Option<String>,
    ) -> Self {
        let decisive = steps
            .last()
            .expect("a pull workflow records at least one step")
            .clone();
        Self::new(steps, decisive, presentation, stash_oid)
    }
}

/// Admission for the pull workflow. Same rung as [`approve_run`]: the plan
/// lives in the confirmation modal, so there is no plan slot to spend.
pub fn approve_pull(s: &mut Sessions, request: PullRequest) -> Result<Approved, AdmissionError> {
    let owner = request.owner.clone();
    approve_modal_plan(s, owner, Planned::Pull(request))
}

/// The blocking core, already bound to its arguments. `Err` means the
/// repository would not open: nothing ran, and the job records that itself.
pub type PullExecute = Box<dyn FnOnce() -> Result<PullReport, String> + Send + 'static>;

pub struct PullJob {
    id: OperationId,
    stamp: OwnerStamp,
    request: PullRequest,
    execute: PullExecute,
}
impl PullJob {
    pub fn id(&self) -> OperationId {
        self.id
    }
    /// The owner frozen at admission; the completion is routed by it.
    pub fn stamp(&self) -> OwnerStamp {
        self.stamp
    }
    pub fn run(self) -> PullCompletion {
        let report = (self.execute)().unwrap_or_else(|error| {
            let entry = OpLogEntry::new(
                self.request.name,
                self.request.path.display().to_string(),
                self.request.plan.current.clone(),
                OpOutcome::Failed {
                    error: error.clone(),
                },
            );
            PullReport::settled(
                vec![RunReport {
                    result: Err(kagi_git::GitError::Other(error.clone())),
                    recording: recording::finalize(entry),
                    stash: None,
                }],
                PullPresentation::Failed { error },
                None,
            )
        });
        PullCompletion {
            id: self.id,
            report,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PullCompletion {
    pub id: OperationId,
    pub report: PullReport,
}

pub fn prepare_pull(
    s: &mut Sessions,
    approved: Approved,
    legacy: LegacyBusy,
    execute: PullExecute,
) -> Result<PullJob, AdmissionError> {
    let Planned::Pull(request) = &approved.prepared else {
        return Err(AdmissionError::StaleApproval);
    };
    let request = request.clone();
    let running = begin_write(s, &approved, legacy)?;
    Ok(PullJob {
        id: running.operation_id,
        stamp: running.owner_stamp,
        request,
        execute,
    })
}
