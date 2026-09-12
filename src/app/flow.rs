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
    /// ADR-0196 Wave 3: a legacy `Backend::run` write whose plan lives in its
    /// modal. Admitted through [`approve_run`], not the plan slot.
    Run(RunRequest),
    /// ADR-0196 Wave 3: the pull workflow — one admitted write that runs up to
    /// three recorded children. Admitted through [`approve_pull`].
    Pull(PullRequest),
}
impl Planned {
    pub fn owner(&self) -> OwnerAttachment {
        match self {
            Self::Remove { request, .. } => OwnerAttachment::Local(request.owner.clone()),
            Self::Stash { request, .. } => OwnerAttachment::Local(request.owner.clone()),
            Self::RemoteStash { request, .. } => OwnerAttachment::Remote(request.owner.clone()),
            Self::Conflict { request, .. } => OwnerAttachment::Local(request.owner.clone()),
            Self::Run(request) => OwnerAttachment::Local(request.owner.clone()),
            Self::Pull(request) => OwnerAttachment::Local(request.owner.clone()),
        }
    }
    pub fn scope(&self) -> WriteScope {
        match self {
            Self::Remove { plan, .. } => WriteScope::Local(plan.common_dir.clone()),
            Self::Stash { plan, .. } => WriteScope::Local(plan.common_dir.clone()),
            Self::RemoteStash { plan, .. } => WriteScope::Remote(plan.repo_id.clone()),
            Self::Conflict { plan, .. } => WriteScope::Local(plan.common_dir().clone()),
            Self::Run(request) => WriteScope::Local(request.repo.clone()),
            Self::Pull(request) => WriteScope::Local(request.repo.clone()),
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
            // Conservative: every run-pipeline write may move refs or HEAD, so
            // every open sibling is told. Index-only families are not here.
            Self::Run { .. } => true,
            // Pull moves refs and its auto-stash rewrites the stash reflog.
            Self::Pull { .. } => true,
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
            if request.owner.worktree.as_ref() != Some(plan.worktree()) {
                return Err(AdmissionError::Identity(
                    "the conflict plan resolved a different worktree than this tab; reopen the repository"
                        .into(),
                ));
            }
            if s.conflict_revision(request.owner.session) != Some(plan.request().revision()) {
                return Err(AdmissionError::StaleApproval);
            }
        }
        Planned::Run(request) => s.confirm_identity(&request.owner)?,
        Planned::Pull(request) => s.confirm_identity(&request.owner)?,
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
        // A run plan has no token to spend; it is admitted by `approve_run`.
        Planned::Run(_) => return Err(AdmissionError::StaleApproval),
        // Likewise the pull workflow: `approve_pull` is its admission.
        Planned::Pull(_) => return Err(AdmissionError::StaleApproval),
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
/// Admit one approved write: the single entry every state-changing operation
/// takes before its task is spawned (ADR-0196 決定 1, #643 A0).
///
/// Checks, in order: the approval is the current one, no legacy writer holds
/// the global busy slot, the scope has no unresolved `Unknown` awaiting
/// reconcile, and no lease is held. Then it reserves the lease, freezes the
/// owner as an [`OwnerStamp`], and expires the plan slot so the same approval
/// cannot be spent twice.
pub fn begin_write(
    s: &mut Sessions,
    approved: &Approved,
    legacy: LegacyBusy,
) -> Result<RunningWrite, AdmissionError> {
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
    let attachment = approved.prepared.owner();
    let session = attachment.session();
    // The owner was verified attached by `approve`; a visit of 0 for a session
    // that vanished between approve and here is still a routing key that will
    // never match a live tab, which is the correct outcome for it.
    let stamp = OwnerStamp {
        session,
        visit: s.visit(session).unwrap_or(0),
        operation: id,
    };
    s.operations.insert(
        id,
        InFlight {
            plan: approved.prepared.clone(),
            attachment,
            stamp,
        },
    );
    s.invalidate_plan();
    Ok(RunningWrite {
        operation_id: id,
        owner_stamp: stamp,
    })
}

/// Family `prepare_*` entry points only need the id today; they migrate to
/// [`begin_write`] as their jobs learn to carry the stamp (Wave 3).
pub(crate) fn reserve(
    s: &mut Sessions,
    approved: &Approved,
    legacy: LegacyBusy,
) -> Result<OperationId, AdmissionError> {
    begin_write(s, approved, legacy).map(|running| running.operation_id)
}
#[derive(Clone, Debug)]
pub enum Completion {
    Remove(Box<RemoveCompletion>),
    Stash(Box<StashCompletion>),
    Conflict(Box<ConflictCompletion>),
    Run(Box<RunCompletion>),
    Pull(Box<PullCompletion>),
}
impl From<PullCompletion> for Completion {
    fn from(c: PullCompletion) -> Self {
        Self::Pull(Box::new(c))
    }
}
impl From<RunCompletion> for Completion {
    fn from(c: RunCompletion) -> Self {
        Self::Run(Box::new(c))
    }
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
    Run(kagi_git::backend::recording::RunReport),
    Pull(PullReport),
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
    Run(RunJob),
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
            Self::Run(job) => job.run().into(),
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
        // The run family binds its own blocking core: see `prepare_run`.
        Planned::Run(_) => Err(AdmissionError::Identity(
            "run-family writes are prepared through prepare_run".into(),
        )),
        Planned::Pull(_) => Err(AdmissionError::Identity(
            "pull writes are prepared through prepare_pull".into(),
        )),
    }
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
