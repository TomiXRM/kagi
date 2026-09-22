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
    /// Effects of a remote workflow, frozen at approval
    /// (`Backend::remote_expectation`), including PR merge's local cleanup.
    /// Empty for a local-only operation or an effect that cannot be named:
    /// reconciliation stays unresolved rather than guessing.
    ///
    /// A list: one operation can promise several refs, and a reconcile is
    /// confirmed only when **every** one of them is.
    pub remote: Vec<kagi_git::backend::remote_ref::RemoteExpectation>,
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
    /// Registered before the executor starts, so what this job spawns is owned
    /// outside the task that runs it (#703).
    supervision: kagi_git::proc::supervisor::JobId,
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
    /// The completion to settle with if this job's task never returns one.
    pub fn abandonment(&self) -> RunAbandonment {
        RunAbandonment {
            id: self.id,
            supervision: self.supervision,
            name: self.request.name,
            path: self.request.path.clone(),
            before: self.request.plan.current.clone(),
        }
    }
    pub fn run(self) -> RunCompletion {
        // This thread is the job: every group `run_child` spawns from here is
        // registered against it until the run proves that group empty.
        let _supervised = kagi_git::proc::supervisor::enter(self.supervision);
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

/// A run whose task ended without a completion — a panicked job (#289).
///
/// The write may have happened, so this is `Unknown`, not a failure, and it
/// settles through the same `apply`: the operation id survives and the lease
/// is retained. What the termination says is now the supervisor's answer
/// (#703): the groups this job spawned are owned outside the task, so a live
/// one becomes a probeable `Unaccounted` and none at all is a stop proof —
/// the unwound executor thread was the only writer left. Dropping the task
/// instead would lose the operation as well.
pub struct RunAbandonment {
    id: OperationId,
    supervision: kagi_git::proc::supervisor::JobId,
    name: &'static str,
    path: PathBuf,
    before: kagi_git::StateSummary,
}
impl RunAbandonment {
    pub fn into_completion(self) -> RunCompletion {
        let evidence = format!(
            "the {} task unwound; whether the write happened cannot be established",
            self.name
        );
        let repo = self.path.display().to_string();
        let entry = OpLogEntry::new(
            self.name,
            repo.clone(),
            self.before.clone(),
            OpOutcome::Unknown {
                after: self.before,
                evidence: evidence.clone(),
            },
        )
        // The same stamp the backend puts on what it records: an abandoned
        // write still names the worktree it was running in.
        .with_worktree(Some(repo));
        RunCompletion {
            id: self.id,
            report: RunReport {
                result: Err(kagi_git::GitError::TerminationUnknown(
                    kagi_git::Termination::from_abandoned_job(
                        evidence,
                        &kagi_git::proc::supervisor::take_live_groups(self.supervision),
                    ),
                )),
                recording: recording::finalize(entry),
                stash: None,
            },
        }
    }
}

pub fn prepare_run(
    s: &mut Sessions,
    approved: Approved,
    execute: RunExecute,
) -> Result<RunJob, AdmissionError> {
    let Planned::Run(request) = &approved.prepared else {
        return Err(AdmissionError::StaleApproval);
    };
    let request = request.clone();
    let running = begin_write(s, &approved)?;
    Ok(RunJob {
        id: running.operation_id,
        stamp: running.owner_stamp,
        supervision: kagi_git::proc::supervisor::begin(),
        request,
        execute,
    })
}
