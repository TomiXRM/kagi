//! Repository-wide conflict writer admission (ADR-0196).
//!
//! Read-only plans probe and release; execution reacquires before its final
//! preflight and keeps the descriptor through verification. Never unlink this
//! sidecar: replacing its inode would let two processes lock different files.
use crate::GitError;
use kagi_domain::plan_note::{ConflictsNote, PlanNote};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

thread_local! {
    // Reentrancy is lexical and thread-local, not process-wide permission for
    // independent background writers. Nested backend/executor calls share fd.
    static HELD: RefCell<HashMap<PathBuf, Weak<File>>> = RefCell::new(HashMap::new());
}

#[must_use]
pub struct RepositoryWriteLock {
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
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(io)?;
        let deadline = Instant::now() + Duration::from_millis(100);
        loop {
            // std uses flock(LOCK_EX | LOCK_NB) on Unix and the corresponding
            // descriptor-based kernel lock on Windows. Closing releases it,
            // including process death; the empty sidecar intentionally remains.
            match file.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err(GitError::Blocked(Box::new(PlanNote::Conflicts(
                        ConflictsNote::RepositoryBusy,
                    ))));
                }
                Err(error) => return Err(io(error)),
            }
        }
        let file = Rc::new(file);
        held.retain(|_, file| file.strong_count() != 0);
        held.insert(path, Rc::downgrade(&file));
        Ok(RepositoryWriteLock { _file: file })
    })
}

fn io(error: impl std::fmt::Display) -> GitError {
    GitError::Other(format!("repository write lock: {error}"))
}

impl crate::Backend {
    /// The existing application admission probes the same backend gate as plans.
    /// Execution reacquires it and revalidates the approval before mutation.
    pub fn check_conflict_write_admission(path: &std::path::Path) -> Result<(), GitError> {
        let repo = git2::Repository::discover(path).map_err(io)?;
        let _lock = match acquire(&repo) {
            Ok(lock) => lock,
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
                return Err(error);
            }
        };
        Ok(())
    }
}
