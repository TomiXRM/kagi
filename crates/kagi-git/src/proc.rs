//! The subprocess runner: one owner for a child, its pipes and its deadline.
//!
//! Every `Command` kagi spawns goes through [`run_child`] — `git` (`cli.rs`),
//! `ssh` (`src/remote/`), worktree `command` steps, the message-gen CLIs. It
//! exists because the same three mistakes were re-made in each of them
//! (issues #294 / #403 / #507, ADR-0188):
//!
//! 1. Reading a pipe only *after* the child exits deadlocks as soon as the
//!    child writes more than one pipe buffer.
//! 2. Timing out a wait without owning the child leaks it — and reports the
//!    cut-short wait as though the process had ended.
//! 3. Waiting on I/O without a bound loses the deadline again, because killing
//!    a child does not close the pipes a descendant of it inherited.
//!
//! The shapes here answer those: [`ProcStop`] carries no exit code, [`ProcIo`]
//! says whether the capture is complete, and neither can pass for success.

mod cwd;
mod group;
mod job;
mod session;
pub mod supervisor;
pub use cwd::{cwd_of_pid, CwdProbe};
pub use group::group_alive;
pub use session::session_members;

use group::{group_settled, kill_group};
use std::process::Stdio;
use std::time::{Duration, Instant};

// ──────────────────────────────────────────────────────────────────────────
// The subprocess runner (issue #507)
// ──────────────────────────────────────────────────────────────────────────

/// Why a run produced **no exit status**.
///
/// Both variants mean *unknown*, never "the process ended". A deadline that
/// expires cuts the **wait** short; whatever the child had already set in
/// motion — a `git push` on the wire, a `git pull` running on an SSH host —
/// is not proven stopped by it. Callers map this to
/// [`GitError::TerminationUnknown`] and an `Unknown` oplog outcome, never to a
/// plain failure (ADR-0177, #582).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcStop {
    /// The deadline expired before the child exited.
    Deadline { secs: u64, reaped: bool },
    /// `try_wait` itself failed, so the exit status is unknowable.
    Wait { error: String, reaped: bool },
}

impl ProcStop {
    /// True when the **local** child was confirmed killed and collected.
    ///
    /// It says nothing about the child's own descendants, nor about work the
    /// child had already started on another machine — those stay unknown.
    // ponytail: kills the direct child only, not its process group, so a child
    // whose grandchildren outlive it (ssh → remote, node) can still leak one.
    // Killing the group is a platform-specific follow-up (#507 "process tree").
    pub fn reaped(&self) -> bool {
        match self {
            ProcStop::Deadline { reaped, .. } | ProcStop::Wait { reaped, .. } => *reaped,
        }
    }
}

impl std::fmt::Display for ProcStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcStop::Deadline { secs, .. } => write!(f, "timed out after {secs}s")?,
            ProcStop::Wait { error, .. } => write!(f, "wait failed: {error}")?,
        }
        if !self.reaped() {
            write!(f, " (the local child could not be stopped)")?;
        }
        Ok(())
    }
}

impl std::error::Error for ProcStop {}

/// Why the captured output is **not proven complete**.
///
/// Separate evidence from [`ProcStop`], because process exit and I/O completion
/// are separate facts: a child can exit 0 having left a descendant holding the
/// pipes, and a truncated read must not pass for a successful one (#507 review).
/// The partial output is still returned — the consumer classifies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcIo {
    /// The input could not be fully written (the child stopped reading it).
    Stdin(String),
    /// Reading a pipe failed. What was captured before the error is kept.
    Read(String),
    /// The streams had not ended within [`IO_GRACE`] of the wait resolving: a
    /// descendant of the child still holds them open. The capture is a prefix.
    Unfinished,
    /// A collector thread panicked; nothing can be said about its stream.
    Panicked,
}

impl std::fmt::Display for ProcIo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcIo::Stdin(e) => write!(f, "input was not fully written: {e}"),
            ProcIo::Read(e) => write!(f, "output could not be read: {e}"),
            ProcIo::Unfinished => write!(
                f,
                "output collection did not finish — a descendant still holds the \
                 pipes, so what was captured may be truncated"
            ),
            ProcIo::Panicked => write!(f, "an output collector thread panicked"),
        }
    }
}

impl std::error::Error for ProcIo {}

/// The result of one subprocess run: what was captured, whether the process
/// really exited, and whether the capture is complete.
///
/// The split is the point of [`run_child`]: a caller cannot accidentally read a
/// cut-short wait as an exit code, because there is no code to read — and
/// cannot read a truncated capture as a whole one, because `io` says so.
#[derive(Debug, Clone)]
pub struct ProcRun {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// `Ok(code)` — the process really exited. `Err(_)` — see [`ProcStop`].
    pub status: Result<i32, ProcStop>,
    /// `Ok(())` — `stdout`/`stderr` are everything the child wrote, and all the
    /// input reached it. `Err(_)` — see [`ProcIo`]; the buffers are a prefix.
    pub io: Result<(), ProcIo>,
    /// The child's process id. On unix it is also the id of the group holding
    /// everything the child started, because the child is spawned as its own
    /// group leader — the handle a later read proves stop against when the kill
    /// could not (ADR-0175). Elsewhere it is just the pid, and
    /// [`group_alive`] has nothing to ask.
    pub pid: u32,
    /// What a later read probes to prove this run stopped: the process group id
    /// on unix (where the number *is* the handle), and the supervisor's job key
    /// on Windows (#703b). Never a recycled pid — see [`job`].
    pub stop_key: u32,
    /// Is the child's whole process **group** confirmed empty?
    ///
    /// The only thing that may become `Termination::Stopped`. Reaping the
    /// direct child is not it: a transport helper or hook it started can still
    /// be writing, and `ProcIo::Unfinished` is that case caught in the act —
    /// something in the group is still holding the pipes (#702 re-review).
    /// Always `false` where there is no probe: no evidence is not "gone".
    pub group_stopped: bool,
}

impl ProcRun {
    /// Captured stdout, UTF-8 lossy.
    pub fn stdout_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
    /// Captured stderr, UTF-8 lossy.
    pub fn stderr_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// How long output collection may continue **after** the wait resolves.
///
/// Once the direct child is gone its own pipe buffers drain in microseconds, so
/// this is generous for the normal case. Anything still holding the streams
/// open past it is a descendant we do not own and may never exit — bounding the
/// reads here is what makes the deadline reach the caller (#507 review P1).
/// Worst case wall time for a run is therefore `timeout + IO_GRACE`.
const IO_GRACE: Duration = Duration::from_secs(2);

/// One collector thread's report: what it captured, and what went wrong.
enum Collected {
    Stdout(Vec<u8>, Option<std::io::Error>),
    Stderr(Vec<u8>, Option<std::io::Error>),
    Stdin(Option<std::io::Error>),
}

/// Run `cmd` with a deadline, owning the child and all three of its pipes —
/// the single subprocess runner for the whole codebase (issue #507).
///
/// - **Pipes**: stdout and stderr are piped and drained on their own threads,
///   so a chatty child can never deadlock in `write(2)` against a full pipe
///   buffer while we wait for it (issues #294/#403). `stdin` is piped and fed
///   from a third thread when `stdin` is `Some` — a prompt bigger than the pipe
///   buffer would otherwise deadlock the same way — and `null` otherwise, so no
///   child ever blocks reading a terminal we are not watching.
/// - **Deadline**: on expiry the child is killed and reaped here, and the run
///   comes back with [`ProcStop`] instead of an exit code. Collection then gets
///   [`IO_GRACE`] and no more — killing the child does *not* close pipes a
///   descendant inherited, so the reads are bounded rather than waited on
///   (#507 review P1). Whatever is not finished by then is reported as
///   [`ProcIo`], with the partial capture.
/// - **Ownership**: a child that could not be reaped, and reads still in
///   flight, are handed to a janitor thread that owns them to completion. The
///   caller is freed without abandoning either (#507 review P2).
///
/// The caller supplies the program, args, env and cwd; the runner sets the
/// three `Stdio`s itself (they are what it owns).
///
/// # Errors
///
/// `Err(io::Error)` **only** when the child never started — nothing ran, so
/// nothing can have changed. Every post-spawn problem is evidence on the
/// `ProcRun`, never "nothing ran".
pub fn run_child(
    cmd: &mut std::process::Command,
    timeout: Duration,
    stdin: Option<&[u8]>,
) -> std::io::Result<ProcRun> {
    use std::io::Write;

    cmd.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    // Its own process group, so the stop proof can be about everything the
    // command started — a transport helper, a hook, an ssh — and not just the
    // one process we hold a handle to (#702 re-review).
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    // Windows has no group to be spawned into, so the child is created
    // **suspended** and resumed below, once its job owns it: a helper started
    // before that assignment would be outside the job and uncounted, and the
    // tracked child exiting would then read as a stop proof (#726 review P1).
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(job::SPAWN_FLAGS);
    }
    let mut child = cmd.spawn()?;
    // The child is its own group leader, so the group id is its pid.
    let pid = child.id();
    // The pipes come off the handle first — they are what this function owns —
    // and then the child itself moves to the supervisor, before anything that
    // can unwind. From here the registry is its owner: a panic on the way to
    // the wait can no longer drop the handle and leave a zombie, and the group
    // is still named for a later probe (#703 / #725 review).
    let (child_stdin, child_stdout, child_stderr) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take());
    // Bind first, then let it run: on unix the group already exists, on Windows
    // this is the job assignment the suspension was for.
    let stop_key = job::attach(pid, &child);
    job::resume(pid);
    let child = supervisor::register_child(stop_key, child);

    // Each collector reports through the channel when it is done, so the wait
    // for them can be bounded (a `JoinHandle` cannot). The handles are kept only
    // for ownership: the janitor joins them if any read is still in flight.
    let (tx, rx) = std::sync::mpsc::channel();
    let mut threads = Vec::new();
    if let (Some(data), Some(mut pipe)) = (stdin, child_stdin) {
        let (data, tx) = (data.to_vec(), tx.clone());
        threads.push(std::thread::spawn(move || {
            let error = pipe.write_all(&data).err();
            drop(pipe); // → the child sees EOF on stdin.
            let _ = tx.send(Collected::Stdin(error));
        }));
    }
    if let Some(pipe) = child_stdout {
        threads.push(drain(pipe, tx.clone(), Collected::Stdout));
    }
    if let Some(pipe) = child_stderr {
        threads.push(drain(pipe, tx.clone(), Collected::Stderr));
    }
    // Our own sender must go, or `Disconnected` (every collector gone) never
    // arrives.
    drop(tx);

    // A deadline that expires stops the whole group, not just the process we
    // hold: whatever the command started is part of the same write, and leaving
    // it running is what makes the termination unprovable. `wait_or_kill` does
    // it, because only it still holds the child unreaped at that moment — see
    // the ordering note there.
    let status = match child.with(|child| wait_or_kill(child, timeout, stop_key)) {
        Some(status) => status,
        // Unreachable in practice: nothing else takes the child while this call
        // owns the run. Treated as a stop that cannot be read rather than
        // panicking on it.
        None => Err(ProcStop::Wait {
            error: "the child handle was taken from under the run".to_string(),
            reaped: false,
        }),
    };
    let (stdout, stderr, io) = collect(&rx, threads.len());
    // Asked *after* the collectors settle, so a descendant still holding the
    // pipes is still counted. This is the whole stop proof.
    let group_stopped = !group_settled(stop_key, status.is_err());
    if group_stopped {
        // Proven empty: the job has nothing left to answer for here, and on
        // Windows the job object that proved it can go with it.
        supervisor::release_group(stop_key);
        job::release(stop_key);
    } else {
        // The run ended with something of its own still inside the job: a hook's
        // daemon, a helper on the pipes. Whether a receipt will name this key
        // depends on what the caller makes of `status` and `io`, and this is the
        // wrong place to guess — so every such key goes to the sweeper, which
        // closes the handle when the tree goes and leaves the proof behind for
        // whoever may still probe it (#726 review P2).
        job::watch_until_empty(stop_key);
    }

    // Hand off whenever something is still ours to own: an unreaped child, or a
    // read that has not ended. On the ordinary path the collectors are done, so
    // this join is immediate.
    if io.is_err() || status.as_ref().err().is_some_and(|s| !s.reaped()) {
        let child = child.reclaim();
        std::thread::spawn(move || {
            // Blocks for as long as the descendant lives — off the caller's
            // path, but with an owner: the child is reaped when it finally
            // exits, and the reader threads/fds are released with it.
            if let Some(mut child) = child {
                let _ = child.wait();
            }
            for t in threads {
                let _ = t.join();
            }
        });
    } else {
        for t in threads {
            let _ = t.join();
        }
    }

    Ok(ProcRun {
        stdout,
        stderr,
        status: status.map(|s| s.code().unwrap_or(-1)),
        io,
        pid,
        stop_key,
        group_stopped,
    })
}

/// Read a child pipe to EOF on its own thread, reporting through `tx`.
fn drain<R: std::io::Read + Send + 'static>(
    mut pipe: R,
    tx: std::sync::mpsc::Sender<Collected>,
    tag: fn(Vec<u8>, Option<std::io::Error>) -> Collected,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        // `read_to_end` keeps what it read before an error — send both.
        let error = pipe.read_to_end(&mut buf).err();
        let _ = tx.send(tag(buf, error));
    })
}

/// Wait up to [`IO_GRACE`] for the `pending` collectors to finish, and return
/// what they captured plus whether the capture is complete.
fn collect(
    rx: &std::sync::mpsc::Receiver<Collected>,
    pending: usize,
) -> (Vec<u8>, Vec<u8>, Result<(), ProcIo>) {
    use std::sync::mpsc::RecvTimeoutError;

    let (mut stdout, mut stderr, mut io) = (Vec::new(), Vec::new(), Ok(()));
    // First problem wins: it is the one closest to the cause.
    let mut fail = |e: ProcIo| {
        if io.is_ok() {
            io = Err(e);
        }
    };
    let end = Instant::now() + IO_GRACE;
    for _ in 0..pending {
        match rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
            Ok(Collected::Stdout(buf, error)) => {
                stdout = buf;
                if let Some(e) = error {
                    fail(ProcIo::Read(e.to_string()));
                }
            }
            Ok(Collected::Stderr(buf, error)) => {
                stderr = buf;
                if let Some(e) = error {
                    fail(ProcIo::Read(e.to_string()));
                }
            }
            Ok(Collected::Stdin(error)) => {
                if let Some(e) = error {
                    fail(ProcIo::Stdin(e.to_string()));
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                fail(ProcIo::Unfinished);
                break;
            }
            // Every sender is gone but not every collector reported: one died.
            Err(RecvTimeoutError::Disconnected) => {
                fail(ProcIo::Panicked);
                break;
            }
        }
    }
    (stdout, stderr, io)
}

/// Wait up to `timeout` for `child` to exit, polling `try_wait`.
///
/// `Ok(status)` means it really exited. On timeout (or a `try_wait` error) the
/// child's **process group** is killed and the child is reaped, and the
/// [`ProcStop`] says so, so a hung process never leaks (issue #294) and no
/// caller can mistake the cut-short wait for an exit (issue #507). The reap is
/// bounded — after a kill the child exits promptly — so this never blocks
/// indefinitely; if the bound is hit, `reaped` is false.
///
/// **The group is signalled first, while the leader is still unreaped.** An
/// unreaped leader pins the pgid; the moment the group empties, that number is
/// free for the OS to hand to someone else, and a `kill(-pgid, …)` sent
/// afterwards would land on a stranger's process group (#702 review 5). A pgid
/// held only as a number, after the reap, is not a handle to anything.
pub(crate) fn wait_or_kill(
    child: &mut std::process::Child,
    timeout: Duration,
    stop_key: u32,
) -> Result<std::process::ExitStatus, ProcStop> {
    let deadline = Instant::now() + timeout;
    let secs = timeout.as_secs();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            // Timed out, or try_wait failed: kill the group, then reap.
            other => {
                let error = other.err().map(|e| e.to_string());
                // Group first — the still-unreaped leader is what makes this
                // pgid ours to signal. `run_child` spawns every child as its
                // own group leader, so the child's pid *is* the group id.
                kill_group(stop_key);
                let _ = child.kill();
                // Bounded reap: the child exits promptly once killed.
                let mut reaped = false;
                for _ in 0..200 {
                    match child.try_wait() {
                        Ok(Some(_)) => {
                            reaped = true;
                            break;
                        }
                        // `try_wait` is broken; we cannot confirm anything.
                        Err(_) => break,
                        Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                    }
                }
                return Err(match error {
                    Some(error) => ProcStop::Wait { error, reaped },
                    None => ProcStop::Deadline { secs, reaped },
                });
            }
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "../../../tests/support/proc_identity.rs"]
mod test_proc_identity;

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    /// A direct child handle pins its PID until it is reaped. Match the
    /// existing test `LiveGroup` pattern: even a panicking test collects it.
    struct OwnedChild {
        child: std::process::Child,
        group_owned: bool,
    }

    impl OwnedChild {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
            let result = self.child.wait();
            if result.is_ok() {
                self.group_owned = false;
            }
            result
        }

        fn wait_or_kill(
            &mut self,
            timeout: Duration,
        ) -> Result<std::process::ExitStatus, ProcStop> {
            let pid = self.child.id();
            let result = super::wait_or_kill(&mut self.child, timeout, pid);
            if result.as_ref().map_or_else(|stop| stop.reaped(), |_| true) {
                self.group_owned = false;
            }
            result
        }
    }

    impl Drop for OwnedChild {
        fn drop(&mut self) {
            if self.group_owned {
                // Still unreaped: the Child pins the group even on a panic
                // before the fixture could publish its descendant identity.
                #[cfg(unix)]
                kill_group(self.child.id());
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[cfg(target_os = "macos")]
    fn fixture_supervisor(_test: &str) -> bool {
        true
    }

    /// Linux orphan reaping belongs to a single re-executed fixture test,
    /// never the shared harness. Its reaper stays outside the stopped group.
    #[cfg(target_os = "linux")]
    fn fixture_supervisor(test: &str) -> bool {
        if std::env::var("KAGI_PROC_FIXTURE_SUPERVISOR").as_deref() == Ok(test) {
            // SAFETY: this dedicated subprocess runs exactly one fixture test.
            assert_eq!(
                unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
                0,
                "enable fixture-owned orphan adoption: {}",
                std::io::Error::last_os_error()
            );
            return true;
        }

        use std::os::unix::process::CommandExt;
        let mut command = Command::new(std::env::current_exe().expect("test executable"));
        command
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env("KAGI_PROC_FIXTURE_SUPERVISOR", test)
            .process_group(0);
        let mut supervisor = OwnedChild {
            child: command.spawn().expect("spawn private fixture supervisor"),
            group_owned: true,
        };
        assert!(
            supervisor
                .wait()
                .expect("reap private fixture supervisor")
                .success(),
            "private fixture test failed: {test}"
        );
        false
    }

    /// True if `pid` is still a live (un-reaped) process.
    #[cfg(unix)]
    fn pid_alive(pid: u32) -> bool {
        // SAFETY: signal 0 only probes this exact PID; it never sends a signal.
        let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
        rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    #[cfg(not(unix))]
    fn pid_alive(pid: u32) -> bool {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// The group is stopped **before** the leader is reaped, and the ordering is
    /// the safety property, not a detail (#702 review 5).
    ///
    /// An unreaped leader pins the pgid. Reap it first and, if nothing else is
    /// left in the group, that number goes back to the OS — a `kill(-pgid, …)`
    /// sent afterwards can land on a stranger's process group. So by the time
    /// `wait_or_kill` says "reaped", the group it was given must already be
    /// gone: here the leader's own child outlives a plain `child.kill()`, and
    /// only a group signal sent while the leader still held the pgid can have
    /// removed it.
    #[cfg(unix)]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn wait_or_kill_stops_the_group_before_it_reaps_the_leader() {
        if !fixture_supervisor(
            "proc::tests::wait_or_kill_stops_the_group_before_it_reaps_the_leader",
        ) {
            return;
        }
        use std::os::unix::process::CommandExt;
        let fixture = DescendantFixture::new();
        let mut cmd = fixture.command(false);
        cmd.process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = OwnedChild {
            child: cmd.spawn().expect("spawn fixture"),
            group_owned: true,
        };
        let pgid = child.child.id();
        let descendant = fixture.wait_for_descendant();

        // On unix the stop key is the group id, which is the leader's pid.
        let stop = child
            .wait_or_kill(Duration::from_millis(200))
            .expect_err("the fixture waits, so the deadline must expire");
        assert!(stop.reaped(), "the leader is reaped: {stop:?}");

        // Probe only this fixture. Its RAII guard owns cleanup even if this
        // assertion fails; never signal the numeric group after its reap.
        assert!(
            descendant
                .wait_until_gone()
                .expect("probe fixture identity"),
            "the descendant outlived the reap: the group was signalled too late, or not at all"
        );
        assert!(!group_alive(pgid), "the fixture's group must be gone");
    }

    /// Every child leads its own process group, so the stop proof can be about
    /// everything the command started rather than the one process we hold a
    /// handle to (#702 re-review). Without this, `group_alive` and a plain pid
    /// probe are the same check and a surviving transport helper reads as gone.
    #[cfg(unix)]
    #[test]
    fn a_child_leads_its_own_process_group() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("ps -o pgid= -p $$");
        let run = run_child(&mut cmd, Duration::from_secs(30), None).expect("spawn sh");
        assert_eq!(run.status, Ok(0), "stderr: {}", run.stderr_lossy());
        let pgid: u32 = run
            .stdout_lossy()
            .trim()
            .parse()
            .unwrap_or_else(|e| panic!("pgid from {:?}: {e}", run.stdout_lossy()));
        assert_eq!(
            pgid, run.pid,
            "the child must be its own group leader, so its pid is the group id"
        );
    }

    #[test]
    fn wait_or_kill_kills_and_reaps_on_timeout() {
        // A child that would otherwise run for 5 minutes: the margins below are
        // loose on purpose (a loaded machine must not fail this), and still
        // nowhere near "waited the child out".
        let mut child = OwnedChild {
            child: Command::new("sleep")
                .arg("300")
                .spawn()
                .expect("spawn sleep"),
            group_owned: false,
        };
        let pid = child.child.id();
        assert!(pid_alive(pid), "sleep should be running before the timeout");

        let start = Instant::now();
        let result = child.wait_or_kill(Duration::from_millis(200));

        assert!(
            matches!(result, Err(ProcStop::Deadline { reaped: true, .. })),
            "a timed-out child reports a killed-and-reaped deadline, not an exit: {result:?}"
        );
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "wait_or_kill must return on its deadline, not wait out the full sleep"
        );
        // The kill()+reap() must have taken effect: the pid is gone.
        assert!(
            !pid_alive(pid),
            "child process leaked: kill()/reap() did not run (issue #294)"
        );
    }

    #[test]
    fn wait_or_kill_returns_status_for_fast_child() {
        let mut child = OwnedChild {
            child: Command::new("true").spawn().expect("spawn true"),
            group_owned: false,
        };
        let status = child.wait_or_kill(Duration::from_secs(5));
        assert_eq!(status.ok().and_then(|s| s.code()), Some(0));
    }

    // ── issue #507: one runner owning the child, its pipes and its deadline ──

    #[cfg(target_os = "macos")]
    type FixtureStarted = test_proc_identity::Started;
    #[cfg(target_os = "linux")]
    type FixtureStarted = u64;

    /// A PID is never a cleanup handle after the direct parent reaps it.
    /// Record the kernel start identity while our fixture still owns its child.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[derive(Clone, Copy)]
    struct FixtureProcess {
        pid: u32,
        started: FixtureStarted,
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    impl FixtureProcess {
        fn record(pid: u32) -> Result<Self, String> {
            Ok(Self {
                pid,
                started: fixture_started(pid)?.ok_or("fixture exited before recording")?,
            })
        }

        fn alive(self) -> Result<bool, String> {
            #[cfg(target_os = "macos")]
            {
                match test_proc_identity::identity(self.pid, self.started) {
                    test_proc_identity::Identity::Same => Ok(true),
                    test_proc_identity::Identity::Gone => Ok(false),
                    test_proc_identity::Identity::Unknown(error) => Err(error),
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                Ok(fixture_started(self.pid)? == Some(self.started))
            }
        }

        fn wait_until_gone(self) -> Result<bool, String> {
            for _ in 0..100 {
                if !self.alive()? {
                    return Ok(true);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(false)
        }

        fn stop(
            self,
            #[cfg(target_os = "linux")] ownership: &std::sync::Mutex<()>,
        ) -> Result<(), String> {
            {
                // Serialize with our exact-PID reaper: it must not release the
                // old PID between this identity check and the signal.
                #[cfg(target_os = "linux")]
                let _ownership = ownership
                    .lock()
                    .map_err(|_| "fixture signal/reap ownership poisoned".to_string())?;
                if self.alive()? {
                    #[cfg(target_os = "macos")]
                    test_proc_identity::sigkill(self.pid as i32)?;
                    #[cfg(target_os = "linux")]
                    {
                        // SAFETY: the private fixture published this identity,
                        // rechecked while its reaper cannot release the PID.
                        if unsafe { libc::kill(self.pid as libc::pid_t, libc::SIGKILL) } != 0 {
                            let error = std::io::Error::last_os_error();
                            if error.raw_os_error() != Some(libc::ESRCH) {
                                return Err(error.to_string());
                            }
                        }
                    }
                }
            }
            if self.wait_until_gone()? {
                Ok(())
            } else {
                Err(format!("fixture process {} did not stop", self.pid))
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn fixture_started(pid: u32) -> Result<Option<FixtureStarted>, String> {
        test_proc_identity::started(pid)
    }

    #[cfg(target_os = "linux")]
    fn fixture_started(pid: u32) -> Result<Option<FixtureStarted>, String> {
        let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        // The parenthesized command (field 2) may itself contain spaces or ')'.
        // Fields after its last ')' begin with state (3); starttime is field 22.
        let end = stat.rfind(')').ok_or("malformed fixture /proc stat")?;
        let mut fields = stat[end + 1..].split_whitespace();
        // Keep zombies in the identity probe: fixture completion on Linux
        // requires the private supervisor to reap them, not just stop them.
        fields.next().ok_or("fixture /proc stat omitted state")?;
        fields
            .nth(18)
            .ok_or_else(|| "fixture /proc stat omitted starttime".to_string())?
            .parse()
            .map(Some)
            .map_err(|error| format!("fixture starttime: {error}"))
    }

    /// The runner's deliberately abandoned descendant is the fixture's to
    /// clean up, not a globally named process and not a reap-released group.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    struct DescendantFixture {
        directory: tempfile::TempDir,
        cleaned: bool,
        #[cfg(target_os = "linux")]
        reaper: FixtureReaper,
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    impl DescendantFixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().expect("private descendant fixture");
            #[cfg(target_os = "linux")]
            let reaper = FixtureReaper::start(directory.path());
            Self {
                directory,
                cleaned: false,
                #[cfg(target_os = "linux")]
                reaper,
            }
        }

        fn command(&self, exit: bool) -> Command {
            let mut cmd = Command::new(std::env::current_exe().expect("test executable"));
            cmd.args([
                "--exact",
                "proc::tests::owned_descendant_fixture_process",
                "--ignored",
                "--nocapture",
            ])
            .env("KAGI_PROC_FIXTURE_DIR", self.directory.path())
            .env("KAGI_PROC_FIXTURE_EXIT", if exit { "1" } else { "0" });
            cmd
        }

        fn descendant(&self) -> Result<Option<FixtureProcess>, String> {
            Self::read_descendant(self.directory.path())
        }

        fn read_descendant(directory: &std::path::Path) -> Result<Option<FixtureProcess>, String> {
            let record = match std::fs::read_to_string(directory.join("descendant")) {
                Ok(record) => record,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.to_string()),
            };
            let mut fields = record.split_whitespace();
            let pid = fields
                .next()
                .ok_or("fixture record omitted PID")?
                .parse()
                .map_err(|error| format!("fixture PID: {error}"))?;
            let started = fields
                .next()
                .ok_or("fixture record omitted start identity")?
                .parse()
                .map_err(|error| format!("fixture start identity: {error}"))?;
            #[cfg(target_os = "macos")]
            let started = test_proc_identity::Started {
                sec: started,
                usec: fields
                    .next()
                    .ok_or("fixture record omitted microseconds")?
                    .parse()
                    .map_err(|error| format!("fixture start microseconds: {error}"))?,
            };
            Ok(Some(FixtureProcess { pid, started }))
        }

        fn wait_for_descendant(&self) -> FixtureProcess {
            for _ in 0..100 {
                if let Some(process) = self.descendant().expect("read private fixture identity") {
                    return process;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            panic!("fixture did not publish its owned descendant");
        }

        fn cleanup(&mut self) -> Result<(), String> {
            if !self.cleaned {
                #[cfg(target_os = "macos")]
                let stopped = self
                    .descendant()
                    .and_then(|process| process.map_or(Ok(()), FixtureProcess::stop));
                #[cfg(target_os = "linux")]
                let stopped = self.descendant().and_then(|process| {
                    process.map_or(Ok(()), |process| process.stop(&self.reaper.ownership))
                });
                #[cfg(target_os = "linux")]
                let reaped = self.reaper.finish();
                stopped?;
                #[cfg(target_os = "linux")]
                reaped?;
                self.cleaned = true;
            }
            Ok(())
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    impl Drop for DescendantFixture {
        fn drop(&mut self) {
            if let Err(error) = self.cleanup() {
                if std::thread::panicking() {
                    eprintln!("descendant fixture cleanup failed: {error}");
                } else {
                    panic!("descendant fixture cleanup failed: {error}");
                }
            }
        }
    }

    /// Reap only the privately published descendant, after Linux adopts it.
    /// In particular, never waitpid(-1): run_child owns its direct Child.
    #[cfg(target_os = "linux")]
    struct FixtureReaper {
        finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
        ownership: std::sync::Arc<std::sync::Mutex<()>>,
        thread: Option<std::thread::JoinHandle<Result<(), String>>>,
    }

    #[cfg(target_os = "linux")]
    impl FixtureReaper {
        fn start(directory: &std::path::Path) -> Self {
            use std::sync::atomic::Ordering;
            let mut enabled: libc::c_int = 0;
            // SAFETY: prctl writes one c_int into our valid local buffer.
            assert_eq!(
                unsafe { libc::prctl(libc::PR_GET_CHILD_SUBREAPER, &mut enabled, 0, 0, 0) },
                0,
                "query fixture supervisor"
            );
            assert_eq!(enabled, 1, "fixture must run in its private supervisor");

            let directory = directory.to_path_buf();
            let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let done = finished.clone();
            let ownership = std::sync::Arc::new(std::sync::Mutex::new(()));
            let thread_ownership = ownership.clone();
            let thread = std::thread::spawn(move || {
                let process = loop {
                    if let Some(process) = DescendantFixture::read_descendant(&directory)? {
                        break process;
                    }
                    if done.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                };
                let mut closing = None;
                loop {
                    if done.load(Ordering::Acquire) {
                        let deadline =
                            closing.get_or_insert_with(|| Instant::now() + Duration::from_secs(5));
                        if Instant::now() >= *deadline {
                            return Err("private fixture descendant was not reaped".into());
                        }
                    }
                    let ownership = thread_ownership
                        .lock()
                        .map_err(|_| "fixture signal/reap ownership poisoned".to_string())?;
                    if fixture_started(process.pid)? != Some(process.started) {
                        return Ok(());
                    }
                    let mut status = 0;
                    // SAFETY: only this recorded descendant PID is collected.
                    // Its original parent may still own it, in which case
                    // ECHILD leaves it alone.
                    let reaped = unsafe {
                        libc::waitpid(process.pid as libc::pid_t, &mut status, libc::WNOHANG)
                    };
                    if reaped == process.pid as libc::pid_t {
                        return Ok(());
                    }
                    if reaped < 0 {
                        let error = std::io::Error::last_os_error();
                        if !matches!(error.raw_os_error(), Some(libc::ECHILD) | Some(libc::EINTR)) {
                            return Err(format!("reap fixture {}: {error}", process.pid));
                        }
                    }
                    drop(ownership);
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
            Self {
                finished,
                ownership,
                thread: Some(thread),
            }
        }

        fn finish(&mut self) -> Result<(), String> {
            self.finished
                .store(true, std::sync::atomic::Ordering::Release);
            match self.thread.take() {
                Some(thread) => thread
                    .join()
                    .map_err(|_| "fixture reaper panicked".to_string())?,
                None => Ok(()),
            }
        }
    }

    #[cfg(target_os = "linux")]
    impl Drop for FixtureReaper {
        fn drop(&mut self) {
            if let Err(error) = self.finish() {
                if std::thread::panicking() {
                    eprintln!("fixture reaper failed: {error}");
                } else {
                    panic!("fixture reaper failed: {error}");
                }
            }
        }
    }

    /// Re-execution keeps start-identity recording on the side holding the
    /// unreaped Child. This is a real inherited-pipe sleep, not a runner mock.
    #[cfg(unix)]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    #[ignore = "launched only by DescendantFixture with a private fixture directory"]
    fn owned_descendant_fixture_process() {
        let directory =
            std::env::var_os("KAGI_PROC_FIXTURE_DIR").expect("private fixture directory");
        let directory = std::path::Path::new(&directory);
        let mut child = OwnedChild {
            child: Command::new("sleep")
                .arg("300")
                .spawn()
                .expect("spawn fixture sleep"),
            group_owned: false,
        };
        let process =
            FixtureProcess::record(child.child.id()).expect("record owned fixture identity");
        #[cfg(target_os = "macos")]
        let record = format!(
            "{} {} {}",
            process.pid, process.started.sec, process.started.usec
        );
        #[cfg(not(target_os = "macos"))]
        let record = format!("{} {}", process.pid, process.started);
        std::fs::write(directory.join("publishing"), record).expect("write fixture identity");
        std::fs::rename(directory.join("publishing"), directory.join("descendant"))
            .expect("publish fixture identity atomically");

        if std::env::var_os("KAGI_PROC_FIXTURE_EXIT").as_deref() == Some(std::ffi::OsStr::new("1"))
        {
            // Ownership of this exact descendant transfers to the caller's
            // pre-existing guard. It must remain alive and holding both pipes.
            std::process::exit(0);
        }
        child.wait().expect("reap owned fixture sleep");
    }

    #[cfg(unix)]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn fixture_cleanup_does_not_stop_another_same_marker_child() {
        if !fixture_supervisor(
            "proc::tests::fixture_cleanup_does_not_stop_another_same_marker_child",
        ) {
            return;
        }
        use std::os::unix::process::CommandExt;
        let mut first = DescendantFixture::new();
        let mut second = DescendantFixture::new();
        // Linux also exercises exact reaping after both parents exit:
        // the supervisor adopts two live descendants with the same marker.
        let mut first_command = first.command(cfg!(target_os = "linux"));
        first_command
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut first_child = OwnedChild {
            child: first_command.spawn().expect("spawn first fixture"),
            group_owned: true,
        };
        let mut second_command = second.command(cfg!(target_os = "linux"));
        second_command
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut second_child = OwnedChild {
            child: second_command.spawn().expect("spawn second fixture"),
            group_owned: true,
        };
        let first_process = first.wait_for_descendant();
        let second_process = second.wait_for_descendant();
        #[cfg(target_os = "linux")]
        {
            assert!(first_child
                .wait()
                .expect("reap first fixture parent")
                .success());
            assert!(second_child
                .wait()
                .expect("reap second fixture parent")
                .success());
        }
        assert_ne!(first_process.pid, second_process.pid);
        assert!(first_process.alive().expect("probe first fixture"));
        assert!(second_process.alive().expect("probe second fixture"));

        first.cleanup().expect("clean first fixture");
        #[cfg(target_os = "macos")]
        assert!(first_child.wait().expect("reap first fixture").success());
        assert!(
            second_process
                .alive()
                .expect("probe second fixture after first cleanup"),
            "cleaning one sleep 300 must not stop the other sleep 300"
        );
        second.cleanup().expect("clean second fixture");
        #[cfg(target_os = "macos")]
        assert!(second_child.wait().expect("reap second fixture").success());
        assert!(!first_process.alive().expect("first fixture is gone"));
        assert!(!second_process.alive().expect("second fixture is gone"));
    }

    #[test]
    #[cfg(unix)]
    fn run_child_deadline_is_not_an_exit_and_reaps_the_child() {
        // `exec` so the sleep replaces the shell: one process, which is exactly
        // the direct child the runner owns and kills.
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "exec sleep 3071"]);

        let start = Instant::now();
        let run = run_child(&mut cmd, Duration::from_millis(300), None).expect("spawn");

        assert!(
            start.elapsed() < Duration::from_secs(60),
            "the runner must return on its deadline, not wait out the child"
        );
        // The whole point of #507: there is no exit code to misread.
        match &run.status {
            Err(ProcStop::Deadline { reaped, .. }) => {
                assert!(*reaped, "the timed-out child must be killed and reaped")
            }
            other => panic!("expected a Deadline stop, got {other:?}"),
        }
        assert!(
            !pid_alive(run.pid),
            "the child leaked: it is still running after the deadline (issue #507)"
        );
    }

    // ── #507 review P1: the deadline must survive a pipe-holding descendant ──

    #[test]
    #[cfg(unix)]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn run_child_deadline_reaches_the_caller_despite_a_grandchild_on_the_pipes() {
        if !fixture_supervisor(
            "proc::tests::run_child_deadline_reaches_the_caller_despite_a_grandchild_on_the_pipes",
        ) {
            return;
        }
        // The fixture waits for its real sleep child, which inherits both
        // output pipes. A direct-child-only stop would leave that child alive
        // and an unbounded reader join would never return.
        let fixture = DescendantFixture::new();
        let mut cmd = fixture.command(false);

        let start = Instant::now();
        let run = run_child(&mut cmd, Duration::from_secs(1), None).expect("spawn");
        let elapsed = start.elapsed();
        let descendant = fixture.wait_for_descendant();

        assert!(
            elapsed < Duration::from_secs(60),
            "the deadline never reached the caller: I/O was waited on unbounded"
        );
        assert!(
            matches!(run.status, Err(ProcStop::Deadline { .. })),
            "expected a Deadline stop, got {:?}",
            run.status
        );
        assert!(
            run.group_stopped,
            "killing the group is the stop proof: nothing the command started \
             may be left running"
        );
        assert!(
            descendant
                .wait_until_gone()
                .expect("probe fixture identity"),
            "the pipe-holding descendant survived the group deadline"
        );
    }

    // ── #507 review P2-4: exit 0 does not mean the capture is complete ──

    #[test]
    #[cfg(unix)]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn run_child_clean_exit_with_unfinished_output_is_not_a_clean_capture() {
        if !fixture_supervisor(
            "proc::tests::run_child_clean_exit_with_unfinished_output_is_not_a_clean_capture",
        ) {
            return;
        }
        // The direct child exits 0; the fixture-owned sleep keeps both pipes.
        let fixture = DescendantFixture::new();
        let mut cmd = fixture.command(true);

        let run = run_child(&mut cmd, Duration::from_secs(30), None).expect("spawn");
        let descendant = fixture.wait_for_descendant();

        assert!(
            descendant.alive().expect("probe fixture identity"),
            "the fixture must still be holding the capture pipes"
        );
        assert!(
            !run.group_stopped,
            "exit 0 cannot prove a stop for the descendant the runner did not collect"
        );

        assert_eq!(run.status, Ok(0), "the process really did exit 0");
        assert_eq!(
            run.io,
            Err(ProcIo::Unfinished),
            "a truncated capture must not pass for a complete one"
        );
    }

    #[test]
    #[cfg(unix)]
    fn run_child_clean_exit_with_undelivered_input_is_not_a_clean_capture() {
        // 1 MiB of input into a child that exits without reading it: the write
        // fails with EPIPE, and that must not vanish behind exit 0.
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "exit 0"]);
        let input = vec![b'x'; 1024 * 1024];

        let run = run_child(&mut cmd, Duration::from_secs(30), Some(&input)).expect("spawn");

        assert_eq!(run.status, Ok(0));
        assert!(
            matches!(run.io, Err(ProcIo::Stdin(_))),
            "undelivered input must be reported, got {:?}",
            run.io
        );
    }

    #[test]
    #[cfg(unix)]
    fn run_child_drains_far_more_than_a_pipe_buffer() {
        // 256 KiB on each stream — four times the ~64 KiB pipe buffer. Without
        // a concurrent drain the child blocks in write(2) and the run can only
        // end at the deadline.
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            "dd if=/dev/zero bs=1024 count=256 2>/dev/null | tr '\\0' 'a'; \
             dd if=/dev/zero bs=1024 count=256 2>/dev/null | tr '\\0' 'b' >&2",
        ]);

        let run = run_child(&mut cmd, Duration::from_secs(30), None).expect("spawn");

        assert_eq!(run.status, Ok(0), "chatty child must exit normally");
        assert_eq!(run.io, Ok(()), "the capture must be complete");
        assert_eq!(run.stdout.len(), 256 * 1024, "stdout was truncated");
        assert_eq!(run.stderr.len(), 256 * 1024, "stderr was truncated");
    }

    #[test]
    #[cfg(unix)]
    fn run_child_feeds_oversized_stdin_while_draining_stdout() {
        // The message-gen shape: a prompt larger than the pipe buffer written to
        // a child that is concurrently filling stdout. Both directions must be
        // in flight at once or this deadlocks.
        let prompt = vec![b'p'; 256 * 1024];
        let mut cmd = Command::new("cat");

        let run = run_child(&mut cmd, Duration::from_secs(30), Some(&prompt)).expect("spawn");

        assert_eq!(run.status, Ok(0));
        assert_eq!(run.io, Ok(()), "the whole prompt must be delivered");
        assert_eq!(run.stdout, prompt, "stdin was not fully delivered/echoed");
    }

    #[test]
    #[cfg(unix)]
    fn run_child_reports_exit_code_and_stderr_unchanged() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo out; echo err >&2; exit 7"]);

        let run = run_child(&mut cmd, Duration::from_secs(30), None).expect("spawn");

        assert_eq!(run.status, Ok(7));
        assert_eq!(run.io, Ok(()));
        assert_eq!(run.stdout_lossy(), "out\n");
        assert_eq!(run.stderr_lossy(), "err\n");
    }

    #[test]
    fn run_child_spawn_failure_is_the_only_hard_error() {
        let mut cmd = Command::new("kagi-no-such-binary-507");
        assert!(
            run_child(&mut cmd, Duration::from_secs(5), None).is_err(),
            "a binary that never started must not look like a run"
        );
    }

    #[test]
    fn proc_stop_reads_as_unknown_not_as_failure() {
        assert_eq!(
            ProcStop::Deadline {
                secs: 60,
                reaped: true
            }
            .to_string(),
            "timed out after 60s"
        );
        assert_eq!(
            ProcStop::Deadline {
                secs: 60,
                reaped: false
            }
            .to_string(),
            "timed out after 60s (the local child could not be stopped)"
        );
    }
}
