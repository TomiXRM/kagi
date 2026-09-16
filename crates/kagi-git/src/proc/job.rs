//! Windows' answer to the process group: one **job object** per spawned child
//! (#703b).
//!
//! On unix a stop proof is "nothing is left in the process group" — the child is
//! spawned as its own group leader, so a transport helper or a hook it started
//! is counted with it ([`group_alive`](super::group_alive)). Windows has no such
//! group, which is why every reconcile read there was a dead end: no probe, no
//! proof, and the scope stayed held until kagi restarted (ADR-0196).
//!
//! A job object is the equivalent handle. The child is assigned to a fresh job
//! at spawn and its descendants inherit that membership, so
//! `JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::ActiveProcesses == 0` says the same
//! thing an empty process group does: nothing of that write is still running.
//!
//! The registry is keyed by the child's pid — the same `u32` a
//! [`Termination::Unaccounted`](crate::Termination::Unaccounted) carries — so
//! the supervisor's ownership and probing are spelled identically on both
//! platforms; only what the number is *asked of* differs.
//!
//! Two limits, deliberately on the safe side:
//!
//! - The assignment happens immediately after `spawn`, not before it runs. A
//!   descendant created in that window is outside the job and is not counted.
//!   (`CREATE_SUSPENDED` + `ResumeThread` would close it, but `std`'s
//!   `Command` gives no handle to the initial thread.)
//! - A child that could not be assigned — an older Windows that refuses to nest
//!   an existing job, a failed `CreateJobObject` — is left with no entry, and a
//!   pid with no entry reads as **alive**. No evidence is not "gone".
use std::process::Child;

#[cfg(windows)]
mod platform {
    use std::collections::HashMap;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
        QueryInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    };

    /// An owned job-object handle.
    struct Job(HANDLE);
    // SAFETY: a job object is a kernel handle with no thread affinity, and the
    // registry below only ever hands it out under its mutex.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}
    impl Drop for Job {
        fn drop(&mut self) {
            // Closing the last handle does **not** terminate the job: kagi never
            // sets `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Forgetting our
            // bookkeeping must never kill a write that is still running.
            unsafe { CloseHandle(self.0) };
        }
    }

    fn jobs() -> MutexGuard<'static, HashMap<u32, Job>> {
        static JOBS: OnceLock<Mutex<HashMap<u32, Job>>> = OnceLock::new();
        JOBS.get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// Put `child` in a job of its own, so its whole tree can be probed later.
    pub(super) fn attach(pid: u32, child: &Child) {
        // SAFETY: a fresh unnamed job object with default security.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return; // no job → no entry → the pid reads as alive
        }
        // SAFETY: `handle` is the job just created and the process handle is
        // owned by `child`, which outlives this call.
        let assigned = unsafe { AssignProcessToJobObject(handle, child.as_raw_handle() as HANDLE) };
        if assigned == 0 {
            unsafe { CloseHandle(handle) };
            return;
        }
        jobs().insert(pid, Job(handle));
    }

    /// Is anything in `pid`'s job still running?
    pub(super) fn alive(pid: u32) -> bool {
        let jobs = jobs();
        let Some(job) = jobs.get(&pid) else {
            return true; // never assigned, or already retired: no evidence
        };
        // SAFETY: `info` is the size and shape this information class writes.
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32;
        let queried = unsafe {
            QueryInformationJobObject(
                job.0,
                JobObjectBasicAccountingInformation,
                std::ptr::addr_of_mut!(info).cast(),
                size,
                std::ptr::null_mut(),
            )
        };
        if queried == 0 {
            return true; // the query failed: still no evidence of a stop
        }
        info.ActiveProcesses > 0
    }

    /// Stop the whole tree — the deadline path, matching `kill_group` on unix.
    pub(super) fn terminate(pid: u32) {
        if let Some(job) = jobs().get(&pid) {
            // SAFETY: terminating a job kagi created and still owns.
            unsafe { TerminateJobObject(job.0, 1) };
        }
    }

    /// Proven empty (or never ours): drop the handle.
    pub(super) fn release(pid: u32) {
        jobs().remove(&pid);
    }
}

#[cfg(not(windows))]
mod platform {
    use std::process::Child;

    /// unix proves a stop with the process group itself; nothing to attach.
    pub(super) fn attach(_pid: u32, _child: &Child) {}
    /// Only reached on a platform with neither process groups nor job objects,
    /// where "no evidence" must read as still running (ADR-0175). unix asks the
    /// process group instead, so nothing calls these two there.
    #[cfg_attr(unix, allow(dead_code))]
    pub(super) fn alive(_pid: u32) -> bool {
        true
    }
    #[cfg_attr(unix, allow(dead_code))]
    pub(super) fn terminate(_pid: u32) {}
    pub(super) fn release(_pid: u32) {}
}

pub(super) fn attach(pid: u32, child: &Child) {
    platform::attach(pid, child);
}
/// On unix the probe and the kill are the process group's own
/// ([`super::group`]), so these two have no caller there.
#[cfg_attr(unix, allow(dead_code))]
pub(super) fn alive(pid: u32) -> bool {
    platform::alive(pid)
}
#[cfg_attr(unix, allow(dead_code))]
pub(super) fn terminate(pid: u32) {
    platform::terminate(pid);
}
pub(super) fn release(pid: u32) {
    platform::release(pid);
}

#[cfg(all(test, windows))]
mod tests {
    use super::super::{run_child, ProcStop};
    use crate::Termination;
    use std::process::Command;
    use std::time::Duration;

    /// Something that keeps running until it is stopped. `ping` is the
    /// Windows-native "sleep" and is present on every runner.
    fn long_running() -> Command {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "ping -n 30 127.0.0.1"]);
        cmd
    }

    /// The Windows half of the #703 acceptance condition: a run that times out
    /// is killed through its job, and the job then proves the tree empty — the
    /// same sequence the unix group probe gives, and the reason a reconcile
    /// read on Windows is no longer a dead end.
    #[test]
    fn a_timed_out_run_is_proven_stopped_by_its_job() {
        let mut cmd = long_running();
        let run = run_child(&mut cmd, Duration::from_millis(300), None).expect("spawn");
        assert!(
            matches!(run.status, Err(ProcStop::Deadline { .. })),
            "the deadline must expire rather than report an exit: {:?}",
            run.status
        );
        assert!(
            run.group_stopped,
            "the job object must prove the tree empty once it has been terminated"
        );
        // Which is what the termination is built from: a proven stop, not an
        // `Unaccounted` that no Windows read could ever resolve (#703).
        assert!(
            Termination::from_run("the deadline expired", &run).child_stopped(),
            "a timed-out Windows run must settle as a proven stop"
        );
    }

    /// A child that is still running is not a stop proof — the half that keeps
    /// this from being "release on a timeout".
    #[test]
    fn a_running_job_is_not_a_stop_proof() {
        let mut child = long_running().spawn().expect("spawn ping");
        let pid = child.id();
        super::attach(pid, &child);
        assert!(
            super::alive(pid),
            "the child is running: its job is not empty"
        );

        super::terminate(pid);
        let mut gone = false;
        for _ in 0..200 {
            if !super::alive(pid) {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = child.wait();
        assert!(gone, "terminating the job must empty it");
        super::release(pid);
        assert!(
            super::alive(pid),
            "a pid with no job reads as alive: no evidence is not 'gone'"
        );
    }
}
