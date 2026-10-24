//! Kernel-released, bounded advisory locks shared by draft and repository writers.
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// Lock this exact persistent sidecar path; never unlink it on release.
///
/// Closing the returned file (including process death) releases the lock. A
/// timeout returns `WouldBlock`, distinct from other I/O errors, so admission
/// callers can render a refusal rather than misclassifying an execution error.
/// A zero timeout performs one nonblocking attempt.
pub fn lock_exclusive(path: &Path, timeout: Duration) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    let deadline = Instant::now() + timeout;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
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
