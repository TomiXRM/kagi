//! Repository worker thread (ADR-0073).
//!
//! A dedicated thread per `RepoSession` that owns the `Backend` exclusively.
//! The UI sends operations via an mpsc channel; the worker executes them and
//! returns results via a oneshot channel.
//!
//! Benefits over the per-op `Backend::open` pattern:
//! 1. **Opens once** — the `git2::Repository` lives for the tab lifetime.
//! 2. **Serializes mutations** — git ops on the same repo are not thread-safe;
//!    the single-threaded receive loop guarantees no concurrency.
//! 3. **No lock contention** — no `Arc<Mutex<Backend>>`; the thread IS the owner.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use super::{Backend, GitError, OperationPlan};
use kagi_domain::operation::{Operation, OperationOutcome};

/// A request sent from the UI to the worker thread.
enum WorkRequest {
    /// Execute a mutating operation via `Backend::run` (ADR-0104).
    Run {
        op: Operation,
        plan: OperationPlan,
        reply: mpsc::Sender<Result<OperationOutcome, GitError>>,
    },
    /// Shut down the worker thread cleanly.
    Shutdown,
}

/// Handle to a dedicated repository worker thread. The thread owns a `Backend`
/// for the lifetime of the tab; the UI communicates via channels.
pub struct RepoWorker {
    tx: mpsc::Sender<WorkRequest>,
    path: PathBuf,
    join: Option<thread::JoinHandle<()>>,
}

impl RepoWorker {
    /// Spawn a worker thread that opens the repository at `path` and waits for
    /// operations on the channel. Returns a handle for sending requests.
    pub fn spawn(path: &Path) -> Result<Self, GitError> {
        let (tx, rx) = mpsc::channel::<WorkRequest>();
        let worker_path = path.to_path_buf();

        let backend = Backend::open(&worker_path)?;
        let join = thread::Builder::new()
            .name(format!("kagi-repo-worker({})", worker_path.display()))
            .spawn(move || {
                Self::worker_loop(backend, rx);
            })
            .map_err(|e| GitError::Other(format!("worker thread spawn failed: {}", e)))?;

        Ok(Self {
            tx,
            path: worker_path,
            join: Some(join),
        })
    }

    /// The repository path this worker was opened for.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Send a mutating operation to the worker and return a receiver for the
    /// result. The caller awaits `recv()` (typically via a background task).
    ///
    /// The operation runs through `Backend::run` (ADR-0104) on the worker
    /// thread — preflight is enforced, no bypass.
    pub fn submit(
        &self,
        op: Operation,
        plan: OperationPlan,
    ) -> Result<mpsc::Receiver<Result<OperationOutcome, GitError>>, GitError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(WorkRequest::Run {
                op,
                plan,
                reply: reply_tx,
            })
            .map_err(|e| GitError::Other(format!("worker channel send failed: {}", e)))?;
        Ok(reply_rx)
    }

    /// Shut down the worker thread cleanly. Sends `Shutdown` and joins.
    /// Safe to call multiple times (subsequent sends are no-ops on a closed
    /// channel).
    pub fn shutdown(&mut self) {
        let _ = self.tx.send(WorkRequest::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }

    /// The worker thread's main loop. Opens the backend, then receives and
    /// executes requests until `Shutdown` or the channel closes.
    fn worker_loop(mut backend: Backend, rx: mpsc::Receiver<WorkRequest>) {
        while let Ok(req) = rx.recv() {
            match req {
                WorkRequest::Run { op, plan, reply } => {
                    let result = backend.run(&op, &plan);
                    // If the reply channel is closed (caller dropped), the
                    // result is silently discarded — the operation still ran.
                    let _ = reply.send(result);
                }
                WorkRequest::Shutdown => break,
            }
        }
        // Thread exits when the channel closes or Shutdown is received.
        // The `backend` (and its `git2::Repository`) is dropped here.
    }
}

impl Drop for RepoWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_test_repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        for (k, v) in [
            ("GIT_AUTHOR_NAME", "test"),
            ("GIT_AUTHOR_EMAIL", "test@test"),
            ("GIT_COMMITTER_NAME", "test"),
            ("GIT_COMMITTER_EMAIL", "test@test"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
        ] {
            std::env::set_var(k, v);
        }
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(["-C"])
                .arg(dir)
                .args(args)
                .output()
                .expect("git")
        };
        git(&["init", "-q"]);
        git(&["config", "user.name", "test"]);
        git(&["config", "user.email", "test@test"]);
        std::fs::write(dir.join("README.md"), "# test\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "init"]);
        tmp
    }

    #[test]
    fn worker_spawns_and_shuts_down() {
        let tmp = init_test_repo();
        let mut worker = RepoWorker::spawn(tmp.path()).expect("spawn");
        worker.shutdown();
        // Drop should be safe after explicit shutdown.
        drop(worker);
    }

    #[test]
    fn worker_executes_create_branch() {
        let tmp = init_test_repo();
        let session = super::super::session::RepoSession::open(tmp.path()).expect("session");

        // Build a plan for CreateBranch (requires a commit id as `at`).
        let backend = session.backend();
        let head = backend.head_state().expect("head");
        let at = match &head {
            crate::Head::Attached { target, .. } => crate::CommitId(target.clone()),
            _ => panic!("expected attached HEAD"),
        };

        let op = Operation::CreateBranch {
            name: "test-branch".to_string(),
            at,
        };
        let plan = backend.plan(&op).expect("plan");

        let rx = session.submit(op, plan).expect("submit");
        let result = rx.recv().expect("recv");
        assert!(result.is_ok(), "create-branch should succeed: {:?}", result);

        // Verify the branch exists.
        assert!(
            backend.local_branch_exists("test-branch"),
            "test-branch should exist after worker executed CreateBranch"
        );
    }

    #[test]
    fn worker_rejects_stale_plan() {
        let tmp = init_test_repo();
        let session = super::super::session::RepoSession::open(tmp.path()).expect("session");
        let backend = session.backend();
        let head = backend.head_state().expect("head");
        let at = match &head {
            crate::Head::Attached { target, .. } => crate::CommitId(target.clone()),
            _ => panic!("expected attached HEAD"),
        };

        // Build a plan, then move HEAD (create a new commit) so the plan is stale.
        let op = Operation::CreateBranch {
            name: "before-stale".to_string(),
            at: at.clone(),
        };
        let plan = backend.plan(&op).expect("plan");

        // Move HEAD by creating another commit.
        std::fs::write(tmp.path().join("new.txt"), "new\n").unwrap();
        std::process::Command::new("git")
            .args(["-C"])
            .arg(tmp.path())
            .args(["add", "."])
            .output()
            .expect("git add");
        std::process::Command::new("git")
            .args(["-C"])
            .arg(tmp.path())
            .args(["commit", "-qm", "second"])
            .output()
            .expect("git commit");

        // Now submit the stale plan — run() should reject via preflight.
        let rx = session.submit(op, plan).expect("submit");
        let result = rx.recv().expect("recv");
        // Preflight should fail because HEAD moved since the plan was built.
        assert!(
            result.is_err(),
            "stale plan should be rejected by preflight: {:?}",
            result
        );
    }
}
