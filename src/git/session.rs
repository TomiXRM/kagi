//! Per-tab repository session (ADR-0107).
//!
//! A `RepoSession` owns a [`Backend`] for the lifetime of a repository tab,
//! eliminating the ~96 per-operation `Backend::open` sites that previously
//! re-read config / re-walked `.git` resolution on every interaction
//! (`docs/performance-review.md` §3.1).
//!
//! Today the session is foreground-only (`Rc`, single-thread). When the
//! worker-thread consolidation (ADR-0073) lands, the session becomes the
//! channel owner and the `Rc` swaps to `Arc` — that is a one-line change
//! here, the only place it happens.
//!
//! The session is also the natural owner of future caches (snapshot, diff,
//! graph layout) that Phase 3+ perf work hangs off it.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::{Backend, GitError};

/// One `Backend` owner per repository tab. Cloning is cheap (`Rc` bump);
/// the underlying `git2::Repository` is opened exactly once.
#[derive(Clone)]
pub struct RepoSession {
    backend: Rc<Backend>,
}

impl RepoSession {
    /// Open the repository at `path` and hold the `Backend` for the tab
    /// lifetime. Subsequent `backend()` calls return the same handle without
    /// re-opening.
    pub fn open(path: &Path) -> Result<Self, GitError> {
        let backend = Backend::open(path)?;
        Ok(Self {
            backend: Rc::new(backend),
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

    /// Shared handle to the `Backend`. Cloning the `Rc` is O(1); the
    /// underlying `git2::Repository` is not re-opened.
    ///
    /// Note: `run()` (ADR-0104) takes `&mut self`, so callers that need to
    /// mutate must hold their own `Backend` (the session is `Rc`, not `Arc`
    /// + `Mutex`). Background-task callers open a fresh `Backend` via
    /// `Backend::open(self.path())` for mutation — this is the same pattern
    /// the `*_blocking` fns already use, and will collapse when the worker
    /// thread lands.
    pub fn backend(&self) -> &Backend {
        &self.backend
    }
}
