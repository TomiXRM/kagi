//! Repository-wide conflict writer admission (ADR-0196).
//!
//! Read-only plans probe and release; execution reacquires before its final
//! preflight and keeps the descriptor through verification. An unconfirmed
//! subprocess hands ownership to its reaper until the whole group stops.
//! Never unlink this sidecar: replacing its inode would split exclusion.
use crate::GitError;
use kagi_domain::plan_note::{ConflictsNote, PlanNote};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

pub(crate) struct RepositoryLockFile {
    file: File,
    active_guards: AtomicUsize,
    reentrant: AtomicBool,
}

impl Drop for RepositoryLockFile {
    fn drop(&mut self) {
        // Only the final lexical/reaper owner releases admission. Closing alone
        // can leave flock held by an unrelated fork until that child execs,
        // even though the descriptor is CLOEXEC. Unlock the shared open-file
        // description now; File's drop still closes it if unlocking fails.
        let _ = self.file.unlock();
    }
}

thread_local! {
    // Reentrancy is lexical and thread-local, not process-wide permission for
    // independent background writers. Nested backend/executor calls share fd.
    static HELD: RefCell<HashMap<PathBuf, Weak<RepositoryLockFile>>> = RefCell::new(HashMap::new());
}

#[must_use]
pub(crate) struct RepositoryWriteLock {
    file: Arc<RepositoryLockFile>,
    // Only the fd's monitor ownership crosses threads, never lexical admission.
    _thread: PhantomData<Rc<()>>,
}

impl Drop for RepositoryWriteLock {
    fn drop(&mut self) {
        self.file.active_guards.fetch_sub(1, Ordering::Relaxed);
    }
}

pub(crate) fn acquire(repo: &git2::Repository) -> Result<RepositoryWriteLock, GitError> {
    acquire_with_timeout(repo, Duration::from_millis(100))
}

fn acquire_with_timeout(
    repo: &git2::Repository,
    timeout: Duration,
) -> Result<RepositoryWriteLock, GitError> {
    // Shared refs require one lock even for different linked worktrees.
    let path = std::fs::canonicalize(repo.commondir())
        .map_err(io)?
        .join("kagi-write.lock");
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        if let Some(file) = held
            .get(&path)
            .and_then(Weak::upgrade)
            .filter(|file| file.reentrant.load(Ordering::Relaxed))
        {
            file.active_guards.fetch_add(1, Ordering::Relaxed);
            return Ok(RepositoryWriteLock {
                file,
                _thread: PhantomData,
            });
        }
        let file = crate::advisory_lock::lock_exclusive(&path, timeout).map_err(|error| {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                GitError::Blocked(Box::new(PlanNote::Conflicts(ConflictsNote::RepositoryBusy)))
            } else {
                io(error)
            }
        })?;
        let file = Arc::new(RepositoryLockFile {
            file,
            active_guards: AtomicUsize::new(1),
            reentrant: AtomicBool::new(true),
        });
        held.retain(|_, file| file.strong_count() != 0);
        held.insert(path, Arc::downgrade(&file));
        Ok(RepositoryWriteLock {
            file,
            _thread: PhantomData,
        })
    })
}

/// Retain admission in the subprocess reaper before an unconfirmed run returns.
/// Quarantine revokes lexical reentrancy: even this executor's next probe must
/// contend with the still-running writer. Keep the weak entry so every further
/// child under an existing outer guard also retains the same descriptor until
/// its own group stops; one child's stop cannot release another live writer.
pub(crate) fn retain_for_termination() -> Vec<Arc<RepositoryLockFile>> {
    HELD.with(|held| {
        held.borrow()
            .values()
            .filter_map(Weak::upgrade)
            .filter(|file| file.active_guards.load(Ordering::Relaxed) != 0)
            .inspect(|file| file.reentrant.store(false, Ordering::Relaxed))
            .collect()
    })
}

fn io(error: impl std::fmt::Display) -> GitError {
    GitError::Other(format!("repository write lock: {error}"))
}

/// Legacy planning attempts own one receipt; return it intact for presentation
/// and required failed-append acknowledgment. Generic probes remain unrecorded.
pub(crate) fn acquire_recorded(
    repo: &git2::Repository,
    path: &std::path::Path,
    operation: &str,
) -> Result<RepositoryWriteLock, GitError> {
    match acquire(repo) {
        Ok(lock) => Ok(lock),
        Err(error) => {
            let recording = crate::backend::recording::finalize(
                crate::oplog::OpLogEntry::new(
                    operation,
                    path.display().to_string(),
                    crate::ops::StateSummary {
                        head: "unobserved".into(),
                        dirty: "unobserved".into(),
                    },
                    crate::oplog::OpOutcome::Refused {
                        blockers: vec![error.to_string()],
                    },
                )
                .with_worktree(Some(path.display().to_string()))
                .with_nothing_moved(),
            );
            Err(GitError::Recorded {
                error: Box::new(error),
                recording: Box::new(recording),
            })
        }
    }
}

impl crate::Backend {
    /// Probe repository admission without waiting or recording: the operation owns
    /// its receipt. Planners and executors retain their bounded admission timeout.
    pub fn check_conflict_write_admission(path: &std::path::Path) -> Result<(), GitError> {
        let repo = git2::Repository::discover(path).map_err(io)?;
        let _lock = acquire_with_timeout(&repo, Duration::ZERO)?;
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

    #[test]
    fn quarantine_revokes_reentrancy_and_retains_each_unconfirmed_run() {
        let root = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(root.path()).unwrap();
        let outer = acquire(&repo).unwrap();
        let first_run = retain_for_termination();
        assert_eq!(first_run.len(), 1);
        assert!(matches!(
            acquire_with_timeout(&repo, Duration::ZERO),
            Err(GitError::Blocked(note))
                if *note == PlanNote::Conflicts(ConflictsNote::RepositoryBusy)
        ));
        let second_run = retain_for_termination();
        assert_eq!(second_run.len(), 1);
        assert!(Arc::ptr_eq(&first_run[0], &second_run[0]));
        drop(outer);
        // A later unrelated read on this executor must not attach its process
        // to a previous operation whose lexical guards have all returned.
        assert!(retain_for_termination().is_empty());
        drop(first_run);
        let path = root.path().to_path_buf();
        assert!(std::thread::spawn(move || {
            let repo = git2::Repository::open(path).unwrap();
            acquire_with_timeout(&repo, Duration::ZERO).is_err()
        })
        .join()
        .unwrap());
        drop(second_run);
        drop(acquire_with_timeout(&repo, Duration::ZERO).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn final_retained_handle_releases_lock_while_unrelated_fork_is_alive() {
        use std::io::{Read, Write};
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;

        let root = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(root.path()).unwrap();
        let outer = acquire(&repo).unwrap();
        let retained = retain_for_termination();
        drop(outer);
        let (mut parent, child) = UnixStream::pair().unwrap();
        parent
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let child_fd = child.as_raw_fd();
        // Reproduce the fork-to-exec window of an unrelated concurrent command.
        // CLOEXEC cannot close its inherited lock descriptor until exec runs.
        // SAFETY: the child calls only async-signal-safe libc functions and
        // _exit; it never runs Rust destructors or touches inherited mutexes.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork failed: {}", std::io::Error::last_os_error());
        if pid == 0 {
            unsafe {
                let ready = [1u8];
                if libc::write(child_fd, ready.as_ptr().cast(), 1) != 1 {
                    libc::_exit(1);
                }
                let mut signal = libc::pollfd {
                    fd: child_fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                // Bound child cleanup even if a parent assertion panics.
                let result = libc::poll(&mut signal, 1, 10_000);
                libc::_exit(if result > 0 { 0 } else { 2 });
            }
        }
        drop(child);
        let mut ready = [0u8];
        parent
            .read_exact(&mut ready)
            .expect("fork child did not confirm descriptor inheritance within 10s");
        drop(retained);
        let admission = acquire_with_timeout(&repo, Duration::ZERO);
        parent.write_all(&[1]).unwrap();
        let mut status = 0;
        // SAFETY: pid is this test's child; status points to initialized storage.
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        assert_eq!(status, 0, "fork child failed or timed out");
        drop(
            admission.expect(
                "final retained handle must release admission before an unrelated fork execs",
            ),
        );
    }
}
