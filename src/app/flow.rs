//! Shared finite approval state; family executors remain owned jobs.
use super::*;
use kagi_git::backend::{recording::Recording, remove::RemovePlan, stash::StashPlan};

#[derive(Clone, Debug)]
pub enum Planned {
    Remove {
        plan: RemovePlan,
        request: RemoveRequest,
        policy: RemovePolicy,
    },
    Stash {
        plan: StashPlan,
        request: StashRequest,
        policy: StashPolicy,
    },
    RemoteStash {
        plan: Box<crate::remote::stash::RemoteStashPlan>,
        request: RemoteStashRequest,
        policy: StashPolicy,
    },
}
impl Planned {
    pub fn owner(&self) -> OwnerAttachment {
        match self {
            Self::Remove { request, .. } => OwnerAttachment::Local(request.owner.clone()),
            Self::Stash { request, .. } => OwnerAttachment::Local(request.owner.clone()),
            Self::RemoteStash { request, .. } => OwnerAttachment::Remote(request.owner.clone()),
        }
    }
    pub fn scope(&self) -> WriteScope {
        match self {
            Self::Remove { plan, .. } => WriteScope::Local(plan.common_dir.clone()),
            Self::Stash { plan, .. } => WriteScope::Local(plan.common_dir.clone()),
            Self::RemoteStash { plan, .. } => WriteScope::Remote(plan.repo_id.clone()),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Policy {
    Remove(RemovePolicy),
    Stash(StashPolicy),
}
impl From<RemovePolicy> for Policy {
    fn from(p: RemovePolicy) -> Self {
        Self::Remove(p)
    }
}
impl From<StashPolicy> for Policy {
    fn from(p: StashPolicy) -> Self {
        Self::Stash(p)
    }
}
#[derive(Clone, Debug)]
pub struct PlanToken {
    pub(crate) revision: RequestId,
}
#[derive(Clone, Debug)]
pub enum PlanState {
    Draft,
    Planning {
        request: RequestId,
    },
    Ready {
        token: PlanToken,
        prepared: Planned,
    },
    Error {
        error: String,
        open_failed: bool,
        recording: Option<Recording>,
    },
    Approved,
}
#[derive(Clone)]
pub struct PlanCompletion {
    pub(crate) revision: RequestId,
    pub(crate) state: PlanState,
    pub(crate) error_job: Option<PlanErrorEvidence>,
}
impl PlanCompletion {
    pub fn is_current(&self, sessions: &Sessions) -> bool {
        self.revision == sessions.revision
    }
}
#[derive(Clone)]
pub(crate) struct PlanErrorEvidence {
    pub(crate) revision: RequestId,
    pub(crate) request: StashRequest,
    pub(crate) policy: StashPolicy,
    pub(crate) error: String,
}
pub struct PlanErrorJob(PlanErrorEvidence);
impl PlanErrorJob {
    pub fn run(self) -> PlanErrorCompletion {
        let evidence = self.0;
        let recording = kagi_git::backend::stash::record_stash_plan_error(
            &evidence.request.owner.path,
            evidence.policy.actor,
            &evidence.request.action,
            &evidence.error,
        );
        PlanErrorCompletion {
            revision: evidence.revision,
            recording,
        }
    }
}
pub struct PlanErrorCompletion {
    revision: RequestId,
    pub recording: Recording,
}
pub fn apply_plan(s: &mut Sessions, c: PlanCompletion) -> bool {
    if s.revision != c.revision || !matches!(s.state, PlanState::Planning { .. }) {
        return false;
    }
    s.state = c.state;
    if let Some(evidence) = c.error_job {
        s.plan_errors.push(PlanErrorJob(evidence));
    }
    true
}
impl Sessions {
    pub fn take_plan_error_jobs(&mut self) -> Vec<PlanErrorJob> {
        std::mem::take(&mut self.plan_errors)
    }
    pub fn apply_plan_error(&mut self, completion: PlanErrorCompletion) {
        if self.revision == completion.revision {
            if let PlanState::Error { recording, .. } = &mut self.state {
                *recording = Some(completion.recording);
            }
        }
    }
}
pub struct Approved {
    pub(crate) revision: RequestId,
    pub(crate) prepared: Planned,
}
pub fn approve(
    s: &mut Sessions,
    token: PlanToken,
    policy: impl Into<Policy>,
) -> Result<Approved, AdmissionError> {
    let PlanState::Ready {
        token: current,
        prepared,
    } = &s.state
    else {
        return Err(AdmissionError::StaleApproval);
    };
    let planned_policy = match prepared {
        Planned::Remove { policy, .. } => Policy::Remove(policy.clone()),
        Planned::Stash { policy, .. } => Policy::Stash(*policy),
        Planned::RemoteStash { policy, .. } => Policy::Stash(*policy),
    };
    if current.revision != token.revision || planned_policy != policy.into() {
        return Err(AdmissionError::StaleApproval);
    }
    let approved = Approved {
        revision: token.revision,
        prepared: prepared.clone(),
    };
    s.state = PlanState::Approved;
    Ok(approved)
}
pub(crate) fn reserve(
    s: &mut Sessions,
    approved: &Approved,
    legacy: LegacyBusy,
) -> Result<OperationId, AdmissionError> {
    if approved.revision != s.revision || !matches!(s.state, PlanState::Approved) {
        return Err(AdmissionError::StaleApproval);
    }
    if legacy.0 {
        return Err(AdmissionError::Busy);
    }
    let scope = approved.prepared.scope();
    if s.reconcile
        .values()
        .any(|entry| entry.plan.scope() == scope)
    {
        return Err(AdmissionError::NeedsReconcile);
    }
    if s.has_leases() {
        return Err(AdmissionError::Busy);
    }
    let id = OperationId(next_id());
    s.reserve_lease(scope, id)?;
    s.operations.insert(
        id,
        InFlight {
            plan: approved.prepared.clone(),
            attachment: approved.prepared.owner(),
        },
    );
    s.invalidate_plan();
    Ok(id)
}
#[derive(Clone, Debug)]
pub enum Completion {
    Remove(Box<RemoveCompletion>),
    Stash(Box<StashCompletion>),
}
impl From<RemoveCompletion> for Completion {
    fn from(c: RemoveCompletion) -> Self {
        Self::Remove(Box::new(c))
    }
}
impl From<StashCompletion> for Completion {
    fn from(c: StashCompletion) -> Self {
        Self::Stash(Box::new(c))
    }
}
#[derive(Clone, Debug)]
pub enum FamilyEvidence {
    Remove(kagi_git::backend::remove::RemoveReport),
    Stash(kagi_git::backend::stash::StashReport),
    RemoteStash(crate::remote::stash::RemoteStashReport),
}
#[derive(Clone, Debug)]
pub struct ExecutionReport {
    pub recording: Recording,
    pub evidence: FamilyEvidence,
}
pub enum Job {
    Remove(RemoveJob),
    Stash(StashJob),
}
pub enum Event {
    Remove(kagi_git::backend::remove::RemoveEvent),
    Stash(kagi_git::backend::stash::StashEvent),
}
impl Job {
    pub fn run(self) -> Completion {
        self.run_with_events(|_| {})
    }
    pub fn run_with_events(self, mut event: impl FnMut(Event)) -> Completion {
        match self {
            Self::Remove(job) => job.run_with_events(|e| event(Event::Remove(e))).into(),
            Self::Stash(job) => job.run_with_events(|e| event(Event::Stash(e))).into(),
        }
    }
}
pub fn prepare(
    s: &mut Sessions,
    approved: Approved,
    legacy: LegacyBusy,
) -> Result<Job, AdmissionError> {
    match &approved.prepared {
        Planned::Remove { .. } => prepare_remove(s, approved, legacy).map(Job::Remove),
        Planned::Stash { .. } => prepare_stash(s, approved, legacy).map(Job::Stash),
        Planned::RemoteStash { .. } => prepare_stash(s, approved, legacy).map(Job::Stash),
    }
}
pub fn apply(s: &mut Sessions, completion: impl Into<Completion>) -> Vec<Delivery> {
    let (id, report, stopped, remote_recovery) = match completion.into() {
        Completion::Remove(c) => {
            let c = *c;
            let stopped = !c.report.progress.termination_unknown;
            (
                c.id,
                ExecutionReport {
                    recording: c.report.recording.clone(),
                    evidence: FamilyEvidence::Remove(c.report),
                },
                stopped,
                None,
            )
        }
        Completion::Stash(c) => match c.report {
            StashExecutionReport::Local(report) => (
                c.id,
                ExecutionReport {
                    recording: report.recording.clone(),
                    evidence: FamilyEvidence::Stash(*report),
                },
                true,
                None,
            ),
            StashExecutionReport::Remote(report) => {
                let stopped = report.evidence.stopped;
                let recovery = Some(report.evidence.clone());
                (
                    c.id,
                    ExecutionReport {
                        recording: report.recording.clone(),
                        evidence: FamilyEvidence::RemoteStash(*report),
                    },
                    stopped,
                    recovery,
                )
            }
        },
    };
    if s.settled.contains(&id) {
        return vec![];
    }
    let Some(owner) = s.operations.remove(&id) else {
        return vec![];
    };
    s.settled.insert(id);
    if stopped {
        s.release_lease(&owner.plan.scope(), id);
    }
    if matches!(
        report.recording.entry().outcome,
        kagi_git::OpOutcome::Unknown { .. }
    ) {
        s.reconcile.insert(
            id,
            ReconcileEntry {
                plan: owner.plan.clone(),
                stopped,
                remote: remote_recovery,
            },
        );
    }
    let mut deliveries = vec![];
    match (&owner.plan, &report.evidence) {
        (Planned::Remove { plan, .. }, FamilyEvidence::Remove(r)) => {
            for path in [&plan.repo, &plan.target] {
                s.stale.insert(path.clone());
                deliveries.push(if path == &plan.target && r.target_exists == Some(false) {
                    Delivery::RemovedTarget(path.clone())
                } else {
                    Delivery::Invalidate(path.clone())
                });
            }
        }
        (Planned::Stash { plan, .. }, FamilyEvidence::Stash(r)) => {
            if matches!(
                plan.action,
                StashAction::Apply { .. } | StashAction::Pop { .. }
            ) && !r.evidence.conflicts.is_empty()
            {
                if let Some(oid) = &r.evidence.oid {
                    s.clear_stash_conflict(&plan.repo);
                    s.stash_conflicts.insert(
                        plan.repo.clone(),
                        StashConflict {
                            operation: id,
                            oid: oid.clone(),
                            identity: r.evidence.conflict_identity.clone(),
                            pending: false,
                        },
                    );
                }
            }
            s.stale.insert(plan.repo.clone());
            deliveries.push(Delivery::Invalidate(plan.repo.clone()));
        }
        (Planned::RemoteStash { .. }, FamilyEvidence::RemoteStash(_)) => {}
        _ => unreachable!("completion family is fixed by its owned job"),
    }
    deliveries.push(match owner.attachment {
        OwnerAttachment::Local(attachment) => Delivery::Completed {
            id,
            attachment,
            report: Box::new(report),
        },
        OwnerAttachment::Remote(attachment) => Delivery::RemoteCompleted {
            id,
            attachment,
            report: Box::new(report),
        },
    });
    deliveries
}
