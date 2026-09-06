use super::*;
use kagi_domain::remote::RemoteRepoId;
use kagi_domain::remove::RepoId;
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    pub path: PathBuf,
    pub generation: u64,
}

#[derive(Clone, Debug)]
pub enum OwnerAttachment {
    Local(Attachment),
    Remote(crate::remote::stash::RemoteAttachment),
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

pub(crate) struct InFlight {
    pub plan: Planned,
    pub attachment: OwnerAttachment,
}
pub(crate) struct ReconcileEntry {
    pub plan: Planned,
    pub stopped: bool,
    pub remote: Option<crate::remote::stash::RemoteStashEvidence>,
}
pub struct Sessions {
    pub(crate) abandoned_tx: std::sync::mpsc::Sender<Completion>,
    abandoned_rx: std::sync::mpsc::Receiver<Completion>,
    pub(crate) plan_errors: Vec<PlanErrorJob>,
    pub(crate) stash_conflicts: HashMap<PathBuf, StashConflict>,
    pub(crate) stash_followups: HashMap<PathBuf, StashConflict>,
    pub(crate) state: PlanState,
    pub(crate) revision: RequestId,
    pub(crate) operations: HashMap<OperationId, InFlight>,
    pub(crate) leases: Arc<Mutex<HashMap<WriteScope, OperationId>>>,
    pub(crate) stale: HashSet<PathBuf>,
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
            stash_conflicts: HashMap::new(),
            stash_followups: HashMap::new(),
            state: PlanState::Draft,
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
    }
    pub fn is_stale(&self, path: &std::path::Path) -> bool {
        self.stale.contains(path)
    }
    pub fn read_applied(&mut self, path: &std::path::Path) {
        self.stale.remove(path);
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
    /// CLI errors with unconfirmed process termination must retain the lease.
    pub fn complete_git<R>(self, result: &Result<R, kagi_git::GitError>) {
        if !matches!(result, Err(kagi_git::GitError::TerminationUnknown(_))) {
            self.complete();
        }
    }
}

#[derive(Clone, Debug)]
pub enum Delivery {
    Invalidate(PathBuf),
    RemovedTarget(PathBuf),
    Completed {
        id: OperationId,
        attachment: Attachment,
        report: Box<ExecutionReport>,
    },
    RemoteCompleted {
        id: OperationId,
        attachment: crate::remote::stash::RemoteAttachment,
        report: Box<ExecutionReport>,
    },
}

#[derive(Clone)]
pub struct ReconcileRead {
    id: OperationId,
    pub observation: String,
    stop_proven: bool,
}
pub struct ReconcileJob {
    id: OperationId,
    plan: Planned,
    remote: Option<crate::remote::stash::RemoteStashEvidence>,
}
impl ReconcileJob {
    pub fn run(self) -> Result<ReconcileRead, String> {
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
        };
        Ok(ReconcileRead {
            id: self.id,
            observation,
            stop_proven,
        })
    }
}
pub fn prepare_reconcile(sessions: &Sessions, id: OperationId) -> Result<ReconcileJob, String> {
    let entry = sessions.reconcile.get(&id).ok_or("no reconcile request")?;
    if !entry.stopped && entry.remote.is_none() {
        return Err("execution termination is unconfirmed".into());
    }
    Ok(ReconcileJob {
        id,
        plan: entry.plan.clone(),
        remote: entry.remote.clone(),
    })
}
pub fn read_reconcile(sessions: &Sessions, id: OperationId) -> Result<ReconcileRead, String> {
    prepare_reconcile(sessions, id)?.run()
}
pub fn acknowledge(sessions: &mut Sessions, read: ReconcileRead) -> Result<(), AdmissionError> {
    let Some(entry) = sessions.reconcile.get(&read.id) else {
        return Err(AdmissionError::NeedsReconcile);
    };
    if !entry.stopped && !read.stop_proven {
        return Err(AdmissionError::NeedsReconcile);
    }
    let scope = entry.plan.scope();
    sessions.reconcile.remove(&read.id);
    sessions.release_lease(&scope, read.id);
    Ok(())
}
