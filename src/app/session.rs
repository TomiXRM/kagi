use super::worktree::{PlanState, RemoveCompletion};
use kagi_git::backend::remove::RemovePlan;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

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

#[derive(Debug, PartialEq, Eq)]
pub enum AdmissionError {
    Busy,
    StaleApproval,
    NeedsReconcile,
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
    pub plan: RemovePlan,
    pub attachment: Attachment,
}
pub struct Sessions {
    pub(crate) abandoned_tx: std::sync::mpsc::Sender<RemoveCompletion>,
    abandoned_rx: std::sync::mpsc::Receiver<RemoveCompletion>,
    pub(crate) state: PlanState,
    pub(crate) revision: RequestId,
    pub(crate) operations: HashMap<OperationId, InFlight>,
    pub(crate) leases: HashMap<kagi_domain::remove::RepoId, OperationId>,
    pub(crate) stale: HashSet<PathBuf>,
    pub(crate) reconcile: HashMap<OperationId, (RemovePlan, bool)>,
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
            state: PlanState::Draft,
            revision: RequestId(next_id()),
            operations: HashMap::new(),
            leases: HashMap::new(),
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
        !self.leases.is_empty()
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
    pub fn apply(&mut self, completion: RemoveCompletion) -> Vec<Delivery> {
        super::apply(self, completion)
    }
}

#[derive(Clone, Debug)]
pub enum Delivery {
    Invalidate(PathBuf),
    RemovedTarget(PathBuf),
    Completed {
        id: OperationId,
        attachment: Attachment,
        report: Box<kagi_git::backend::remove::RemoveReport>,
    },
}

#[derive(Clone)]
pub struct ReconcileRead {
    id: OperationId,
    pub observation: String,
}
pub struct ReconcileJob {
    id: OperationId,
    plan: RemovePlan,
}
impl ReconcileJob {
    pub fn run(self) -> Result<ReconcileRead, String> {
        let observation =
            kagi_git::Backend::read_remove_status(&self.plan).map_err(|e| e.to_string())?;
        Ok(ReconcileRead {
            id: self.id,
            observation,
        })
    }
}
pub fn prepare_reconcile(sessions: &Sessions, id: OperationId) -> Result<ReconcileJob, String> {
    let (plan, stopped) = sessions.reconcile.get(&id).ok_or("no reconcile request")?;
    if !stopped {
        return Err("execution termination is unconfirmed".into());
    }
    Ok(ReconcileJob {
        id,
        plan: plan.clone(),
    })
}
pub fn read_reconcile(sessions: &Sessions, id: OperationId) -> Result<ReconcileRead, String> {
    prepare_reconcile(sessions, id)?.run()
}
pub fn acknowledge(sessions: &mut Sessions, read: ReconcileRead) -> Result<(), AdmissionError> {
    if !matches!(sessions.reconcile.get(&read.id), Some((_, true))) {
        return Err(AdmissionError::NeedsReconcile);
    }
    sessions.reconcile.remove(&read.id);
    Ok(())
}
