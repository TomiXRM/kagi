//! Windows' answer to the process group: one **job object** per spawned child
//! (#703b).
//!
//! On unix a stop proof is "nothing is left in the process group" — the child is
//! spawned as its own group leader, so a transport helper or a hook it started
//! is counted with it ([`group_alive`](super::group_alive)). Windows has no such
//! group, which is why every reconcile read there was a dead end: no probe, no
//! proof, and the scope stayed held until kagi restarted (ADR-0196).
//!
//! A job object is the equivalent handle. `ActiveProcesses == 0` says what an
//! empty process group says: nothing of that write is still running. Two things
//! have to be true for that to be *evidence* rather than a guess, and both are
//! this module's job (#726 review):
//!
//! 1. **Nothing escapes the job.** The child is created suspended, assigned to
//!    the job, and only then resumed, so it cannot have started a transport
//!    helper or a hook before the assignment. A descendant born in that window
//!    would not be counted, and the tracked child exiting would then read as
//!    "everything stopped" while that descendant kept writing.
//! 2. **The handle is not addressed by a recyclable name.** A pid is recycled by
//!    the OS once the process is reaped — which is exactly the state an
//!    `Unaccounted` termination is in (child reaped, descendants alive). Jobs
//!    are therefore keyed by a **stop key**: a number minted once per spawn,
//!    never reused, and never confused with a pid that came back.
//!
//! On unix the stop key *is* the process group id, because there the number is
//! the handle — `kill(-pgid, 0)` asks the kernel directly. Both platforms carry
//! the same `u32` through
//! [`Termination::Unaccounted`](crate::Termination::Unaccounted); only what that
//! number is asked *of* differs.
//!
//! A child with no job — `CreateJobObject` failed, an older Windows refused to
//! nest an existing job, the query failed — reads as **alive**. No evidence is
//! not "gone" (ADR-0175).
//!
//! Which is why a proof, once made, is **kept**: an emptied job leaves a
//! tombstone behind rather than disappearing (#726 review P2). A reconcile read
//! probes before it reads the repository, and that read can fail — an
//! `ls-remote` that times out, a repository that will not open. Forgetting the
//! proof between the probe and the retry would turn "proven stopped" back into
//! "no evidence", and the scope would be held until kagi restarts.
//!
//! The other end of the lifetime: a run that exits cleanly, with every pipe
//! closed, can still leave something of its own inside the job — a hook that
//! started a daemon. No receipt names that key, so nothing will ever probe it;
//! [`watch_until_empty`] hands it to the sweeper, which drops the handle when
//! that tree finally goes.
use std::process::Child;

/// Flags `run_child` adds to a `Command` before spawning, so the child can be
/// put in its job before it runs.
#[cfg(windows)]
pub(super) const SPAWN_FLAGS: u32 = 0x0000_0004; // CREATE_SUSPENDED

#[cfg(windows)]
mod platform {
    use std::collections::HashMap;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
        QueryInformationJobObject, TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    /// What the registry knows about one spawn: the job itself, or the fact
    /// that it was once observed empty — a proof that outlives the handle.
    enum Entry {
        Live(Job),
        Stopped,
    }

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

    fn jobs() -> MutexGuard<'static, HashMap<u32, Entry>> {
        static JOBS: OnceLock<Mutex<HashMap<u32, Entry>>> = OnceLock::new();
        JOBS.get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// One spawn's identity, minted here and never reused — unlike the pid it
    /// replaces, which the OS hands out again once the child is reaped.
    fn next_key() -> u32 {
        static NEXT: AtomicU32 = AtomicU32::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    /// Put `child` in a job of its own and return the key a later probe asks
    /// about. The child is still suspended here; [`resume`] is what lets it run,
    /// so nothing it starts can be outside the job.
    pub(super) fn attach(_pid: u32, child: &Child) -> u32 {
        let key = next_key();
        // SAFETY: a fresh unnamed job object with default security.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return key; // no job → no entry → the key reads as alive
        }
        // SAFETY: `handle` is the job just created and the process handle is
        // owned by `child`, which outlives this call.
        let assigned = unsafe { AssignProcessToJobObject(handle, child.as_raw_handle() as HANDLE) };
        if assigned == 0 {
            unsafe { CloseHandle(handle) };
            return key;
        }
        jobs().insert(key, Entry::Live(Job(handle)));
        key
    }

    /// Let a child created with `CREATE_SUSPENDED` run.
    ///
    /// `std`'s `Command` hands back no handle to the initial thread, so the
    /// thread is found by walking the system thread snapshot for this pid. A
    /// suspended child has exactly one thread; resuming every thread it owns
    /// stays correct if that ever changes.
    ///
    /// Failing to resume leaves the child suspended, where the run's own
    /// deadline kills it — the safe direction, since a child that never ran has
    /// written nothing.
    pub(super) fn resume(pid: u32) {
        // SAFETY: documented Win32 calls; every handle opened here is closed,
        // and `dwSize` is set before each snapshot read as the API requires.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return;
            }
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut found = Thread32First(snapshot, &mut entry);
            while found != 0 {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if !thread.is_null() {
                        ResumeThread(thread);
                        CloseHandle(thread);
                    }
                }
                entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
                found = Thread32Next(snapshot, &mut entry);
            }
            CloseHandle(snapshot);
        }
    }

    /// Is anything in `key`'s job still running?
    ///
    /// An empty job is recorded as such before the handle is dropped, so every
    /// later ask — a retried reconcile read — gets the same answer (#726
    /// review P2).
    pub(super) fn alive(key: u32) -> bool {
        let mut jobs = jobs();
        let job = match jobs.get(&key) {
            Some(Entry::Live(job)) => job,
            // Proven empty earlier: the proof is the entry now.
            Some(Entry::Stopped) => return false,
            None => return true, // never assigned, or retired: no evidence
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
        if info.ActiveProcesses > 0 {
            return true;
        }
        // Empty. Keep the fact, drop the handle.
        jobs.insert(key, Entry::Stopped);
        false
    }

    /// Stop the whole tree — the deadline path, matching `kill_group` on unix.
    pub(super) fn terminate(key: u32) {
        if let Some(Entry::Live(job)) = jobs().get(&key) {
            // SAFETY: terminating a job kagi created and still owns.
            unsafe { TerminateJobObject(job.0, 1) };
        }
    }

    /// This key is finished with: drop the handle and the proof alike.
    pub(super) fn release(key: u32) {
        jobs().remove(&key);
    }

    /// Keys no receipt names, whose job was not empty when their run ended.
    fn watched() -> MutexGuard<'static, Vec<u32>> {
        static WATCHED: OnceLock<Mutex<Vec<u32>>> = OnceLock::new();
        WATCHED
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// Hand `key` to the sweeper: nothing will probe it, so it is the sweeper
    /// that notices the tree going and drops the handle (#726 review P2).
    ///
    /// One thread for all of them, and it only wakes once a second: a daemon a
    /// hook started may outlive the whole session, and that is not a reason to
    /// spin. A key whose tree never goes keeps its handle — which is honest,
    /// because something kagi started is still running.
    pub(super) fn watch_until_empty(key: u32) {
        static SWEEPER: std::sync::Once = std::sync::Once::new();
        watched().push(key);
        SWEEPER.call_once(|| {
            std::thread::spawn(|| loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let keys: Vec<u32> = watched().clone();
                for key in keys {
                    if alive(key) {
                        continue;
                    }
                    release(key);
                    watched().retain(|watched| *watched != key);
                }
            });
        });
    }

    /// Test seam: does the registry still hold anything for `key`?
    #[cfg(test)]
    pub(super) fn is_tracked(key: u32) -> bool {
        jobs().contains_key(&key)
    }
}

#[cfg(not(windows))]
mod platform {
    use std::process::Child;

    /// unix proves a stop with the process group itself, and there the group id
    /// *is* the handle — so the pid the child leads is the stop key.
    pub(super) fn attach(pid: u32, _child: &Child) -> u32 {
        pid
    }
    /// Nothing is spawned suspended off Windows.
    pub(super) fn resume(_pid: u32) {}
    /// No handle to sweep where the process group is the handle.
    pub(super) fn watch_until_empty(_key: u32) {}
    /// Only reached on a platform with neither process groups nor job objects,
    /// where "no evidence" must read as still running (ADR-0175). unix asks the
    /// process group instead, so nothing calls these two there.
    #[cfg_attr(unix, allow(dead_code))]
    pub(super) fn alive(_key: u32) -> bool {
        true
    }
    #[cfg_attr(unix, allow(dead_code))]
    pub(super) fn terminate(_key: u32) {}
    pub(super) fn release(_key: u32) {}
}

/// Bind `child` to whatever this platform can later prove a stop against, and
/// return the key that names it.
pub(super) fn attach(pid: u32, child: &Child) -> u32 {
    platform::attach(pid, child)
}
/// Let a child that was spawned suspended run, now that it is bound.
pub(super) fn resume(pid: u32) {
    platform::resume(pid);
}
/// On unix the probe and the kill are the process group's own
/// ([`super::group`]), so these two have no caller there.
#[cfg_attr(unix, allow(dead_code))]
pub(super) fn alive(key: u32) -> bool {
    platform::alive(key)
}
#[cfg_attr(unix, allow(dead_code))]
pub(super) fn terminate(key: u32) {
    platform::terminate(key);
}
/// Retire a key nothing will ask about again.
pub(super) fn release(key: u32) {
    platform::release(key);
}
/// Let the sweeper own a key whose run ended while its tree was still alive and
/// whose termination names nobody — see [`platform::watch_until_empty`].
pub(super) fn watch_until_empty(key: u32) {
    platform::watch_until_empty(key);
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

    /// The child is created suspended so it cannot outrun its job assignment,
    /// which means the resume has to actually happen. If it did not, this run
    /// would never exit: it would sit suspended until the deadline killed it
    /// (#726 review P1).
    #[test]
    fn a_suspended_child_is_resumed_and_exits_on_its_own() {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "exit 0"]);
        let run = run_child(&mut cmd, Duration::from_secs(30), None).expect("spawn");
        assert_eq!(
            run.status.as_ref().ok().copied(),
            Some(0),
            "a suspended child that is never resumed cannot exit: {:?}",
            run.status
        );
        assert!(run.group_stopped, "and its job is empty once it has gone");
    }

    /// What the suspended spawn buys: a descendant is inside the job, so the
    /// direct child exiting is **not** a stop proof while that descendant runs.
    /// This is the shape the P1 described — `ActiveProcesses == 0` must not be
    /// reachable while something kagi started is still writing.
    #[test]
    fn a_descendant_that_outlives_the_child_keeps_the_job_alive() {
        // `start /B` hands the ping to a new process and lets cmd exit.
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start /B ping -n 30 127.0.0.1"]);
        let run = run_child(&mut cmd, Duration::from_secs(10), None).expect("spawn");
        assert!(
            !run.group_stopped,
            "the direct child exited, but its descendant is still in the job"
        );
        assert!(
            super::alive(run.stop_key),
            "the job must still count the descendant"
        );
        assert_eq!(
            Termination::from_run("the child exited", &run).group(),
            Some(run.stop_key),
            "so the termination is the probeable one, not a stop proof"
        );

        super::terminate(run.stop_key);
        let mut gone = false;
        for _ in 0..200 {
            if !super::alive(run.stop_key) {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(gone, "terminating the job must empty it");
        super::release(run.stop_key);
        assert!(
            super::alive(run.stop_key),
            "a key with no job reads as alive: no evidence is not 'gone'"
        );
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

    /// #726 review P2: a reconcile read probes **before** it reads the
    /// repository, and that read can fail. The proof has to survive the retry,
    /// or a scope that was provably free goes back to "no evidence" and stays
    /// held until kagi restarts.
    #[test]
    fn a_proven_stop_survives_a_retry() {
        let mut child = long_running().spawn().expect("spawn");
        let key = super::attach(child.id(), &child);
        assert!(
            !crate::proc::supervisor::group_stopped(key),
            "precondition: the job is not empty while the child runs"
        );
        child.kill().expect("stop it");
        child.wait().expect("reap it");

        let mut proven = false;
        for _ in 0..200 {
            if crate::proc::supervisor::group_stopped(key) {
                proven = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(proven, "an empty job must be provable at all");
        assert!(
            crate::proc::supervisor::group_stopped(key),
            "asking again — the retry after a failed repository read — must \
             give the same proof, not fall back to 'no evidence'"
        );
        super::release(key);
    }

    /// #726 review P2: a run can exit cleanly, with every pipe closed, and
    /// still leave something inside its job — a hook that started a daemon. No
    /// termination is built from a run like that, so nothing will ever probe
    /// its key: the sweeper is what eventually drops the handle.
    #[test]
    fn a_clean_run_that_leaves_a_descendant_is_swept_when_the_tree_goes() {
        // `start /B` with the streams redirected: the descendant holds no pipe,
        // so the capture completes and the run is an ordinary success.
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start /B ping -n 30 127.0.0.1 > nul 2>&1"]);
        let run = run_child(&mut cmd, Duration::from_secs(10), None).expect("spawn");
        assert!(run.status.is_ok(), "the child exited: {:?}", run.status);
        assert!(run.io.is_ok(), "and its capture is complete");
        assert!(
            !run.group_stopped,
            "but the descendant is still in the job, so this is no stop proof"
        );
        assert!(
            super::platform::is_tracked(run.stop_key),
            "the handle is still held while that descendant runs"
        );

        super::terminate(run.stop_key);
        let mut swept = false;
        for _ in 0..400 {
            if !super::platform::is_tracked(run.stop_key) {
                swept = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            swept,
            "once the tree is gone the sweeper must drop the handle nobody else owns"
        );
    }

    /// #726 review P1: a job is **not** addressed by the child's pid, which the
    /// OS recycles as soon as the child is reaped — the state an `Unaccounted`
    /// termination is in. A recycled pid would otherwise point a parked
    /// receipt's probe at a later spawn's job.
    ///
    /// Reuse itself cannot be forced in a test, so what is asserted is the
    /// property that makes it harmless: the key is minted here, and is not the
    /// pid.
    #[test]
    fn a_job_is_not_addressed_by_the_recyclable_pid() {
        let mut first = long_running().spawn().expect("spawn");
        let mut second = long_running().spawn().expect("spawn");
        let (first_pid, second_pid) = (first.id(), second.id());
        let one = super::attach(first_pid, &first);
        let two = super::attach(second_pid, &second);
        assert_ne!(
            one, first_pid,
            "the key must not be the pid: a reaped pid comes back, a key does not"
        );
        assert_ne!(two, second_pid, "likewise for the second spawn");
        assert_ne!(one, two, "each spawn is named once and never again");
        super::release(one);
        assert!(
            super::alive(two),
            "retiring one job must not disturb another"
        );
        super::terminate(two);
        super::release(two);
        let _ = first.kill();
        let _ = second.kill();
        let _ = first.wait();
        let _ = second.wait();
    }
}
