//! Who owns a spawned process group when the task that started it unwinds.
//!
//! [`run_child`](super::run_child) hands the caller a finished [`ProcRun`] and
//! keeps nothing — which is right for a run that ended, and wrong for a task
//! that **panics** while one is in flight. The child handle lives on that
//! task's stack, so the unwind took it: kagi lost the only thing that could say
//! whether the write is still running, the lease stayed reserved and the
//! reconcile entry had nothing to probe until the application restarted
//! (#703 / ADR-0175).
//!
//! The supervisor is the owner outside the task. A job registers itself before
//! its executor starts ([`begin`]), the executor thread marks itself as that
//! job ([`enter`]), and every group [`run_child`](super::run_child) spawns is
//! recorded against it until that run proves the group empty. The registry is
//! process-global, so an unwind cannot take it: the abandonment reads back the
//! groups the job still owns ([`take_live_groups`]) and the termination becomes
//! a probeable [`Unaccounted`](crate::Termination::Unaccounted) instead of a
//! dead end.
//!
//! What it deliberately does **not** do: kill anything, or claim a stop it did
//! not observe. The only stop proof is still an empty process group
//! ([`group_stopped`]), and on a platform without one the answer stays "alive",
//! which keeps the scope held (see [`group_alive`](super::group_alive)).
use super::group_alive;
use std::cell::Cell;
use std::collections::HashMap;
use std::process::Child;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// One supervised execution. Allocated by [`begin`] before the job is handed to
/// an executor, so the abandonment can name the job even though the task that
/// would have reported it is gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(u64);

/// One spawned process group: the id a later probe asks about, and the direct
/// child until someone takes it back to wait on.
struct Spawned {
    pgid: u32,
    /// `None` once [`ChildHandle::reclaim`] handed it to the run's own janitor.
    /// While it is `Some`, this registry is the child's only owner — which is
    /// the point: an unwind between the spawn and the wait would otherwise drop
    /// the handle and leave a zombie nobody reaps (#725 review).
    child: Arc<Mutex<Option<Child>>>,
}

/// Groups this job spawned that have **not** been proven empty.
type Jobs = HashMap<JobId, Vec<Spawned>>;

fn jobs() -> std::sync::MutexGuard<'static, Jobs> {
    static JOBS: OnceLock<Mutex<Jobs>> = OnceLock::new();
    let lock = JOBS.get_or_init(|| Mutex::new(HashMap::new()));
    lock.lock().unwrap_or_else(|e| e.into_inner())
}

thread_local! {
    /// The job the current thread is executing, if any. Set by [`enter`] on the
    /// executor thread itself, which is where `run_child` runs.
    static CURRENT: Cell<Option<JobId>> = const { Cell::new(None) };
}

/// Register a job about to be executed.
///
/// Called on the thread that *owns* the job, not the one that runs it: the
/// caller keeps the id so it can settle an abandonment. A job that is dropped
/// without ever being entered leaves an empty entry behind; that is one `Vec`
/// per dropped job and no process, which is why nothing tries to be clever
/// about it.
pub fn begin() -> JobId {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let id = JobId(NEXT.fetch_add(1, Ordering::Relaxed));
    jobs().insert(id, Vec::new());
    id
}

/// Mark this thread as executing `id` until the guard drops.
pub fn enter(id: JobId) -> JobGuard {
    let previous = CURRENT.with(|current| current.replace(Some(id)));
    JobGuard { id, previous }
}

/// Scope of [`enter`]. Dropping it on the ordinary path forgets the job: the
/// run that finished reported its own termination, and anything it could not
/// account for travels in that receipt.
///
/// **On an unwind the entry is kept.** That is the whole point: the
/// abandonment that settles the panicked job is what takes it, and until then
/// nothing else may claim those groups are gone.
pub struct JobGuard {
    id: JobId,
    previous: Option<JobId>,
}
impl Drop for JobGuard {
    fn drop(&mut self) {
        CURRENT.with(|current| current.set(self.previous));
        if std::thread::panicking() {
            // The groups stay registered for the abandonment to read, but a
            // child still owned here has lost the janitor that would have
            // waited on it, so this takes that over now (#725 review).
            if let Some(groups) = jobs().get(&self.id) {
                reap_orphans(groups);
            }
            return;
        }
        jobs().remove(&self.id);
    }
}

/// Take ownership of a freshly spawned child and its group.
///
/// Called by [`run_child`](super::run_child) with the `Child` it just got, and
/// **before** anything that can unwind: from here the registry owns the handle,
/// so a panic on the way to the wait cannot drop it. The caller works through
/// the returned [`ChildHandle`] and takes the child back for its own janitor
/// when the run ends.
///
/// A spawn outside a supervised job — a read, a probe, a test — still gets a
/// handle; it just has no job to be registered against.
pub(super) fn register_child(pgid: u32, child: Child) -> ChildHandle {
    let child = Arc::new(Mutex::new(Some(child)));
    let handle = ChildHandle {
        child: Arc::clone(&child),
    };
    let Some(id) = CURRENT.with(|current| current.get()) else {
        return handle;
    };
    if let Some(groups) = jobs().get_mut(&id) {
        groups.push(Spawned { pgid, child });
    }
    handle
}

/// The caller's access to a child the supervisor owns.
pub(super) struct ChildHandle {
    child: Arc<Mutex<Option<Child>>>,
}
impl ChildHandle {
    /// Borrow the child for as long as `f` runs — `try_wait`, the kill, the
    /// reap. `None` once it has been reclaimed (or reaped by an abandonment).
    pub(super) fn with<R>(&self, f: impl FnOnce(&mut Child) -> R) -> Option<R> {
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        child.as_mut().map(f)
    }
    /// Take the child back, to be owned by the run's own janitor from here on.
    pub(super) fn reclaim(&self) -> Option<Child> {
        self.child.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

/// That group was proven empty by the run that owned it, so the job no longer
/// has anything to answer for it.
pub(super) fn release_group(pgid: u32) {
    let Some(id) = CURRENT.with(|current| current.get()) else {
        return;
    };
    if let Some(groups) = jobs().get_mut(&id) {
        groups.retain(|spawned| spawned.pgid != pgid);
    }
}

/// Reap whatever children an unwound job left owned here, on their own thread.
///
/// The group may well still be running — that is what the abandonment is
/// about — so this blocks until the direct child exits, exactly as the run's
/// janitor would have. Without it the child is never waited on and stays a
/// zombie for the life of the process (#725 review). The group id stays
/// registered either way: reaping the leader is not a stop proof.
fn reap_orphans(groups: &[Spawned]) {
    for spawned in groups {
        let Some(mut child) = spawned
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        else {
            continue;
        };
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// The groups `id` spawned that are **still alive**, for an abandonment to
/// build its termination from. An empty answer means no process of this job is
/// running: the executor thread itself was the writer, and it unwound.
///
/// Live groups stay registered under `id`, because [`group_stopped`] resolves
/// the later probe against the whole job rather than the one group the
/// requirement happened to name.
pub fn take_live_groups(id: JobId) -> Vec<u32> {
    let mut jobs = jobs();
    let Some(groups) = jobs.remove(&id) else {
        return Vec::new();
    };
    // Anything this job still owns is reaped by its own thread; what the
    // abandonment needs is the group ids, not the handles.
    reap_orphans(&groups);
    let live: Vec<Spawned> = groups
        .into_iter()
        .filter(|spawned| group_alive(spawned.pgid))
        .collect();
    let pgids: Vec<u32> = live.iter().map(|spawned| spawned.pgid).collect();
    if !live.is_empty() {
        jobs.insert(id, live);
    }
    pgids
}

/// Is the writer behind `pgid` proven stopped? The one stop proof a reconcile
/// read is allowed to use (ADR-0175).
///
/// For a supervised abandonment the question is asked of the **job**: a
/// requirement names one group, but the job may still own others, and releasing
/// the scope while any of them is alive would be a release on partial evidence.
/// Once they are all gone the job is forgotten. A `pgid` the supervisor never
/// saw — the ordinary `Unaccounted` from a killed child — falls back to probing
/// that group alone, which is what it meant before this existed.
pub fn group_stopped(pgid: u32) -> bool {
    let mut jobs = jobs();
    let job = jobs
        .iter()
        .find(|(_, groups)| groups.iter().any(|spawned| spawned.pgid == pgid))
        .map(|(id, groups)| {
            (
                *id,
                groups
                    .iter()
                    .map(|spawned| spawned.pgid)
                    .collect::<Vec<_>>(),
            )
        });
    let Some((id, groups)) = job else {
        // Not a supervised abandonment: an ordinary `Unaccounted` whose job
        // entry went with its `JobGuard`. The probe itself records an empty job
        // (see `super::job`), so a reconcile read that proves the stop and then
        // fails on the repository can prove it again on the retry.
        return !group_alive(pgid);
    };
    if groups.iter().any(|group| group_alive(*group)) {
        return false;
    }
    // The supervisor is done with this job, but the **proof** is not thrown
    // away with it: `group_alive` remembers an empty job, so a retried
    // reconcile read gets the same answer (#726 review P2).
    jobs.remove(&id);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A child of our own, spawned as its own group leader exactly as
    /// `run_child` does — the shape the supervisor is given.
    #[cfg(unix)]
    fn spawn_group(args: &[&str]) -> Child {
        use std::os::unix::process::CommandExt;
        let mut command = Command::new(args[0]);
        command
            .args(&args[1..])
            .process_group(0)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        command.spawn().expect("spawn")
    }

    /// Is `pid` still in the process table — including as an unreaped zombie?
    /// `ps -o stat=` prints `Z`/`Z+` for one, and nothing at all once it has
    /// been waited on.
    #[cfg(unix)]
    fn process_state(pid: u32) -> String {
        let out = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .expect("ps");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// The ordinary path: a job that finishes leaves nothing behind, so a later
    /// probe about its group is the plain group question.
    #[test]
    #[cfg(unix)]
    fn a_finished_job_is_forgotten() {
        let child = spawn_group(&["true"]);
        let group = child.id();
        let id = begin();
        {
            let _guard = enter(id);
            let handle = register_child(group, child);
            let mut child = handle.reclaim().expect("the run takes its child back");
            let _ = child.wait();
        }
        assert!(
            take_live_groups(id).is_empty(),
            "a job that returned normally must not hold groups for an abandonment"
        );
    }

    /// An unwind keeps the entry — that is what the abandonment reads — and a
    /// group that is still running is not a stop proof.
    #[test]
    #[cfg(unix)]
    fn an_unwound_job_keeps_its_live_groups() {
        let child = spawn_group(&["sleep", "300"]);
        let group = child.id();

        let id = begin();
        let hushed = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let panicked = std::panic::catch_unwind(|| {
            let _guard = enter(id);
            let _handle = register_child(group, child);
            panic!("the task unwound");
        })
        .is_err();
        std::panic::set_hook(hushed);
        assert!(panicked, "the test must actually unwind");
        assert_eq!(
            take_live_groups(id),
            vec![group],
            "the panicked job's live group must survive its task"
        );
        assert!(!group_stopped(group), "a live group is not a stop proof");

        // The abandonment's reaper owns the child now, so stopping the group is
        // the test's only job; the wait is not the test's to make.
        // SAFETY: signalling a group this test created and still owns.
        unsafe { libc::kill(-(group as i32) as libc::pid_t, libc::SIGKILL) };
        for _ in 0..200 {
            if !group_alive(group) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            group_stopped(group),
            "an empty group is the stop proof, and it retires the job"
        );
    }

    /// #725 review: a task that unwinds **between** the spawn and the wait left
    /// the child unreaped — the handle was on the stack the panic destroyed.
    /// The supervisor owns it from the spawn, so the unwind hands it to a
    /// reaper instead of leaving a zombie for the life of the process.
    #[test]
    #[cfg(unix)]
    fn an_unwound_job_reaps_the_child_it_owned() {
        // Exits immediately: without a wait it becomes a zombie and stays one.
        let child = spawn_group(&["true"]);
        let pid = child.id();

        let id = begin();
        let hushed = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let panicked = std::panic::catch_unwind(|| {
            let _guard = enter(id);
            // Ownership has moved; the handle this frame holds is only access.
            let _handle = register_child(pid, child);
            panic!("the task unwound before the wait");
        })
        .is_err();
        std::panic::set_hook(hushed);
        assert!(panicked, "the test must actually unwind");

        let mut state = process_state(pid);
        for _ in 0..200 {
            if state.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
            state = process_state(pid);
        }
        assert!(
            state.is_empty(),
            "the abandoned child was never waited on: ps still reports it as {state:?}"
        );
        let _ = take_live_groups(id);
    }

    /// A group the run proved empty is not something the job still answers for.
    #[test]
    #[cfg(unix)]
    fn a_released_group_is_not_an_abandonment() {
        let child = spawn_group(&["true"]);
        let group = child.id();
        let id = begin();
        let guard = enter(id);
        let handle = register_child(group, child);
        let mut child = handle.reclaim().expect("the run takes its child back");
        let _ = child.wait();
        release_group(group);
        std::mem::forget(guard); // leave the entry as an unwind would
        assert!(take_live_groups(id).is_empty());
        CURRENT.with(|current| current.set(None));
    }
}
