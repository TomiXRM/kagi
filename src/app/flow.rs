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
    Conflict {
        plan: Box<kagi_git::backend::conflict_ops::ConflictPlan>,
        request: ConflictAppRequest,
        policy: kagi_git::backend::ExecutionPolicy,
    },
}
impl Planned {
    pub fn owner(&self) -> OwnerAttachment {
        match self {
            Self::Remove { request, .. } => OwnerAttachment::Local(request.owner.clone()),
            Self::Stash { request, .. } => OwnerAttachment::Local(request.owner.clone()),
            Self::RemoteStash { request, .. } => OwnerAttachment::Remote(request.owner.clone()),
            Self::Conflict { request, .. } => OwnerAttachment::Local(request.owner.clone()),
        }
    }
    pub fn scope(&self) -> WriteScope {
        match self {
            Self::Remove { plan, .. } => WriteScope::Local(plan.common_dir.clone()),
            Self::Stash { plan, .. } => WriteScope::Local(plan.common_dir.clone()),
            Self::RemoteStash { plan, .. } => WriteScope::Remote(plan.repo_id.clone()),
            Self::Conflict { plan, .. } => WriteScope::Local(plan.common_dir.clone()),
        }
    }
    pub fn owner_session(&self) -> SessionId {
        self.owner().session()
    }
    /// Whether this operation changes resources the whole repository shares
    /// (refs, the stash reflog, worktree administration) rather than only the
    /// target worktree's index and working tree. Shared changes make every open
    /// sibling worktree stale (#482 invariant).
    pub fn changes_shared_refs(&self) -> bool {
        match self {
            Self::Remove { .. } => true,
            // Apply reads the stash and writes only this worktree's index/WT;
            // push/pop/drop all rewrite the stash reflog.
            Self::Stash { plan, .. } => !matches!(plan.action, StashAction::Apply { .. }),
            // Remote invalidation is routed to its frozen session; local
            // WorktreeId sibling discovery cannot describe a remote repository.
            Self::RemoteStash { .. } => false,
            Self::Conflict { .. } => false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Policy {
    Remove(RemovePolicy),
    Stash(StashPolicy),
    Conflict(kagi_git::backend::ExecutionPolicy),
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
/// Local plans must keep their frozen attachment, planned identity, and current
/// locator on one worktree. Remote plans instead require an attached remote
/// session (`worktree=None`); their frozen connection and `RemoteRepoId` are
/// rechecked by the transport.
fn identity_matches(s: &Sessions, prepared: &Planned) -> Result<(), AdmissionError> {
    match prepared {
        Planned::Remove { plan, request, .. } => {
            s.confirm_identity(&request.owner)?;
            if request.owner.worktree.as_ref() != Some(&plan.worktree) {
                return Err(AdmissionError::Identity(
                    "the plan resolved a different worktree than this tab; reopen the repository"
                        .into(),
                ));
            }
        }
        Planned::Stash { plan, request, .. } => {
            s.confirm_identity(&request.owner)?;
            if request.owner.worktree.as_ref() != Some(&plan.worktree) {
                return Err(AdmissionError::Identity(
                    "the plan resolved a different worktree than this tab; reopen the repository"
                        .into(),
                ));
            }
        }
        // The transport freezes and rechecks the remote connection and
        // RemoteRepoId. The application boundary owns only the remote tab's
        // SessionId; its Attachment intentionally has no local WorktreeId.
        Planned::RemoteStash { request, .. } => {
            if s.worktree_of(request.owner.session).is_some() {
                return Err(AdmissionError::Identity(
                    "a remote operation requires a remote tab attachment".into(),
                ));
            }
        }
        Planned::Conflict { plan, request, .. } => {
            s.confirm_identity(&request.owner)?;
            if request.owner.worktree.as_ref() != Some(&plan.worktree) {
                return Err(AdmissionError::Identity(
                    "the conflict plan resolved a different worktree than this tab; reopen the repository"
                        .into(),
                ));
            }
            if s.conflict_revision(request.owner.session) != Some(plan.request.revision()) {
                return Err(AdmissionError::StaleApproval);
            }
        }
    }
    Ok(())
}
pub fn apply_plan(s: &mut Sessions, c: PlanCompletion) -> bool {
    if s.revision != c.revision || !matches!(s.state, PlanState::Planning { .. }) {
        return false;
    }
    // #482: never adopt a plan built against a different worktree than the one
    // this tab froze at attach time. The preview, stash indices and OIDs in it
    // came from that other repository — showing them is already the bug.
    if let PlanState::Ready { prepared, .. } = &c.state {
        if let Err(error) = identity_matches(s, prepared) {
            s.state = PlanState::Error {
                error: error.to_string(),
                open_failed: false,
                recording: None,
            };
            return true;
        }
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
        Planned::Conflict { policy, .. } => Policy::Conflict(*policy),
    };
    if current.revision != token.revision || planned_policy != policy.into() {
        return Err(AdmissionError::StaleApproval);
    }
    // #482 stage 1: an approval whose owner has left cannot be dispatched. The
    // plan slot is already expired by `detach`; this is the belt to that brace.
    if !s.is_attached(prepared.owner_session()) {
        return Err(AdmissionError::StaleApproval);
    }
    // Re-resolve: the modal was on screen while the user could swap the path.
    identity_matches(s, prepared)?;
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
    Conflict(Box<ConflictCompletion>),
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
impl From<ConflictCompletion> for Completion {
    fn from(c: ConflictCompletion) -> Self {
        Self::Conflict(Box::new(c))
    }
}
#[derive(Clone, Debug)]
pub enum FamilyEvidence {
    Remove(kagi_git::backend::remove::RemoveReport),
    Stash(kagi_git::backend::stash::StashReport),
    RemoteStash(crate::remote::stash::RemoteStashReport),
    Conflict(kagi_git::backend::conflict_ops::ConflictReport),
}
#[derive(Clone, Debug)]
pub struct ExecutionReport {
    pub recording: Recording,
    pub evidence: FamilyEvidence,
}
pub enum Job {
    Remove(RemoveJob),
    Stash(StashJob),
    Conflict(ConflictJob),
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
            Self::Conflict(job) => job.run().into(),
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
        Planned::Conflict { .. } => prepare_conflict(s, approved, legacy).map(Job::Conflict),
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
        Completion::Conflict(c) => {
            let c = *c;
            (
                c.id,
                ExecutionReport {
                    recording: c.report.recording.clone(),
                    evidence: FamilyEvidence::Conflict(c.report),
                },
                true,
                None,
            )
        }
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
            let manager = InvalidTarget {
                worktree: plan.worktree.clone(),
                path: plan.repo.clone(),
            };
            let target = InvalidTarget {
                worktree: plan.worktree_id.clone(),
                path: plan.target.clone(),
            };
            deliveries.push(Delivery::Invalidate(manager.clone()));
            s.stale.insert(manager.worktree.clone());
            deliveries.push(if r.target_exists == Some(false) {
                Delivery::RemovedTarget(target.clone())
            } else {
                Delivery::Invalidate(target.clone())
            });
            s.stale.insert(target.worktree);
        }
        (Planned::Stash { plan, request, .. }, FamilyEvidence::Stash(r)) => {
            let manager = InvalidTarget {
                worktree: plan.worktree.clone(),
                path: plan.repo.clone(),
            };
            if matches!(
                plan.action,
                StashAction::Apply { .. } | StashAction::Pop { .. }
            ) && !r.evidence.conflicts.is_empty()
            {
                // The conflict belongs to the session that approved the stash,
                // and to the *visit* it approved it in. A closed owner leaves no
                // payload behind, and one the user has since left gets no new
                // proposal — the next visit must re-observe it live (#557).
                let session = request.owner.session;
                if let (Some(oid), true) = (
                    &r.evidence.oid,
                    s.visit(session) == Some(request.owner.visit),
                ) {
                    s.clear_stash_conflict(session);
                    s.stash_conflicts.insert(
                        session,
                        StashConflict {
                            operation: id,
                            oid: oid.clone(),
                            identity: r.evidence.conflict_identity.clone(),
                            pending: false,
                            visit: request.owner.visit,
                        },
                    );
                }
            }
            deliveries.push(Delivery::Invalidate(manager.clone()));
            s.stale.insert(manager.worktree.clone());
        }
        (Planned::RemoteStash { .. }, FamilyEvidence::RemoteStash(_)) => {}
        (Planned::Conflict { request, .. }, FamilyEvidence::Conflict(report)) => {
            if s.is_attached(request.owner.session) {
                s.conflict_states.insert(
                    request.owner.session,
                    ConflictOwnerState::Settled(report.evidence.after.clone()),
                );
            }
            let target = InvalidTarget {
                worktree: request
                    .owner
                    .worktree
                    .clone()
                    .expect("local conflict owner has a worktree"),
                path: request.owner.path.clone(),
            };
            s.stale.insert(target.worktree.clone());
            deliveries.push(Delivery::Invalidate(target));
        }
        _ => unreachable!("completion family is fixed by its owned job"),
    }
    // Shared refs / stash / worktree administration make every *open* sibling
    // worktree of the same repository stale, not only the one that was written.
    // Index- and working-tree-only changes stay scoped to their target (#482).
    if owner.plan.changes_shared_refs() {
        let common_dir = match &owner.plan {
            Planned::Remove { plan, .. } => &plan.common_dir,
            Planned::Stash { plan, .. } => &plan.common_dir,
            Planned::RemoteStash { .. } => unreachable!("remote refs have no local siblings"),
            Planned::Conflict { .. } => unreachable!("conflict writes are worktree-local"),
        };
        let delivered: Vec<_> = deliveries
            .iter()
            .filter_map(|delivery| match delivery {
                Delivery::Invalidate(t) | Delivery::RemovedTarget(t) => Some(t.worktree.clone()),
                Delivery::Completed { .. } | Delivery::RemoteCompleted { .. } => None,
            })
            .collect();
        for worktree in s.siblings_of(common_dir) {
            if delivered.contains(&worktree) {
                continue;
            }
            let path = s
                .path_of(&worktree)
                .unwrap_or_else(|| worktree.git_dir.clone());
            s.stale.insert(worktree.clone());
            deliveries.push(Delivery::Invalidate(InvalidTarget { worktree, path }));
        }
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
