//! The generic run-pipeline family (ADR-0196 Wave 3).
//!
//! Every legacy write that goes through `Backend::run_recorded` — checkout,
//! commit, merge, push, … — is one family here: the plan was made by the
//! modal against the tab's `RepoSession`, so there is no plan slot to earn a
//! token from. [`approve_run`] is the admission entry that stands in for
//! `approve`: it freezes the owner and the write scope, and hands back the
//! `Approved` that [`begin_write`](super::begin_write) consumes exactly once.
//! The job is the family's own blocking core; its receipt is the record.
use super::*;
use kagi_git::backend::recording::{self, RunReport};
use kagi_git::oplog::{OpLogEntry, OpOutcome};
use kagi_git::OperationPlan;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct RunRequest {
    /// The tab that approved the write. Frozen: completion is routed to it.
    pub owner: Attachment,
    /// The oplog `op` name; also the busy label.
    pub name: &'static str,
    /// The repository the write lands in. Usually `owner.path`; a commit panel
    /// showing a linked worktree writes there instead (#476).
    pub path: PathBuf,
    /// Frozen at approval — the lease scope.
    pub repo: RepoId,
    pub plan: Arc<OperationPlan>,
    /// What this write is about to make true on a remote, frozen here at
    /// approval (`Backend::remote_expectation`). `None` for a local-only
    /// operation, and for a remote one whose effect cannot be named — which
    /// leaves the reconcile read unresolved rather than guessing.
    pub remote: Option<kagi_git::backend::remote_ref::RemoteExpectation>,
}

/// Admission for a plan that lives in a modal rather than the plan slot: the
/// owner must still be attached and still be the worktree it froze at attach
/// time. The slot is re-issued so a migrated family's pending plan can never
/// be spent across this write.
pub fn approve_run(s: &mut Sessions, request: RunRequest) -> Result<Approved, AdmissionError> {
    let owner = request.owner.clone();
    approve_modal_plan(s, owner, Planned::Run(request))
}

/// The admission [`approve_run`] and [`approve_pull`](super::approve_pull)
/// share: both families plan in their modal, so neither has a plan token.
pub(crate) fn approve_modal_plan(
    s: &mut Sessions,
    owner: Attachment,
    prepared: Planned,
) -> Result<Approved, AdmissionError> {
    if !s.is_attached(owner.session) {
        return Err(AdmissionError::StaleApproval);
    }
    s.confirm_identity(&owner)?;
    s.invalidate_plan();
    s.plan_owner = Some(owner.session);
    s.state = PlanState::Approved;
    Ok(Approved {
        revision: s.revision,
        prepared,
    })
}

/// The family's blocking core, already bound to its arguments. `Err` means the
/// repository would not open: nothing ran, and the job records that itself so
/// every completion carries a receipt.
pub type RunExecute = Box<dyn FnOnce() -> Result<RunReport, String> + Send + 'static>;

pub struct RunJob {
    id: OperationId,
    stamp: OwnerStamp,
    request: RunRequest,
    execute: RunExecute,
}
impl RunJob {
    pub fn id(&self) -> OperationId {
        self.id
    }
    /// The owner frozen at admission; the completion is routed by it.
    pub fn stamp(&self) -> OwnerStamp {
        self.stamp
    }
    pub fn run(self) -> RunCompletion {
        let report = match (self.execute)() {
            Ok(report) => report,
            Err(error) => {
                let entry = OpLogEntry::new(
                    self.request.name,
                    self.request.path.display().to_string(),
                    self.request.plan.current.clone(),
                    OpOutcome::Failed {
                        error: error.clone(),
                    },
                );
                RunReport {
                    result: Err(kagi_git::GitError::Other(error)),
                    recording: recording::finalize(entry),
                    stash: None,
                }
            }
        };
        RunCompletion {
            id: self.id,
            report,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RunCompletion {
    pub id: OperationId,
    pub report: RunReport,
}

pub fn prepare_run(
    s: &mut Sessions,
    approved: Approved,
    legacy: LegacyBusy,
    execute: RunExecute,
) -> Result<RunJob, AdmissionError> {
    let Planned::Run(request) = &approved.prepared else {
        return Err(AdmissionError::StaleApproval);
    };
    let request = request.clone();
    let running = begin_write(s, &approved, legacy)?;
    Ok(RunJob {
        id: running.operation_id,
        stamp: running.owner_stamp,
        request,
        execute,
    })
}
