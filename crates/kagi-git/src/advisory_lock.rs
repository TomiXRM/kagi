//! Kernel-released, bounded advisory locks shared by draft and repository writers.
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// Owns a sidecar lock; dropping the final owner unlocks before closing the fd.
#[derive(Debug)]
pub struct AdvisoryLock {
    file: File,
}

impl AdvisoryLock {
    /// Oplog sequence metadata lives in its locked sidecar.
    pub(crate) fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }
}

impl Drop for AdvisoryLock {
    fn drop(&mut self) {
        // CLOEXEC does not prevent an unrelated fork from temporarily inheriting
        // the open-file description. Close alone could leave its flock held
        // until that child execs. File's drop still closes if unlocking fails.
        let _ = self.file.unlock();
    }
}

/// Lock this exact persistent sidecar path; never unlink it on release.
///
/// Dropping the returned guard explicitly releases the lock; process death also
/// releases it in the kernel. A timeout returns `WouldBlock`, distinct from
/// other I/O errors, so admission callers can render a refusal rather than
/// misclassifying an execution error.
/// A zero timeout performs one nonblocking attempt.
pub fn lock_exclusive(path: &Path, timeout: Duration) -> io::Result<AdvisoryLock> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    let deadline = Instant::now() + timeout;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(AdvisoryLock { file }),
            Err(std::fs::TryLockError::WouldBlock) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    // Retain the established append/retirement diagnostic and
                    // a typed WouldBlock, allocating only on the final refusal.
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        std::fs::TryLockError::WouldBlock,
                    ));
                }
                std::thread::sleep(remaining.min(Duration::from_millis(10)));
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
    }
}

#[cfg(all(test, unix))]
pub(crate) mod tests {
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    pub(crate) fn with_unrelated_fork<T>(action: impl FnOnce() -> T) -> T {
        let (mut parent, child) = UnixStream::pair().unwrap();
        parent
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let child_fd = child.as_raw_fd();
        // Hold the fork-to-exec window: CLOEXEC has not closed inherited locks.
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
                let result = libc::poll(&mut signal, 1, 10_000);
                libc::_exit(if result > 0 { 0 } else { 2 });
            }
        }
        drop(child);
        // Reap the bounded child even if the assertion or action panics.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut ready = [0u8];
            parent
                .read_exact(&mut ready)
                .expect("fork child did not confirm descriptor inheritance within 10s");
            action()
        }));
        let _ = parent.write_all(&[1]);
        let mut status = 0;
        // SAFETY: pid is this test's child; status points to initialized storage.
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        assert_eq!(status, 0, "fork child failed or timed out");
        result.unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    }
}
