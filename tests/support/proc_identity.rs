//! A process's identity as the macOS kernel records it, and SIGKILL through
//! `kill(2)` — the GUI E2E runner's KEEP_GOING cleanup (#967 review).
//!
//! A pid alone is a number the OS hands out again. A pid plus the start time
//! `ps -o lstart` prints is not enough either: that time is whole seconds, so
//! a pid reused within the same second looks like the process that was
//! recorded. The kernel keeps the start time to the microsecond
//! (`proc_pidinfo(PROC_PIDTBSDINFO)`: `pbi_start_tvsec` / `pbi_start_tvusec`),
//! and that is what identifies a process here.
//!
//! Signals go through `kill(2)` itself, not `/bin/kill`: a scenario that used
//! up the process limit leaves nothing to spawn, and a cleanup that cannot
//! stop the runner would then wait for it forever.
//!
//! Declared here rather than taken from the `libc` crate, which the root
//! crate does not depend on.

use std::ffi::{c_int, c_void};

/// When a process started, to the microsecond.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Started {
    pub sec: u64,
    pub usec: u64,
}

/// What a recorded `(pid, Started)` is now.
#[derive(Debug, PartialEq, Eq)]
pub enum Identity {
    /// The pid is the process that was recorded.
    Same,
    /// That process is gone; the pid is free or someone else's.
    Gone,
    /// The kernel would not say. Never signalled: a process left running
    /// is reported, an unrelated one killed is not undone.
    Unknown(String),
}

/// `struct proc_bsdinfo`, `<sys/proc_info.h>` (136 bytes). Every field is
/// spelled out so the layout matches; only the status and start are read.
#[allow(dead_code)]
#[repr(C)]
struct ProcBsdInfo {
    pbi_flags: u32,
    pbi_status: u32,
    pbi_xstatus: u32,
    pbi_pid: u32,
    pbi_ppid: u32,
    pbi_uid: u32,
    pbi_gid: u32,
    pbi_ruid: u32,
    pbi_rgid: u32,
    pbi_svuid: u32,
    pbi_svgid: u32,
    rfu_1: u32,
    pbi_comm: [u8; 16],
    pbi_name: [u8; 32],
    pbi_nfiles: u32,
    pbi_pgid: u32,
    pbi_pjobc: u32,
    e_tdev: u32,
    e_tpgid: u32,
    pbi_nice: i32,
    pbi_start_tvsec: u64,
    pbi_start_tvusec: u64,
}

const PROC_PIDTBSDINFO: c_int = 3;
/// `SZOMB`, `<sys/proc.h>`: exited, waiting for its parent to reap it.
const SZOMB: u32 = 5;
const SIGKILL: c_int = 9;
const ESRCH: i32 = 3;

extern "C" {
    fn proc_pidinfo(pid: c_int, flavor: c_int, arg: u64, buffer: *mut c_void, size: c_int)
        -> c_int;
    fn kill(pid: c_int, signal: c_int) -> c_int;
}

/// When `pid` started. `Ok(None)`: no such process, or one that has exited
/// and only waits to be reaped.
pub fn started(pid: u32) -> Result<Option<Started>, String> {
    let size = std::mem::size_of::<ProcBsdInfo>() as c_int;
    let mut info = std::mem::MaybeUninit::<ProcBsdInfo>::zeroed();
    // SAFETY: the buffer is `size` bytes, the layout `<sys/proc_info.h>`'s.
    let wrote = unsafe {
        proc_pidinfo(
            pid as c_int,
            PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if wrote != size {
        let error = std::io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(ESRCH) => Ok(None),
            _ => Err(format!("proc_pidinfo({pid}): {error}")),
        };
    }
    // SAFETY: the kernel filled all `size` bytes.
    let info = unsafe { info.assume_init() };
    if info.pbi_status == SZOMB {
        return Ok(None);
    }
    Ok(Some(Started {
        sec: info.pbi_start_tvsec,
        usec: info.pbi_start_tvusec,
    }))
}

/// Is `pid` still the process that started at `recorded`?
pub fn identity(pid: u32, recorded: Started) -> Identity {
    match started(pid) {
        Ok(Some(now)) if now == recorded => Identity::Same,
        Ok(_) => Identity::Gone,
        Err(error) => Identity::Unknown(error),
    }
}

/// SIGKILL `pid` (negative: the process group `-pid`). Nothing left to
/// signal (`ESRCH`) is not an error.
pub fn sigkill(pid: i32) -> Result<(), String> {
    // SAFETY: a plain syscall; the caller picks whom to signal.
    if unsafe { kill(pid, SIGKILL) } == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(ESRCH) => Ok(()),
        _ => Err(format!("kill({pid}, SIGKILL): {error}")),
    }
}
