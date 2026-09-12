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
    /// The child's process id — and, because it is spawned as its own group
    /// leader, the id of the group holding everything it started. The handle a
    /// later read proves stop against when the kill could not (ADR-0175).
    pub pid: u32,
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
    let mut child = cmd.spawn()?;
    // The child is its own group leader, so the group id is its pid.
    let pid = child.id();

    // Each collector reports through the channel when it is done, so the wait
    // for them can be bounded (a `JoinHandle` cannot). The handles are kept only
    // for ownership: the janitor joins them if any read is still in flight.
    let (tx, rx) = std::sync::mpsc::channel();
    let mut threads = Vec::new();
    if let (Some(data), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let (data, tx) = (data.to_vec(), tx.clone());
        threads.push(std::thread::spawn(move || {
            let error = pipe.write_all(&data).err();
            drop(pipe); // → the child sees EOF on stdin.
            let _ = tx.send(Collected::Stdin(error));
        }));
    }
    if let Some(pipe) = child.stdout.take() {
        threads.push(drain(pipe, tx.clone(), Collected::Stdout));
    }
    if let Some(pipe) = child.stderr.take() {
        threads.push(drain(pipe, tx.clone(), Collected::Stderr));
    }
    // Our own sender must go, or `Disconnected` (every collector gone) never
    // arrives.
    drop(tx);

    let status = wait_or_kill(&mut child, timeout);
    let (stdout, stderr, io) = collect(&rx, threads.len());

    // Hand off whenever something is still ours to own: an unreaped child, or a
    // read that has not ended. On the ordinary path the collectors are done, so
    // this join is immediate.
    if io.is_err() || status.as_ref().err().is_some_and(|s| !s.reaped()) {
        std::thread::spawn(move || {
            // Blocks for as long as the descendant lives — off the caller's
            // path, but with an owner: the child is reaped when it finally
            // exits, and the reader threads/fds are released with it.
            let _ = child.wait();
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
    })
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
// A platform without a probe cannot answer this, and both answers are wrong:
// `true` wedges the scope for the life of the process, `false` releases a lease
// on no evidence. Refuse to build there instead of shipping either (#702 Codex
// review). Kagi is macOS-only today, so this arm is unreachable.
#[cfg(not(unix))]
compile_error!(
    "the reconcile exit needs a process-group probe: implement `group_alive` \
     for this platform before targeting it"
);

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
/// child is **killed and reaped** and the [`ProcStop`] says so, so a hung
/// process never leaks (issue #294) and no caller can mistake the cut-short
/// wait for an exit (issue #507). The reap is bounded — after a kill the child
/// exits promptly — so this never blocks indefinitely; if the bound is hit,
/// `reaped` is false.
pub(crate) fn wait_or_kill(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<std::process::ExitStatus, ProcStop> {
    let deadline = Instant::now() + timeout;
    let secs = timeout.as_secs();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            // Timed out, or try_wait failed: kill and reap.
            other => {
                let error = other.err().map(|e| e.to_string());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    /// True if `pid` is still a live (un-reaped) process.
    fn pid_alive(pid: u32) -> bool {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Every child leads its own process group, so the stop proof can be about
    /// everything the command started rather than the one process we hold a
    /// handle to (#702 re-review). Without this, `group_alive` and a plain pid
    /// probe are the same check and a surviving transport helper reads as gone.
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
        let mut child = Command::new("sleep")
            .arg("300")
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        assert!(pid_alive(pid), "sleep should be running before the timeout");

        let start = Instant::now();
        let result = wait_or_kill(&mut child, Duration::from_millis(200));

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
        let mut child = Command::new("true").spawn().expect("spawn true");
        let status = wait_or_kill(&mut child, Duration::from_secs(5));
        assert_eq!(status.ok().and_then(|s| s.code()), Some(0));
    }

    // ── issue #507: one runner owning the child, its pipes and its deadline ──

    /// True while any process on the machine has `token` in its command line.
    #[cfg(unix)]
    fn any_process_matching(token: &str) -> bool {
        let out = Command::new("ps")
            .args(["-A", "-o", "args="])
            .output()
            .expect("ps");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|l| l.contains(token))
    }

    /// Wait (briefly) for `token` to leave the process table. Keyed on the
    /// observable event rather than on a wall-clock margin, so a loaded machine
    /// cannot make this flap.
    #[cfg(unix)]
    fn wait_until_gone(token: &str) -> bool {
        for _ in 0..100 {
            if !any_process_matching(token) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// Stop a descendant a test deliberately left holding the pipes. It is not
    /// the runner's to kill — but it is the test's to clean up.
    #[cfg(unix)]
    fn pkill(token: &str) {
        let _ = Command::new("pkill").args(["-f", token]).status();
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
            wait_until_gone("sleep 3071"),
            "the child leaked: it is still running after the deadline (issue #507)"
        );
    }

    // ── #507 review P1: the deadline must survive a pipe-holding descendant ──

    #[test]
    #[cfg(unix)]
    fn run_child_deadline_reaches_the_caller_despite_a_grandchild_on_the_pipes() {
        // The shell stays alive (`wait`), so the deadline kills *it* — but the
        // grandchild inherited stdout/stderr, so the reads cannot reach EOF.
        // Joining them would hang here forever; the caller must still be freed.
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 3072 & wait"]);

        let start = Instant::now();
        let run = run_child(&mut cmd, Duration::from_millis(300), None).expect("spawn");
        let elapsed = start.elapsed();
        pkill("sleep 3072");

        assert!(
            elapsed < Duration::from_secs(60),
            "the deadline never reached the caller: I/O was waited on unbounded"
        );
        assert!(
            matches!(run.status, Err(ProcStop::Deadline { .. })),
            "expected a Deadline stop, got {:?}",
            run.status
        );
        assert_eq!(
            run.io,
            Err(ProcIo::Unfinished),
            "an unfinished capture must say so"
        );
    }

    // ── #507 review P2-4: exit 0 does not mean the capture is complete ──

    #[test]
    #[cfg(unix)]
    fn run_child_clean_exit_with_unfinished_output_is_not_a_clean_capture() {
        // The shell exits 0 immediately; its background child keeps the pipes.
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 3073 & exit 0"]);

        let run = run_child(&mut cmd, Duration::from_secs(30), None).expect("spawn");
        pkill("sleep 3073");

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
