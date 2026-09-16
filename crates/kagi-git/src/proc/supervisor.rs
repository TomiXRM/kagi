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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// One supervised execution. Allocated by [`begin`] before the job is handed to
/// an executor, so the abandonment can name the job even though the task that
/// would have reported it is gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(u64);

/// Groups this job spawned that have **not** been proven empty.
type Jobs = HashMap<JobId, Vec<u32>>;

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
            return;
        }
        jobs().remove(&self.id);
    }
}

/// A group was spawned by whatever job this thread is executing.
pub(super) fn register_group(pgid: u32) {
    let Some(id) = CURRENT.with(|current| current.get()) else {
        return; // not part of a supervised job: a read, a probe, a test
    };
    if let Some(groups) = jobs().get_mut(&id) {
        groups.push(pgid);
    }
}

/// That group was proven empty by the run that owned it, so the job no longer
/// has anything to answer for it.
pub(super) fn release_group(pgid: u32) {
    let Some(id) = CURRENT.with(|current| current.get()) else {
        return;
    };
    if let Some(groups) = jobs().get_mut(&id) {
        groups.retain(|group| *group != pgid);
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
    let live: Vec<u32> = groups.into_iter().filter(|g| group_alive(*g)).collect();
    if !live.is_empty() {
        jobs.insert(id, live.clone());
    }
    live
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
        .find(|(_, groups)| groups.contains(&pgid))
        .map(|(id, groups)| (*id, groups.clone()));
    let Some((id, groups)) = job else {
        return !group_alive(pgid);
    };
    if groups.iter().any(|group| group_alive(*group)) {
        return false;
    }
    jobs.remove(&id);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ordinary path: a job that finishes leaves nothing behind, so a later
    /// probe about its group is the plain group question.
    #[test]
    fn a_finished_job_is_forgotten() {
        let id = begin();
        {
            let _guard = enter(id);
            register_group(4242);
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
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("sleep");
        command.arg("300").process_group(0);
        let mut child = command.spawn().expect("spawn sleep");
        let group = child.id();

        let id = begin();
        let hushed = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let panicked = std::panic::catch_unwind(|| {
            let _guard = enter(id);
            register_group(group);
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

        child.kill().expect("stop the group the test started");
        child.wait().expect("reap it");
        assert!(
            group_stopped(group),
            "an empty group is the stop proof, and it retires the job"
        );
    }

    /// A group the run proved empty is not something the job still answers for.
    #[test]
    fn a_released_group_is_not_an_abandonment() {
        let id = begin();
        let guard = enter(id);
        register_group(4242);
        release_group(4242);
        std::mem::forget(guard); // leave the entry as an unwind would
        assert!(take_live_groups(id).is_empty());
        CURRENT.with(|current| current.set(None));
    }
}
