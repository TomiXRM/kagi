//! Per-tab repository session (ADR-0107 + ADR-0073).
//!
//! A `RepoSession` owns a [`Backend`] for read paths (opened once per tab) and
//! an optional [`RepoWorker`] for write paths (a dedicated thread that owns its
//! own `Backend`, eliminating per-operation re-opens for mutations).
//!
//! Read paths use `session.backend()` (synchronous, `Rc`-shared).
//! Write paths use `session.submit(op, plan)` (sends to the worker thread,
//! returns a receiver for the result).
//!
//! The session is also the natural owner of future caches (snapshot, diff,
//! graph layout) that Phase 3+ perf work hangs off it.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::worker::RepoWorker;
use super::{Backend, GitError, OperationPlan};
use kagi_domain::operation::{Operation, OperationOutcome};
use std::sync::mpsc;

/// One `Backend` owner per repository tab. Cloning is cheap (`Rc` bump);
/// the underlying `git2::Repository` is opened exactly once (for reads) plus
/// once for the worker thread (for writes).
#[derive(Clone)]
pub struct RepoSession {
    backend: Rc<Backend>,
    /// Lazily-spawned worker thread for write operations (ADR-0073).
    /// Shared via `Rc<RefCell>` so all clones of the session see the same
    /// worker once spawned. `None` until the first `submit()` call.
    worker: Rc<std::cell::RefCell<Option<RepoWorker>>>,
}

impl RepoSession {
    /// Open the repository at `path` and hold the `Backend` for the tab
    /// lifetime. The worker thread is spawned lazily on first `submit()`.
    pub fn open(path: &Path) -> Result<Self, GitError> {
        let backend = Backend::open(path)?;
        Ok(Self {
            backend: Rc::new(backend),
            worker: Rc::new(std::cell::RefCell::new(None)),
        })
    }

    /// The workdir path of the open repository (same as `Backend::path()`).
    pub fn path(&self) -> &Path {
        self.backend.path()
    }

    /// The workdir path as an owned `PathBuf` (convenience for callers that
    /// need to move it into a background task).
    pub fn path_buf(&self) -> PathBuf {
        self.backend.path().to_path_buf()
    }

    /// Shared handle to the `Backend` for read paths. Cloning the `Rc` is O(1).
    pub fn backend(&self) -> &Backend {
        &self.backend
    }

    /// Submit a mutating operation to the worker thread (ADR-0073). Returns a
    /// receiver for the `OperationOutcome`. The caller typically awaits this
    /// inside a `cx.background_spawn` task.
    ///
    /// The worker thread owns its own `Backend` (opened once on spawn), so this
    /// does NOT re-open the repo. The operation runs through `Backend::run`
    /// (ADR-0104 enforced pipeline — preflight cannot be bypassed).
    ///
    /// On first call, spawns the worker thread. If the worker fails to spawn
    /// (e.g. the repo disappeared), returns `Err`.
    pub fn submit(
        &self,
        op: Operation,
        plan: OperationPlan,
    ) -> Result<mpsc::Receiver<Result<OperationOutcome, GitError>>, GitError> {
        // Lazily spawn the worker if it doesn't exist yet.
        let mut worker_slot = self.worker.borrow_mut();
        if worker_slot.is_none() {
            let w = RepoWorker::spawn(self.backend.path())?;
            *worker_slot = Some(w);
        }
        let worker = worker_slot.as_ref().unwrap();
        worker.submit(op, plan)
    }
}
