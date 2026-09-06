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
}
impl Planned {
    pub fn owner(&self) -> &Attachment {
        match self {
            Self::Remove { request, .. } => &request.owner,
            Self::Stash { request, .. } => &request.owner,
        }
    }
    pub fn common_dir(&self) -> &RepoId {
        match self {
            Self::Remove { plan, .. } => &plan.common_dir,
            Self::Stash { plan, .. } => &plan.common_dir,
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
    if legacy.0 || s.has_leases() {
        return Err(AdmissionError::Busy);
    }
    let repo = approved.prepared.common_dir();
    if s.reconcile.values().any(|(p, _)| p.common_dir() == repo) {
        return Err(AdmissionError::NeedsReconcile);
    }
    let id = OperationId(next_id());
    s.reserve_lease(repo.clone(), id)?;
    s.operations.insert(
        id,
        InFlight {
            plan: approved.prepared.clone(),
            attachment: approved.prepared.owner().clone(),
        },
    );
    s.invalidate_plan();
    Ok(id)
}
#[derive(Clone, Debug)]
pub enum Completion {
    Remove(RemoveCompletion),
    Stash(StashCompletion),
}
impl From<RemoveCompletion> for Completion {
    fn from(c: RemoveCompletion) -> Self {
        Self::Remove(c)
    }
}
impl From<StashCompletion> for Completion {
    fn from(c: StashCompletion) -> Self {
        Self::Stash(c)
    }
}
#[derive(Clone, Debug)]
pub enum FamilyEvidence {
    Remove(kagi_git::backend::remove::RemoveReport),
    Stash(kagi_git::backend::stash::StashReport),
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
    }
}
pub fn apply(s: &mut Sessions, completion: impl Into<Completion>) -> Vec<Delivery> {
    let (id, report, stopped) = match completion.into() {
        Completion::Remove(c) => {
            let stopped = !c.report.progress.termination_unknown;
            (
                c.id,
                ExecutionReport {
                    recording: c.report.recording.clone(),
                    evidence: FamilyEvidence::Remove(c.report),
                },
                stopped,
            )
        }
        Completion::Stash(c) => (
            c.id,
            ExecutionReport {
                recording: c.report.recording.clone(),
                evidence: FamilyEvidence::Stash(c.report),
            },
            true,
        ),
    };
    if s.settled.contains(&id) {
        return vec![];
    }
    let Some(owner) = s.operations.remove(&id) else {
        return vec![];
    };
    s.settled.insert(id);
    if stopped {
        s.release_lease(owner.plan.common_dir(), id);
    }
    if matches!(
        report.recording.entry().outcome,
        kagi_git::OpOutcome::Unknown { .. }
    ) {
        s.reconcile.insert(id, (owner.plan.clone(), stopped));
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
        _ => unreachable!("completion family is fixed by its owned job"),
    }
    deliveries.push(Delivery::Completed {
        id,
        attachment: owner.attachment,
        report: Box::new(report),
    });
    deliveries
}

/// A modal's plan before it has earned an execution token: the plan computed
/// for the current input, or the explicit failure that replaced it.
///
/// This is the tokenless rung of the same ladder as [`PlanState`]: modals that
/// plan synchronously against the per-tab `RepoSession` have no `RequestId` and
/// no [`PlanToken`], but they need the same guarantee — a (re)plan that fails
/// **replaces** the plan it was recomputing instead of leaving it behind
/// (#510). `Failed` carries no plan, so [`PlanSlot::plan`] — the only way a
/// confirm path reaches one — returns `None`, and both Enter and the confirm
/// button refuse. A later successful replan puts the slot back in `Ready`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PlanSlot<P> {
    /// No plan yet: the modal just opened, or the first plan is still running.
    #[default]
    Pending,
    /// The plan for the current input. The only confirmable state.
    Ready(P),
    /// The last (re)plan failed. Any previous plan is gone, not merely hidden.
    Failed(String),
}

impl<P> PlanSlot<P> {
    /// Adopt one (re)plan result, whichever way it went.
    pub fn replan(&mut self, result: Result<P, impl std::fmt::Display>) {
        *self = match result {
            Ok(plan) => Self::Ready(plan),
            Err(error) => Self::Failed(error.to_string()),
        };
    }
    /// The confirmable plan. `None` while pending or failed.
    pub fn plan(&self) -> Option<&P> {
        match self {
            Self::Ready(plan) => Some(plan),
            _ => None,
        }
    }
    /// The plan failure to render. `None` unless the last (re)plan failed.
    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Failed(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod plan_slot_tests {
    use super::PlanSlot;

    #[test]
    fn failure_invalidates_the_plan_it_replaces() {
        let mut slot = PlanSlot::Ready("old plan");
        slot.replan(Err::<&str, _>("repo went away"));
        assert!(
            slot.plan().is_none(),
            "a stale plan must not stay confirmable"
        );
        assert_eq!(slot.error(), Some("repo went away"));
    }

    #[test]
    fn retry_after_failure_restores_a_confirmable_plan() {
        let mut slot = PlanSlot::<&str>::default();
        assert!(matches!(slot, PlanSlot::Pending));
        slot.replan(Err::<&str, _>("transient"));
        slot.replan(Ok::<_, &str>("fresh plan"));
        assert_eq!(slot.plan(), Some(&"fresh plan"));
        assert_eq!(slot.error(), None, "a successful replan clears the failure");
    }

    #[test]
    fn a_pending_slot_is_not_confirmable_and_shows_no_error() {
        let slot = PlanSlot::<&str>::Pending;
        assert!(slot.plan().is_none());
        assert!(slot.error().is_none());
    }

    #[test]
    fn replan_keeps_only_the_newest_plan() {
        let mut slot = PlanSlot::Ready("first");
        slot.replan(Ok::<_, &str>("second"));
        assert_eq!(slot.plan(), Some(&"second"));
    }
}
