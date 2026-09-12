//! Process-group lifetime: stopping one, and asking whether it is gone.
//!
//! Split from `proc.rs` because it is the whole stop proof and its one
//! ordering rule deserves to sit together: a pgid is only a handle while the
//! group's leader is unreaped, so it is signalled *before* the reap and only
//! ever probed afterwards (ADR-0175, #702 review 5).
use super::*;

/// Stop the whole process group. Called only when the deadline expired: the
/// command is over either way, and a descendant of it still running is a write
/// kagi cannot account for.
///
/// **Only ever with the group's leader still unreaped.** The leader is what
/// keeps the pgid allocated; once the group empties, the number goes back to
/// the OS and this would SIGKILL whatever holds it next (#702 review 5). A pgid
/// carried past the reap is a number, not a handle.
#[cfg(unix)]
pub(super) fn kill_group(pgid: u32) {
    // SAFETY: signalling our own child's group; the group id is the child's pid
    // because `run_child` spawned it as the group leader.
    unsafe { libc::kill(-(pgid as i32) as libc::pid_t, libc::SIGKILL) };
}
#[cfg(not(unix))]
pub(super) fn kill_group(_pgid: u32) {}

/// Is anything in `pgid` still alive, after giving a just-killed group a
/// bounded moment to go? `killed` says whether we signalled it, which is the
/// only case worth waiting for.
pub(super) fn group_settled(pgid: u32, killed: bool) -> bool {
    if !killed {
        return group_alive(pgid);
    }
    for _ in 0..100 {
        if !group_alive(pgid) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

/// Is anything in process group `pgid` still alive?
///
/// `kill(-pgid, 0)` is the POSIX existence probe applied to a whole group: it
/// sends no signal. `Ok` or `EPERM` means at least one process in the group is
/// there; `ESRCH` means the group is empty. This is how a writer that could not
/// be reaped is finally proven stopped, long after the run that abandoned it
/// (ADR-0175, #702 review).
///
/// The **group**, not the pid: [`run_child`] spawns each child as its own group
/// leader, so a transport helper or hook that outlived the child it was
/// spawned from is still counted. Reaping the direct child proves nothing about
/// those ([`ProcStop::reaped`] says so), and a writer lease must not be
/// released on a proof that narrow.
// ponytail: pid reuse can make a recycled group id read as alive, which keeps a
// scope closed that could have been released. Conservative in the safe
// direction; a start-time comparison would be the upgrade if it ever bites. A
// just-exited member reads alive until `run_child`'s janitor reaps it, which is
// prompt — the janitor owns every child it could not reap itself, so a zombie
// is never permanent.
#[cfg(unix)]
pub fn group_alive(pgid: u32) -> bool {
    // SAFETY: `kill` with signal 0 performs no action; it only reports whether
    // the target exists and is signallable. A negative pid targets the group.
    let rc = unsafe { libc::kill(-(pgid as i32) as libc::pid_t, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}
/// Without a probe there is no evidence, and "no evidence" is not "gone".
///
/// Reporting alive keeps the scope reserved and the acknowledge refused, which
/// is the safe half of a wrong answer: a held lease costs the user a restart,
/// releasing one on no evidence costs them a concurrent write over a mutation
/// that may still be running (ADR-0175).
// ponytail: Windows has no process group in this sense, so the reconcile exit
// there is the safe dead end — held until the application restarts. The upgrade
// is a job object per child (`CreateJobObject` + `AssignProcessToJobObject` at
// spawn, `QueryInformationJobObject` for live process ids), which is the
// Windows equivalent of the group and the only thing that could answer this.
#[cfg(not(unix))]
pub fn group_alive(_pgid: u32) -> bool {
    true
}
