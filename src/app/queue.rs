//! Session-owned intent ordering. The UI executes returned effects; this module
//! never plans, acquires a lease, opens a modal, or writes to a repository.
use super::{Attachment, OwnerStamp, SessionId, WorktreeId};
use kagi_git::backend::recording::Recording;
use kagi_git::oplog::OpOutcome as SemanticOutcome;
use std::collections::{HashMap, HashSet, VecDeque};

pub const MAX_QUEUED_INTENTS: usize = 16;
pub const MAX_CANCELLED_INTENTS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IntentId(pub u64);

/// Only frozen user inputs, never an approved plan, resolved HEAD, or prediction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntentRequest {
    Checkout { target: CheckoutIntent },
    Commit { message: String },
    Merge { source: String, into: String },
}

/// The checkout target the user picked: a branch name or a commit OID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckoutIntent {
    Branch(String),
    Commit(kagi_git::CommitId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitReason {
    WriteRunning,
    PlanSlotBusy,
    NeedsConfirmation,
    NeedsReconcile,
    RemoteLatched,
}

/// The observable event which may release each named wait. All such events run
/// the same arbitration pass over *every* session's oldest head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseEvent {
    LeaseReleased,
    PlanSlotFreed,
    OwnerReturned,
    ModalSlotFree,
    RevalidationDone,
    ReconcileAcknowledged,
    RemoteLatchReleased,
}
impl WaitReason {
    pub fn released_by(self, event: ReleaseEvent) -> bool {
        match self {
            Self::WriteRunning => matches!(event, ReleaseEvent::LeaseReleased),
            Self::PlanSlotBusy => matches!(event, ReleaseEvent::PlanSlotFreed),
            Self::NeedsConfirmation => matches!(
                event,
                ReleaseEvent::OwnerReturned
                    | ReleaseEvent::ModalSlotFree
                    | ReleaseEvent::RevalidationDone
            ),
            Self::NeedsReconcile => matches!(event, ReleaseEvent::ReconcileAcknowledged),
            Self::RemoteLatched => matches!(event, ReleaseEvent::RemoteLatchReleased),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelReason {
    UserRemoved,
    UserRejected,
    PlanError,
    ChainTripped { by: ChainAnchor },
    OwnerGone,
    IdentityChanged,
    StaleApproval,
    CapacityRejected,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntentState {
    Queued,
    Waiting { reason: WaitReason },
    Planning,
    AwaitingConfirm,
    Admitting,
    Running { stamp: OwnerStamp },
    Settled,
    Cancelled { reason: CancelReason },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainAnchor {
    QueuedHead(IntentId),
    ActiveWrite(OwnerStamp),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainGate {
    Armed { anchor: Option<ChainAnchor> },
    Tripped { by: ChainAnchor },
}
impl Default for ChainGate {
    fn default() -> Self {
        Self::Armed { anchor: None }
    }
}
#[derive(Clone, Debug)]
pub struct QueuedIntent {
    pub id: IntentId,
    pub owner: SessionId,
    /// Recorded for diagnostics, never used to authorize a new visit.
    pub visit: u64,
    pub worktree: WorktreeId,
    pub request: IntentRequest,
    pub enqueued: u64,
    pub state: IntentState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionEvidence {
    Verified,
    Unverified,
    NotApplicable,
}
/// Only the authoritative, once-settled receipt from `Sessions::apply` is valid.
pub struct Settlement<'a> {
    pub outcome: &'a SemanticOutcome,
    pub evidence: ExecutionEvidence,
    pub recording: &'a Recording,
    pub reconcile_required: bool,
}
impl Settlement<'_> {
    fn permits_successor(&self) -> bool {
        matches!(self.outcome, SemanticOutcome::Success { .. })
            && self.evidence == ExecutionEvidence::Verified
            && matches!(self.recording, Recording::Appended { .. })
            && !self.reconcile_required
    }
}

pub enum QueueEvent<'a> {
    Enqueue {
        owner: Attachment,
        request: IntentRequest,
    },
    RemoveOne(IntentId),
    CancelAll(SessionId),
    CancelHead(IntentId),
    PlanCompleted(IntentId),
    PlanError(IntentId),
    Approve(IntentId),
    Reject(IntentId),
    Admission {
        id: IntentId,
        result: Result<OwnerStamp, super::AdmissionError>,
    },
    AnchorSettled {
        stamp: OwnerStamp,
        receipt: Settlement<'a>,
    },
    IdentityChanged(IntentId),
    /// A run-family write whose receipt the chain can judge (it has a
    /// verify path), started outside the queue.
    WriteStarted(OwnerStamp),
    /// A write with no judgeable receipt: a guard writer, a run-family write
    /// without a verify path, or a clone. `owner: None` is window-level
    /// background work (auto-fetch, clone) that is no one's predecessor.
    UntrackedWriteStarted {
        owner: Option<SessionId>,
    },
    /// The lease (or untracked latch) is gone. `Some` names the tracked write.
    LeaseReleased(Option<OwnerStamp>),
    /// A plan job outside the queue took the plan slot.
    PlanSlotTaken,
    /// `Some(id)` identifies a queue plan job; `None` is an unrelated plan slot.
    PlanSlotFreed(Option<IntentId>),
    /// The head's confirmation went away unanswered (a repository reload
    /// invalidated the plan, or another modal replaced it). Not a decision.
    ConfirmWithdrawn(IntentId),
    OwnerDeparted(SessionId),
    OwnerReturned(SessionId),
    OwnerDetached(SessionId),
    OwnerReattached(SessionId),
    ModalSlotBusy,
    ModalSlotFree,
    RevalidationStarted(SessionId),
    RevalidationDone(SessionId),
    ReconcileAcknowledged(SessionId),
    RemoteLatched,
    RemoteLatchReleased,
    DismissCancelled(SessionId),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnqueueError {
    CapacityRejected,
    RemoteLatched,
    IdentityChanged,
    /// The owner's own write has no judgeable receipt, so `&&` cannot hold.
    UntrackedWrite,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueEffect {
    Enqueued(IntentId),
    Cancelled(IntentId, CancelReason),
    Settled(IntentId),
    Rejected(EnqueueError),
    StartPlan(IntentId),
    OpenConfirm(IntentId),
    BeginAdmission(IntentId),
    CloseConfirm(IntentId),
    InvalidatePlan(IntentId),
}

#[derive(Default)]
pub struct IntentQueue {
    per_session: HashMap<SessionId, VecDeque<QueuedIntent>>,
    cancelled: HashMap<SessionId, VecDeque<QueuedIntent>>,
    gates: HashMap<SessionId, ChainGate>,
    next: u64,
    active: Option<SessionId>,
    write: Option<OwnerStamp>,
    write_busy: bool,
    untracked_owner: Option<SessionId>,
    plan_slot_busy: bool,
    planning_job: Option<IntentId>,
    modal_busy: bool,
    revalidating: HashSet<SessionId>,
    reconciling: HashSet<SessionId>,
    remote_latched: bool,
}
impl IntentQueue {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn intents(&self, session: SessionId) -> Option<&VecDeque<QueuedIntent>> {
        self.per_session.get(&session)
    }
    pub fn cancelled(&self, session: SessionId) -> Option<&VecDeque<QueuedIntent>> {
        self.cancelled.get(&session)
    }
    pub fn gate(&self, session: SessionId) -> ChainGate {
        self.gates.get(&session).copied().unwrap_or_default()
    }
    /// Intents not yet admitted as writes (ADR-0204 決定 2).
    pub fn queued_count(&self, session: SessionId) -> usize {
        self.per_session.get(&session).map_or(0, |queue| {
            queue
                .iter()
                .filter(|intent| match intent.state {
                    IntentState::Queued
                    | IntentState::Waiting { .. }
                    | IntentState::Planning
                    | IntentState::AwaitingConfirm
                    | IntentState::Admitting => true,
                    IntentState::Running { .. }
                    | IntentState::Settled
                    | IntentState::Cancelled { .. } => false,
                })
                .count()
        })
    }
    pub fn auto_fetch_allowed(&self, session: SessionId) -> bool {
        self.per_session
            .get(&session)
            .is_none_or(VecDeque::is_empty)
    }
    pub fn pipeline_count(&self) -> usize {
        self.per_session
            .values()
            .flat_map(|q| q.iter())
            .filter(|intent| {
                matches!(
                    intent.state,
                    IntentState::Planning
                        | IntentState::AwaitingConfirm
                        | IntentState::Admitting
                        | IntentState::Running { .. }
                )
            })
            .count()
    }
    /// Every session that owns queued or cancelled intents.
    pub fn sessions(&self) -> Vec<SessionId> {
        let mut sessions: Vec<_> = self
            .per_session
            .keys()
            .chain(self.cancelled.keys())
            .copied()
            .collect();
        sessions.sort_unstable_by_key(|s| (s.tab.0, s.incarnation));
        sessions.dedup();
        sessions
    }
    /// A live (not settled, not cancelled) intent.
    pub fn intent(&self, id: IntentId) -> Option<&QueuedIntent> {
        self.per_session
            .values()
            .flat_map(|q| q.iter())
            .find(|intent| intent.id == id)
    }
    pub fn is_empty(&self) -> bool {
        self.per_session.values().all(VecDeque::is_empty)
    }
    pub fn apply(&mut self, event: QueueEvent<'_>) -> Vec<QueueEffect> {
        use QueueEvent as E;
        let mut effects = Vec::new();
        match event {
            E::Enqueue { owner, request } => {
                let session = owner.session;
                if self.remote_latched {
                    effects.push(QueueEffect::Rejected(EnqueueError::RemoteLatched));
                } else if self.untracked_owner == Some(session) {
                    effects.push(QueueEffect::Rejected(EnqueueError::UntrackedWrite));
                } else if owner.worktree.is_none() {
                    effects.push(QueueEffect::Rejected(EnqueueError::IdentityChanged));
                } else if self.queued_count(session) >= MAX_QUEUED_INTENTS {
                    effects.push(QueueEffect::Rejected(EnqueueError::CapacityRejected));
                } else {
                    self.next += 1;
                    let id = IntentId(self.next);
                    let previous = self
                        .per_session
                        .get(&session)
                        .and_then(VecDeque::front)
                        .map(|i| i.id);
                    if previous.is_none() {
                        self.gates.insert(
                            session,
                            ChainGate::Armed {
                                anchor: self
                                    .write
                                    .filter(|stamp| stamp.session == session)
                                    .map(ChainAnchor::ActiveWrite),
                            },
                        );
                    } else if self.per_session.get(&session).is_some_and(|q| q.len() == 1)
                        && matches!(self.gate(session), ChainGate::Armed { anchor: None })
                    {
                        self.gates.insert(
                            session,
                            ChainGate::Armed {
                                anchor: previous.map(ChainAnchor::QueuedHead),
                            },
                        );
                    }
                    self.per_session
                        .entry(session)
                        .or_default()
                        .push_back(QueuedIntent {
                            id,
                            owner: session,
                            visit: owner.visit,
                            worktree: owner.worktree.expect("checked"),
                            request,
                            enqueued: self.next,
                            state: IntentState::Queued,
                        });
                    effects.push(QueueEffect::Enqueued(id));
                }
            }
            E::RemoveOne(id) => {
                if let Some(session) = self.per_session.iter().find_map(|(session, q)| {
                    q.iter()
                        .any(|i| {
                            i.id == id
                                && matches!(
                                    i.state,
                                    IntentState::Queued | IntentState::Waiting { .. }
                                )
                        })
                        .then_some(*session)
                }) {
                    let q = self.per_session.get_mut(&session).expect("found");
                    let index = q.iter().position(|i| i.id == id).expect("found");
                    let item = q.remove(index).expect("found");
                    self.record_cancel(item, CancelReason::UserRemoved);
                    if let ChainGate::Armed {
                        anchor: Some(ChainAnchor::QueuedHead(anchor)),
                    } = self.gate(session)
                    {
                        if anchor == id {
                            self.gates.insert(
                                session,
                                ChainGate::Armed {
                                    anchor: self
                                        .per_session
                                        .get(&session)
                                        .and_then(VecDeque::front)
                                        .map(|i| ChainAnchor::QueuedHead(i.id)),
                                },
                            );
                        }
                    }
                    self.clear_empty(session);
                }
            }
            E::CancelAll(session) => {
                if self
                    .per_session
                    .get(&session)
                    .and_then(VecDeque::front)
                    .is_some_and(|i| {
                        matches!(
                            i.state,
                            IntentState::Planning | IntentState::AwaitingConfirm
                        )
                    })
                {
                    // Closes its own confirmation and leaves the latch to
                    // the plan job's terminal callback (決定 3).
                    self.cancel_head(session, CancelReason::UserRemoved, &mut effects);
                }
                if let Some(mut queue) = self.per_session.remove(&session) {
                    while let Some(item) = queue.pop_front() {
                        if matches!(item.state, IntentState::Running { .. }) {
                            self.per_session.entry(session).or_default().push_back(item);
                        } else {
                            self.record_cancel(item, CancelReason::UserRemoved);
                        }
                    }
                    self.clear_empty(session);
                }
            }
            E::CancelHead(id) | E::PlanError(id) | E::Reject(id) => {
                if let Some(session) = self.head(id) {
                    let state = self.per_session[&session].front().expect("head").state;
                    if matches!(state, IntentState::Planning | IntentState::AwaitingConfirm) {
                        let reason = match event {
                            E::Reject(_) => CancelReason::UserRejected,
                            E::PlanError(_) => CancelReason::PlanError,
                            _ => CancelReason::UserRemoved,
                        };
                        self.cancel_head(session, reason, &mut effects);
                    }
                }
            }
            E::IdentityChanged(id) => {
                if let Some(session) = self.head(id) {
                    self.cancel_head(session, CancelReason::IdentityChanged, &mut effects);
                }
            }
            E::PlanCompleted(id) => {
                if let Some(session) = self.head(id) {
                    let head = self
                        .per_session
                        .get_mut(&session)
                        .expect("head")
                        .front_mut()
                        .expect("head");
                    if head.state == IntentState::Planning {
                        self.plan_slot_busy = false;
                        self.planning_job = None;
                        if self.active == Some(session)
                            && !self.modal_busy
                            && !self.revalidating.contains(&session)
                        {
                            head.state = IntentState::AwaitingConfirm;
                            self.modal_busy = true;
                            effects.push(QueueEffect::OpenConfirm(id));
                        } else {
                            head.state = IntentState::Queued;
                            effects.push(QueueEffect::InvalidatePlan(id));
                        }
                    }
                }
            }
            E::Approve(id) => {
                if let Some(session) = self.head(id) {
                    let head = self
                        .per_session
                        .get_mut(&session)
                        .expect("head")
                        .front_mut()
                        .expect("head");
                    if head.state == IntentState::AwaitingConfirm && self.active == Some(session) {
                        head.state = IntentState::Admitting;
                        effects.push(QueueEffect::BeginAdmission(id));
                    }
                }
            }
            E::Admission { id, result } => {
                if let Some(session) = self.head(id) {
                    if self.per_session[&session].front().expect("head").state
                        == IntentState::Admitting
                    {
                        match result {
                            Ok(stamp) if stamp.session == session => {
                                self.per_session
                                    .get_mut(&session)
                                    .expect("head")
                                    .front_mut()
                                    .expect("head")
                                    .state = IntentState::Running { stamp };
                                self.write = Some(stamp);
                                self.modal_busy = false;
                                effects.push(QueueEffect::CloseConfirm(id));
                            }
                            Ok(_) | Err(super::AdmissionError::Identity(_)) => {
                                self.cancel_head(
                                    session,
                                    CancelReason::IdentityChanged,
                                    &mut effects,
                                );
                            }
                            Err(super::AdmissionError::StaleApproval) => {
                                self.cancel_head(
                                    session,
                                    CancelReason::StaleApproval,
                                    &mut effects,
                                );
                            }
                            Err(error) => {
                                let reason = match error {
                                    super::AdmissionError::Busy => {
                                        self.write_busy = true;
                                        WaitReason::WriteRunning
                                    }
                                    super::AdmissionError::NeedsReconcile => {
                                        self.reconciling.insert(session);
                                        WaitReason::NeedsReconcile
                                    }
                                    super::AdmissionError::Identity(_)
                                    | super::AdmissionError::StaleApproval => unreachable!(),
                                };
                                self.per_session
                                    .get_mut(&session)
                                    .expect("head")
                                    .front_mut()
                                    .expect("head")
                                    .state = IntentState::Waiting { reason };
                                self.modal_busy = false;
                                effects.push(QueueEffect::CloseConfirm(id));
                                effects.push(QueueEffect::InvalidatePlan(id));
                            }
                        }
                    }
                }
            }
            E::AnchorSettled { stamp, receipt } => {
                if let Some(id) = self.settle(stamp, receipt) {
                    effects.push(QueueEffect::Settled(id));
                }
            }
            E::WriteStarted(stamp) => self.write = Some(stamp),
            E::UntrackedWriteStarted { owner } => {
                self.write_busy = true;
                self.untracked_owner = owner;
            }
            E::LeaseReleased(stamp) => {
                if stamp.is_some() && self.write == stamp {
                    self.write = None;
                }
                self.write_busy = false;
                self.untracked_owner = None;
            }
            E::PlanSlotTaken => {
                if self.planning_job.is_none() {
                    self.plan_slot_busy = true;
                }
            }
            E::PlanSlotFreed(owner) => {
                if self.planning_job == owner {
                    self.plan_slot_busy = false;
                    self.planning_job = None;
                }
            }
            E::ConfirmWithdrawn(id) => {
                if let Some(session) = self.head(id) {
                    let head = self
                        .per_session
                        .get_mut(&session)
                        .and_then(VecDeque::front_mut)
                        .expect("head");
                    // Whatever took the slot (or nothing) is reported by
                    // ModalSlotBusy / ModalSlotFree, never assumed here.
                    if head.state == IntentState::AwaitingConfirm {
                        head.state = IntentState::Queued;
                        effects.push(QueueEffect::InvalidatePlan(id));
                    }
                }
            }
            E::OwnerDeparted(session) => {
                if self.active == Some(session) {
                    self.active = None;
                }
                if let Some(head) = self
                    .per_session
                    .get_mut(&session)
                    .and_then(VecDeque::front_mut)
                {
                    if matches!(
                        head.state,
                        IntentState::Planning | IntentState::AwaitingConfirm
                    ) {
                        let was_planning = head.state == IntentState::Planning;
                        head.state = IntentState::Queued;
                        effects.push(QueueEffect::CloseConfirm(head.id));
                        effects.push(QueueEffect::InvalidatePlan(head.id));
                        self.modal_busy = false;
                        if !was_planning {
                            self.plan_slot_busy = false;
                            self.planning_job = None;
                        }
                    }
                }
            }
            E::OwnerReturned(session) => {
                if let Some(previous) = self.active.filter(|previous| *previous != session) {
                    effects.extend(self.apply(E::OwnerDeparted(previous)));
                }
                self.active = Some(session);
            }
            E::OwnerDetached(session) | E::OwnerReattached(session) => {
                if self.active == Some(session) {
                    self.active = None;
                }
                self.revalidating.remove(&session);
                self.reconciling.remove(&session);
                self.cancelled.remove(&session);
                if let Some(mut queue) = self.per_session.remove(&session) {
                    while let Some(intent) = queue.pop_front() {
                        if let IntentState::Running { .. } = intent.state {
                            self.per_session
                                .entry(session)
                                .or_default()
                                .push_back(intent);
                        } else {
                            if matches!(
                                intent.state,
                                IntentState::Planning | IntentState::AwaitingConfirm
                            ) {
                                effects.push(QueueEffect::CloseConfirm(intent.id));
                                effects.push(QueueEffect::InvalidatePlan(intent.id));
                                self.modal_busy = false;
                                if intent.state == IntentState::AwaitingConfirm {
                                    self.plan_slot_busy = false;
                                    self.planning_job = None;
                                }
                            }
                            effects
                                .push(QueueEffect::Cancelled(intent.id, CancelReason::OwnerGone));
                        }
                    }
                }
                self.gates.remove(&session);
            }
            E::ModalSlotBusy => self.modal_busy = true,
            E::ModalSlotFree => self.modal_busy = false,
            E::RevalidationStarted(session) => {
                self.revalidating.insert(session);
            }
            E::RevalidationDone(session) => {
                self.revalidating.remove(&session);
            }
            E::ReconcileAcknowledged(session) => {
                self.reconciling.remove(&session);
            }
            E::RemoteLatched => self.remote_latched = true,
            E::RemoteLatchReleased => self.remote_latched = false,
            E::DismissCancelled(session) => {
                self.cancelled.remove(&session);
                if matches!(self.gate(session), ChainGate::Tripped { .. }) {
                    self.gates.insert(session, ChainGate::default());
                }
            }
        }
        self.arbitrate(&mut effects);
        effects
    }
}

mod chain;

#[cfg(test)]
mod tests;
