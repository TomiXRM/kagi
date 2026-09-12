use super::*;
use kagi_domain::remote::RemoteRepoId;
use kagi_domain::remove::{RepoId, WorktreeId};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OperationId(pub(crate) u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestId(pub(crate) u64);
pub(crate) fn next_id() -> u64 {
    static IDS: AtomicU64 = AtomicU64::new(1);
    IDS.fetch_add(1, Ordering::Relaxed)
}

/// One display slot in the tab strip. Stable while the tab exists; a closed and
/// reopened path gets a new `TabId`, so tab-strip index reuse can never make a
/// stale result look current (#482 stage 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TabId(pub(crate) u64);

/// A `TabId` plus the incarnation issued when that slot was attached to a
/// repository. Monotonic and globally unique: `SessionId` equality is the *only*
/// owner test in the application layer — never `(path, index)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionId {
    pub(crate) tab: TabId,
    pub(crate) incarnation: u64,
}
impl SessionId {
    pub fn tab(&self) -> TabId {
        self.tab
    }
}

/// What the application layer knows about one attached tab. GPUI views, rows and
/// snapshots stay in the UI — this is identity and lifetime only.
struct TabSession {
    /// `None` for a remote read-only tab (ADR-0089): its path is a synthetic
    /// `<host>:<root>` key with no local worktree to write to.
    worktree: Option<WorktreeId>,
    /// Locator, not identity (DESIGN §2.1). Used as plan input and for the
    /// still-path-keyed view cache until stage 2 removes it.
    path: PathBuf,
    /// Departure revision: bumped every time the user leaves this tab. The
    /// incarnation is unchanged (the tab is still open) but every *proposal*
    /// made during the previous visit is dead — a stash follow-up must be
    /// produced again from a live re-observation, never restored (#482 / #557).
    visit: u64,
}

/// The frozen delivery owner of one operation. Captured at plan time and carried
/// through execution: completion is routed by `session`, never re-resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    pub session: SessionId,
    pub path: PathBuf,
    /// Frozen at attach time. `None` only for a remote tab, which cannot write.
    /// Re-checked against what the plan actually resolved before adoption and
    /// again before approval, so a path swapped under an open tab is refused.
    pub worktree: Option<WorktreeId>,
    /// The visit this request was made in. A completion arriving after the user
    /// left the tab may still be recorded and displayed, but may not create a
    /// proposal for the next visit.
    pub visit: u64,
}

/// A synthetic remote key never opens, so that tab simply has no worktree.
fn resolve_worktree(path: &std::path::Path) -> Option<WorktreeId> {
    kagi_git::Backend::open(path)
        .and_then(|backend| backend.write_worktree_id())
        .ok()
}

/// Where an invalidation lands. Identity is the `WorktreeId`; `path` is the
/// locator the stage-1 UI still needs for its path-keyed view cache.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidTarget {
    pub worktree: WorktreeId,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub enum OwnerAttachment {
    Local(Attachment),
    Remote(crate::remote::stash::RemoteAttachment),
}
impl OwnerAttachment {
    pub(crate) fn session(&self) -> SessionId {
        match self {
            Self::Local(owner) => owner.session,
            Self::Remote(owner) => owner.session,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WriteScope {
    Local(RepoId),
    Remote(RemoteRepoId),
}

#[derive(Debug, PartialEq, Eq)]
pub enum AdmissionError {
    Busy,
    StaleApproval,
    NeedsReconcile,
    Identity(String),
}
impl std::fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Temporary compatibility input. Remove at the last family migration.
#[derive(Clone, Copy)]
pub struct LegacyBusy(pub bool);

/// The delivery owner of one write, frozen at admission (ADR-0196 決定 3).
///
/// Completion is routed by this stamp and never re-resolved: the legacy path
/// compared `repo_path + switch_generation`, which is a path string standing in
/// for identity and is exactly what let a completion land on the wrong tab or
/// be dropped. `session` says *which* tab incarnation, `visit` says which stay
/// in it (a completion from an earlier visit may be recorded and shown but
/// must not seed a proposal for the next one, #557), `operation` ties it to the
/// one admitted write it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OwnerStamp {
    pub session: SessionId,
    pub visit: u64,
    pub operation: OperationId,
}

/// What `begin_write` hands back: the admitted operation and its frozen owner.
///
/// The lease itself lives in `Sessions` keyed by `operation_id` and is released
/// by `apply` on settlement, so this carries no guard — dropping it is not a
/// cancellation (ADR-0175).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunningWrite {
    pub operation_id: OperationId,
    pub owner_stamp: OwnerStamp,
}

pub(crate) struct InFlight {
    pub plan: Planned,
    pub attachment: OwnerAttachment,
    pub stamp: OwnerStamp,
}
pub(crate) struct ReconcileEntry {
    pub plan: Planned,
    pub stopped: bool,
    pub remote: Option<crate::remote::stash::RemoteStashEvidence>,
    /// The auto-stash a pull created and did not restore, as the backend saw
    /// it. `oid` is `None` exactly when the entry could not be identified
    /// (#623), which is why the evidence — not just an OID — travels here.
    pub pull: Option<kagi_git::backend::stash::StashEvidence>,
    /// The child of an unproven termination. While it is alive the writer is
    /// not proven stopped, so the scope stays reserved (ADR-0175); once it is
    /// gone the reconcile read says so and the entry can be acknowledged.
    pub child: Option<u32>,
}
pub struct Sessions {
    pub(crate) abandoned_tx: std::sync::mpsc::Sender<Completion>,
    abandoned_rx: std::sync::mpsc::Receiver<Completion>,
    pub(crate) plan_errors: Vec<PlanErrorJob>,
    sessions: HashMap<SessionId, TabSession>,
    pub(crate) stash_conflicts: HashMap<SessionId, StashConflict>,
    pub(crate) stash_followups: HashMap<SessionId, StashConflict>,
    pub(crate) conflict_states: HashMap<SessionId, ConflictOwnerState>,
    pub(crate) state: PlanState,
    /// Owner of the single plan slot. The slot expires when that session
    /// detaches, so an approval planned in A can never be spent in B.
    pub(crate) plan_owner: Option<SessionId>,
    pub(crate) revision: RequestId,
    pub(crate) operations: HashMap<OperationId, InFlight>,
    pub(crate) leases: Arc<Mutex<HashMap<WriteScope, OperationId>>>,
    pub(crate) stale: HashSet<WorktreeId>,
    pub(crate) reconcile: HashMap<OperationId, ReconcileEntry>,
    pub(crate) settled: HashSet<OperationId>,
}
impl Default for Sessions {
    fn default() -> Self {
        Self::new()
    }
}
impl Sessions {
    pub fn new() -> Self {
        let (abandoned_tx, abandoned_rx) = std::sync::mpsc::channel();
        Self {
            abandoned_tx,
            abandoned_rx,
            plan_errors: Vec::new(),
            sessions: HashMap::new(),
            stash_conflicts: HashMap::new(),
            stash_followups: HashMap::new(),
            conflict_states: HashMap::new(),
            state: PlanState::Draft,
            plan_owner: None,
            revision: RequestId(next_id()),
            operations: HashMap::new(),
            leases: Arc::new(Mutex::new(HashMap::new())),
            stale: HashSet::new(),
            reconcile: HashMap::new(),
            settled: HashSet::new(),
        }
    }
    pub fn plan_state(&self) -> &PlanState {
        &self.state
    }

    // ── tab attach / detach ───────────────────────────────────────────────
    /// Open a display slot for `path` and issue its first incarnation. The
    /// `WorktreeId` is resolved once, here, and frozen for the slot's life: a
    /// later rename or replacement of the path cannot re-target an operation.
    /// Two locators for one worktree (`/repo` and `/repo/.git`, a symlink, a
    /// `..`-relative path) resolve to the same `WorktreeId` and therefore to the
    /// **same session** — identity unification happens here, not in path
    /// comparison, so a second tab for one worktree is never opened.
    pub fn attach(&mut self, path: PathBuf) -> SessionId {
        if let Some(existing) = resolve_worktree(&path)
            .and_then(|worktree| self.sessions_for(&worktree).first().copied())
        {
            return existing;
        }
        self.attach_incarnation(TabId(next_id()), path)
    }
    /// Same slot, fresh incarnation — used when a tab is re-pointed at a new
    /// session of the same repository (a remote re-snapshot). Everything the old
    /// incarnation owned expires exactly as it would on close.
    pub fn reattach(&mut self, session: SessionId, path: PathBuf) -> SessionId {
        self.detach(session);
        self.attach_incarnation(session.tab, path)
    }
    fn attach_incarnation(&mut self, tab: TabId, path: PathBuf) -> SessionId {
        let session = SessionId {
            tab,
            incarnation: next_id(),
        };
        let worktree = resolve_worktree(&path);
        self.sessions.insert(
            session,
            TabSession {
                worktree,
                path,
                visit: 0,
            },
        );
        session
    }
    /// The user left this tab (a real switch, not a re-select of the live tab).
    /// The tab stays open and its incarnation is unchanged, but the visit ends:
    /// a pending follow-up proposal is discarded and no late completion may
    /// create one for the next visit.
    pub fn depart(&mut self, session: SessionId) {
        self.stash_followups.remove(&session);
        if !matches!(
            self.conflict_states.get(&session),
            Some(ConflictOwnerState::InFlight { .. })
        ) {
            self.conflict_states.remove(&session);
        }
        if let Some(tab) = self.sessions.get_mut(&session) {
            tab.visit += 1;
        }
    }
    pub(crate) fn visit(&self, session: SessionId) -> Option<u64> {
        self.sessions.get(&session).map(|tab| tab.visit)
    }
    /// Close a display slot. Drops what belonged to the *display* — the conflict
    /// and follow-up payloads and the plan slot if this session owned it — and
    /// deliberately keeps `operations` / `leases` / `settled` / `reconcile`:
    /// closing a tab is not cancelling an execution (ADR-0175).
    pub fn detach(&mut self, session: SessionId) {
        self.sessions.remove(&session);
        self.stash_conflicts.remove(&session);
        self.stash_followups.remove(&session);
        self.conflict_states.remove(&session);
        if self.plan_owner == Some(session) {
            self.invalidate_plan();
        }
    }
    pub fn is_attached(&self, session: SessionId) -> bool {
        self.sessions.contains_key(&session)
    }
    /// The frozen owner record for an attached slot. The only way to build an
    /// [`Attachment`]: a caller cannot forge one for a session that is gone.
    pub fn attachment(&self, session: SessionId) -> Option<Attachment> {
        self.sessions.get(&session).map(|tab| Attachment {
            session,
            path: tab.path.clone(),
            worktree: tab.worktree.clone(),
            visit: tab.visit,
        })
    }
    /// The identity check both the plan-adoption and the approval gate use: what
    /// the backend resolves for this locator *now* must still be the identity
    /// frozen when the tab attached. A path swapped under an open tab, or an
    /// identity that no longer resolves, requires a re-attach rather than an
    /// execution against whatever the locator points at today.
    pub(crate) fn confirm_identity(&self, owner: &Attachment) -> Result<(), AdmissionError> {
        let frozen = owner
            .worktree
            .as_ref()
            .ok_or_else(|| AdmissionError::Identity("no local worktree identity".into()))?;
        match resolve_worktree(&owner.path) {
            Some(current) if &current == frozen => Ok(()),
            Some(_) => Err(AdmissionError::Identity(
                "the path now resolves to a different worktree; reopen the repository".into(),
            )),
            None => Err(AdmissionError::Identity(
                "worktree identity could not be resolved; reopen the repository".into(),
            )),
        }
    }
    /// Every open slot holding this worktree, by identity rather than by path.
    /// `attach` unifies aliases so this is normally one, but delivery must never
    /// pick an arbitrary `HashMap` entry when a bootstrap path pushed two.
    pub fn sessions_for(&self, worktree: &WorktreeId) -> Vec<SessionId> {
        self.sessions
            .iter()
            .filter(|(_, tab)| tab.worktree.as_ref() == Some(worktree))
            .map(|(session, _)| *session)
            .collect()
    }
    /// Open worktrees sharing `repo`. Changing shared refs/stash makes every one
    /// of them stale, not only the one that was written (#482 invariant).
    pub fn siblings_of(&self, repo: &RepoId) -> Vec<WorktreeId> {
        let mut worktrees: Vec<_> = self
            .sessions
            .values()
            .filter_map(|tab| tab.worktree.clone())
            .filter(|worktree| &worktree.repo == repo)
            .collect();
        worktrees.sort_by(|a, b| a.git_dir.cmp(&b.git_dir));
        worktrees.dedup();
        worktrees
    }
    /// The locator an open tab uses for a worktree — the stage-1 path-keyed view
    /// cache still needs one to evict by.
    pub fn path_of(&self, worktree: &WorktreeId) -> Option<PathBuf> {
        self.sessions
            .values()
            .find(|tab| tab.worktree.as_ref() == Some(worktree))
            .map(|tab| tab.path.clone())
    }
    pub fn worktree_of(&self, session: SessionId) -> Option<&WorktreeId> {
        self.sessions.get(&session)?.worktree.as_ref()
    }
    pub(crate) fn conflict_revision(
        &self,
        session: SessionId,
    ) -> Option<&kagi_domain::conflict_family::ConflictRevision> {
        match self.conflict_states.get(&session) {
            Some(ConflictOwnerState::Observed(observation)) => Some(&observation.revision),
            Some(ConflictOwnerState::InFlight { revision, .. }) => Some(revision),
            Some(ConflictOwnerState::Settled(Some(observation))) => Some(&observation.revision),
            _ => None,
        }
    }
    pub fn drain_abandoned(&mut self) -> Vec<Delivery> {
        let completions: Vec<_> = self.abandoned_rx.try_iter().collect();
        completions
            .into_iter()
            .flat_map(|completion| self.apply(completion))
            .collect()
    }
    pub fn has_leases(&self) -> bool {
        self.leases
            .lock()
            .map(|leases| !leases.is_empty())
            .unwrap_or(true)
    }
    /// Reserve before dispatch, not a check-only API. All repositories remain
    /// mutually exclusive until the final writer family migration.
    pub fn write_lease(
        &mut self,
        path: &std::path::Path,
        legacy: LegacyBusy,
    ) -> Result<WriteGuard, AdmissionError> {
        if legacy.0 || self.has_leases() {
            return Err(AdmissionError::Busy);
        }
        let repo = kagi_git::Backend::open(path)
            .and_then(|backend| backend.write_repo_id())
            .map_err(|error| AdmissionError::Identity(error.to_string()))?;
        if self
            .reconcile
            .values()
            .any(|entry| entry.plan.scope() == WriteScope::Local(repo.clone()))
        {
            return Err(AdmissionError::NeedsReconcile);
        }
        let id = OperationId(next_id());
        self.reserve_lease(WriteScope::Local(repo.clone()), id)?;
        Ok(WriteGuard {
            leases: self.leases.clone(),
            scope: WriteScope::Local(repo),
            id,
        })
    }
    pub(crate) fn reserve_lease(
        &self,
        scope: WriteScope,
        id: OperationId,
    ) -> Result<(), AdmissionError> {
        let mut leases = self.leases.lock().map_err(|_| AdmissionError::Busy)?;
        if !leases.is_empty() {
            return Err(AdmissionError::Busy);
        }
        leases.insert(scope, id);
        Ok(())
    }
    pub(crate) fn release_lease(&self, scope: &WriteScope, id: OperationId) {
        if let Ok(mut leases) = self.leases.lock() {
            if leases.get(scope) == Some(&id) {
                leases.remove(scope);
            }
        }
    }
    /// Shared policy used by both window-close and Quit adapters, never tab close.
    pub fn may_close_host(&self) -> bool {
        !self.has_leases()
    }
    pub fn invalidate_plan(&mut self) {
        self.revision = RequestId(next_id());
        self.state = PlanState::Draft;
        self.plan_owner = None;
    }
    pub fn is_stale(&self, worktree: &WorktreeId) -> bool {
        self.stale.contains(worktree)
    }
    /// A fresh read landed for this slot's worktree.
    pub fn read_applied(&mut self, session: SessionId) {
        if let Some(worktree) = self.worktree_of(session).cloned() {
            self.stale.remove(&worktree);
        }
    }
    pub fn apply(&mut self, completion: impl Into<Completion>) -> Vec<Delivery> {
        super::apply(self, completion)
    }
}

/// Owned reservation, Send across executor boundaries, independent of views.
/// Drop deliberately DOES NOT release: cancellation/panic is not evidence of
/// termination. Call `complete` only after the writer has definitely stopped.
#[must_use = "hold the reservation until completion; dropping it retains the lease"]
pub struct WriteGuard {
    leases: Arc<Mutex<HashMap<WriteScope, OperationId>>>,
    scope: WriteScope,
    id: OperationId,
}
impl WriteGuard {
    pub fn complete(self) {
        if let Ok(mut leases) = self.leases.lock() {
            if leases.get(&self.scope) == Some(&self.id) {
                leases.remove(&self.scope);
            }
        }
    }
    /// For synchronous file/index writers. Unwinding retains the reservation.
    pub fn run<R>(self, write: impl FnOnce() -> R) -> R {
        let result = write();
        self.complete();
        result
    }
    /// CLI errors with unconfirmed process termination must retain the lease —
    /// and so must a stash whose entry could not be identified (#623): the
    /// repository did change, kagi just cannot name the entry, so the scope
    /// stays reserved until the user reconciles.
    pub fn complete_git<R>(self, result: &Result<R, kagi_git::GitError>) {
        if !matches!(
            result,
            Err(kagi_git::GitError::TerminationUnknown(_)
                | kagi_git::GitError::StashIdentityUnverified(_))
        ) {
            self.complete();
        }
    }
}

/// C0 bridge for conflict Continue / Skip / Abort while those writers still
/// use the legacy UI execution path (#569). Preserve an unconfirmed process
/// termination as `Unknown` and deliberately retain the repository lease.
/// Known termination releases normally. The full conflict-family report and
/// read/ack lifecycle replace this bridge in C3.
pub fn settle_conflict_write<R>(
    guard: WriteGuard,
    result: &Result<R, kagi_git::GitError>,
    after: kagi_git::StateSummary,
) -> Option<kagi_git::OpOutcome> {
    let unknown = match result {
        Err(kagi_git::GitError::TerminationUnknown(reason)) => Some(kagi_git::OpOutcome::Unknown {
            after,
            evidence: format!(
                "{}; process termination is unconfirmed — do not retry this operation",
                reason
            ),
        }),
        _ => None,
    };
    guard.complete_git(result);
    unknown
}

#[derive(Clone, Debug)]
pub enum Delivery {
    Invalidate(InvalidTarget),
    RemovedTarget(InvalidTarget),
    Completed {
        id: OperationId,
        attachment: Attachment,
        /// Routing key frozen at admission; equals what `begin_write` returned.
        stamp: OwnerStamp,
        report: Box<ExecutionReport>,
    },
    RemoteCompleted {
        id: OperationId,
        attachment: crate::remote::stash::RemoteAttachment,
        stamp: OwnerStamp,
        report: Box<ExecutionReport>,
    },
}

#[derive(Clone)]
pub struct ReconcileRead {
    id: OperationId,
    pub observation: String,
    stop_proven: bool,
    /// Whether the read could account for everything the operation left behind.
    /// An unresolved read may be shown, but never acknowledged: releasing the
    /// scope would report "settled" about work kagi cannot point at.
    resolved: bool,
}
impl ReconcileRead {
    pub fn resolved(&self) -> bool {
        self.resolved
    }
    pub fn stop_proven(&self) -> bool {
        self.stop_proven
    }
}
pub struct ReconcileJob {
    id: OperationId,
    plan: Planned,
    remote: Option<crate::remote::stash::RemoteStashEvidence>,
    pull_stash: Option<kagi_git::backend::stash::StashEvidence>,
    child: Option<u32>,
}
impl ReconcileJob {
    pub fn run(self) -> Result<ReconcileRead, String> {
        // `resolved` is whether the read could account for everything the
        // operation left behind; only the pull family can come back unresolved.
        let mut resolved = true;
        let (observation, stop_proven) = match &self.plan {
            Planned::Remove { plan, .. } => (
                kagi_git::Backend::read_remove_status(plan).map_err(|e| e.to_string())?,
                true,
            ),
            Planned::Stash { plan, .. } => (
                kagi_git::Backend::read_stash_status(plan).map_err(|e| e.to_string())?,
                true,
            ),
            Planned::RemoteStash { plan, .. } => {
                let evidence = self
                    .remote
                    .as_ref()
                    .ok_or("remote completion evidence is missing")?;
                (
                    crate::remote::stash::reconcile_remote_stash(plan, evidence, self.id.0)?,
                    true,
                )
            }
            Planned::Conflict { plan, .. } => (
                kagi_git::Backend::open(plan.repo())
                    .and_then(|backend| backend.conflict_snapshot())
                    .map(|snapshot| format!("conflict={snapshot:?}"))
                    .map_err(|e| e.to_string())?,
                true,
            ),
            Planned::Run(request) => (
                kagi_git::Backend::open(&request.path)
                    .and_then(|mut backend| backend.snapshot(1))
                    .map(|snap| {
                        format!(
                            "head={} dirty={}",
                            snap.head.display(),
                            snap.status.is_dirty()
                        )
                    })
                    .map_err(|e| e.to_string())?,
                true,
            ),
            Planned::Pull(request) => {
                let (observation, accounted) = observe_pull(request, self.pull_stash.as_ref())?;
                resolved = accounted;
                (observation, true)
            }
        };
        // A child that could not be reaped is the one thing a repository read
        // cannot speak for: ask the OS whether it is still there.
        let stop_proven = stop_proven
            && self
                .child
                .is_none_or(|pid| !kagi_git::proc::process_alive(pid));
        Ok(ReconcileRead {
            id: self.id,
            observation,
            stop_proven,
            resolved,
        })
    }
}
/// What a pull left behind, read back live: where HEAD and its upstream now
/// stand, whether the auto-stash entry can be accounted for, and whether the
/// working tree is still the one the confirmation named. Never retries and
/// never pops. The `bool` says whether the stash could be accounted for.
fn observe_pull(
    request: &PullRequest,
    stash: Option<&kagi_git::backend::stash::StashEvidence>,
) -> Result<(String, bool), String> {
    let snap = kagi_git::Backend::open(&request.path)
        .and_then(|mut backend| backend.snapshot(1))
        .map_err(|e| e.to_string())?;
    let upstream = match &snap.head {
        kagi_git::Head::Attached { branch, .. } | kagi_git::Head::Unborn { branch } => snap
            .branches
            .iter()
            .find(|candidate| &candidate.name == branch)
            .and_then(|candidate| candidate.upstream.as_ref())
            .map(|up| {
                format!(
                    "{} ahead={} behind={}",
                    up.remote_branch, up.ahead, up.behind
                )
            })
            .unwrap_or_else(|| "none".to_string()),
        kagi_git::Head::Detached { .. } => "detached".to_string(),
    };
    let (stash, resolved) = match stash {
        None => ("none".to_string(), true),
        Some(evidence) => match evidence.oid.as_deref() {
            Some(oid) => match kagi_git::Backend::unique_stash_index(&request.path, oid)
                .map_err(|e| e.to_string())?
            {
                Some(index) => (format!("{oid} at stash@{{{index}}}"), true),
                None => (format!("{oid} no longer in the stash list"), true),
            },
            // #623: the entry could not be identified when it was created, so
            // there is no OID to look up — only what the workflow wrote. Match
            // kagi's own auto-stash message against the live list. While more
            // than one answers to it the read stays unresolved: acknowledging
            // would report "settled" about work kagi cannot point at.
            None => {
                let candidates: Vec<_> = snap
                    .stashes
                    .iter()
                    .filter(|entry| entry.message.contains(AUTO_STASH_MESSAGE))
                    .collect();
                match candidates.as_slice() {
                    [only] => (
                        format!(
                            "unidentified auto-stash resolved to {} at stash@{{{}}}",
                            only.target.short(),
                            only.index
                        ),
                        true,
                    ),
                    [] => (
                        "an auto-stash was created but no entry answers to it".to_string(),
                        true,
                    ),
                    many => (
                        format!(
                            "{} entries answer to the auto-stash; it cannot be told apart",
                            many.len()
                        ),
                        false,
                    ),
                }
            }
        },
    };
    let digest = snap.status.digest();
    let promise = match request.promised_dirty {
        Some(promised) if promised == digest => "as confirmed",
        Some(_) => "moved",
        None => "not promised",
    };
    Ok((
        format!(
            "head={} upstream={upstream} auto_stash={} stash={stash} dirty={} ({promise})",
            snap.head.display(),
            request.auto_stash,
            digest.0
        ),
        resolved,
    ))
}

pub fn prepare_reconcile(sessions: &Sessions, id: OperationId) -> Result<ReconcileJob, String> {
    let entry = sessions.reconcile.get(&id).ok_or("no reconcile request")?;
    // An unproven termination is readable exactly when something can prove it:
    // the remote evidence, or the child the executor could not account for.
    if !entry.stopped && entry.remote.is_none() && entry.child.is_none() {
        return Err("execution termination is unconfirmed".into());
    }
    Ok(ReconcileJob {
        id,
        plan: entry.plan.clone(),
        remote: entry.remote.clone(),
        pull_stash: entry.pull.clone(),
        child: entry.child,
    })
}
pub fn read_reconcile(sessions: &Sessions, id: OperationId) -> Result<ReconcileRead, String> {
    prepare_reconcile(sessions, id)?.run()
}
/// Close one reconcile requirement and release the scope it holds.
///
/// The exit from an unproven termination, for **every** local family — the run
/// pipeline, pull, and whatever migrates next. The lifecycle is one path:
///
/// 1. the executor reports `GitError::TerminationUnknown(kagi_git::Termination)`,
///    which says whether it saw the child stop and, if not, carries its pid;
/// 2. `apply` settles: the lease is released only on a proven stop, and an
///    entry is registered either way, so a retained lease always has something
///    to be acknowledged against;
/// 3. [`prepare_reconcile`] / [`read_reconcile`] observe — including asking the
///    OS whether that pid is still there, which is what turns "unproven" into
///    `stop_proven` later, without ever treating a repository snapshot as
///    proof that a process stopped;
/// 4. this releases the scope, but only for a read that is both `stop_proven`
///    and `resolved`.
///
/// A family adds nothing to this path: it only has to let its
/// `TerminationUnknown` reach the completion with its type intact.
pub fn acknowledge(sessions: &mut Sessions, read: ReconcileRead) -> Result<(), AdmissionError> {
    let Some(entry) = sessions.reconcile.get(&read.id) else {
        return Err(AdmissionError::NeedsReconcile);
    };
    if !entry.stopped && !read.stop_proven {
        return Err(AdmissionError::NeedsReconcile);
    }
    if !read.resolved {
        return Err(AdmissionError::NeedsReconcile);
    }
    let scope = entry.plan.scope();
    sessions.reconcile.remove(&read.id);
    sessions.release_lease(&scope, read.id);
    Ok(())
}
