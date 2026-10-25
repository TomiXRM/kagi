//! Repository-wide conflict writer admission (ADR-0196).
//!
//! Read-only plans probe and release; execution reacquires before its final
//! preflight and keeps the descriptor through verification. Never unlink this
//! sidecar: replacing its inode would let two processes lock different files.
use crate::GitError;
use kagi_domain::plan_note::{ConflictsNote, PlanNote};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

thread_local! {
    // Reentrancy is lexical and thread-local, not process-wide permission for
    // independent background writers. Nested backend/executor calls share fd.
    static HELD: RefCell<HashMap<PathBuf, Weak<File>>> = RefCell::new(HashMap::new());
}

#[must_use]
pub(crate) struct RepositoryWriteLock {
    _file: Rc<File>,
}

pub(crate) fn acquire(repo: &git2::Repository) -> Result<RepositoryWriteLock, GitError> {
    // Shared refs require one lock even for different linked worktrees.
    let path = std::fs::canonicalize(repo.commondir())
        .map_err(io)?
        .join("kagi-write.lock");
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        if let Some(file) = held.get(&path).and_then(Weak::upgrade) {
            return Ok(RepositoryWriteLock { _file: file });
        }
        let file = crate::advisory_lock::lock_exclusive(&path, Duration::from_millis(100))
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    GitError::Blocked(Box::new(PlanNote::Conflicts(ConflictsNote::RepositoryBusy)))
                } else {
                    io(error)
                }
            })?;
        let file = Rc::new(file);
        held.retain(|_, file| file.strong_count() != 0);
        held.insert(path, Rc::downgrade(&file));
        Ok(RepositoryWriteLock { _file: file })
    })
}

fn io(error: impl std::fmt::Display) -> GitError {
    GitError::Other(format!("repository write lock: {error}"))
}

/// Plans whose legacy callers only display preparation failures still need a
/// durable refusal. Recorded conflict jobs use their existing receipt owner.
pub(crate) fn acquire_recorded(
    repo: &git2::Repository,
    path: &std::path::Path,
) -> Result<RepositoryWriteLock, GitError> {
    match acquire(repo) {
        Ok(lock) => Ok(lock),
        Err(error) => {
            let recording = crate::backend::recording::finalize(
                crate::oplog::OpLogEntry::new(
                    "conflict-write-admission",
                    path.display().to_string(),
                    crate::ops::StateSummary {
                        head: "not started".into(),
                        dirty: "unchanged".into(),
                    },
                    crate::oplog::OpOutcome::Refused {
                        blockers: vec![error.to_string()],
                    },
                )
                .with_worktree(Some(path.display().to_string()))
                .with_nothing_moved(),
            );
            if let crate::backend::recording::Recording::Failed {
                error: recording_error,
                ..
            } = recording
            {
                return Err(GitError::Other(format!(
                    "{error}; refusal recording failed: {recording_error}"
                )));
            }
            Err(error)
        }
    }
}

impl crate::Backend {
    /// Probe repository admission without recording: the operation owns its receipt.
    pub fn check_conflict_write_admission(path: &std::path::Path) -> Result<(), GitError> {
        let repo = git2::Repository::discover(path).map_err(io)?;
        let _lock = acquire(&repo)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_admission_reuses_fd_but_another_thread_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(root.path()).unwrap();
        let outer = acquire(&repo).unwrap();
        let inner = acquire(&repo).unwrap();
        drop(outer);
        let path = root.path().to_path_buf();
        assert!(std::thread::spawn(move || {
            let repo = git2::Repository::open(path).unwrap();
            matches!(acquire(&repo), Err(GitError::Blocked(note))
                if *note == PlanNote::Conflicts(ConflictsNote::RepositoryBusy))
        })
        .join()
        .unwrap());
        drop(inner);
        let path = root.path().to_path_buf();
        assert!(std::thread::spawn(move || {
            let repo = git2::Repository::open(path).unwrap();
            acquire(&repo).is_ok()
        })
        .join()
        .unwrap());
    }
}
