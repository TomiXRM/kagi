//! Git CLI wrapper for network operations (fetch/push).
//!
//! Network operations require credential helpers and SSH agents that only work
//! through the system `git` binary (ADR-0009 §3).  This module wraps
//! `std::process::Command` with:
//!
//! - Shell-bypass: arguments are passed as a `&[&str]` array, never interpolated
//!   into a shell string.
//! - `GIT_TERMINAL_PROMPT=0` and `LC_ALL=C` environment variables set on every
//!   invocation so authentication prompts never hang the process.
//! - A 60-second deadline; on expiry the child is killed and reaped so no `git`
//!   process or pipe-reader thread leaks (issue #294), and the caller gets
//!   [`GitError::TerminationUnknown`] rather than something that reads like an
//!   exit (issue #507).
//!
//! [`run_child`] is that machinery on its own: **the** subprocess runner for the
//! whole codebase (`git`, `ssh`, worktree `command` steps, the message-gen CLIs
//! — issue #507). One owner for the child, its three pipes and its deadline;
//! no caller polls `try_wait` or reads a pipe itself.
//! - Config hardening: [`HARDENING_ARGS`] plus [`repo_local_overrides`] are
//!   injected as `-c KEY=VALUE` *before* the subcommand so a hostile
//!   `.git/config` cannot turn `git status`/`git fetch` into code execution
//!   (issue #290).
//! - [`check_operand`]: call sites validate remote/ref names read back from the
//!   repository, and pass `--` before positional operands (issue #291).
//!
//! # Usage
//!
//! ```ignore
//! let out = run_git(repo_path, &["fetch", "origin"])?;
//! if out.status != 0 {
//!     return Err(GitError::Other(format!("fetch failed: {}", out.stderr)));
//! }
//! ```

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::GitError;

// Timeout for git CLI operations (fetch can be slow on large repos).
const GIT_CLI_TIMEOUT_SECS: u64 = 60;

/// The combined output of a single `git` CLI invocation.
#[derive(Debug, Clone)]
pub struct GitCliOutput {
    /// Exit code of the git process.
    pub status: i32,
    /// Captured stdout (UTF-8 lossy).
    pub stdout: String,
    /// Captured stderr (UTF-8 lossy).
    pub stderr: String,
}

/// `-c KEY=VALUE` overrides injected **before** the subcommand on every
/// invocation (issue #290).
///
/// Every one of these keys is a key git will happily execute as a command, and
/// all of them are settable from a repository's own `.git/config` — so merely
/// opening a hostile repository (auto-fetch is on by default) is enough to
/// reach them. None of them has a legitimate use inside kagi:
///
/// - `core.fsmonitor` — runs on every `git status`; kagi never uses it.
/// - `core.hooksPath` — kagi runs no repo hooks (it cannot show their output).
/// - `core.askPass` — kagi already sets `GIT_TERMINAL_PROMPT=0`; askpass is
///   purely an execution vector here.
/// - `protocol.allow=user` — the CVE-2018-17456 class hardening. Verified not
///   to disturb local-path or `file://` remotes (both stay user-initiated).
///
/// `--no-pager` is belt-and-braces: stdout is piped, so `core.pager` is already
/// inert, but the flag costs nothing and does not change the piped output.
const HARDENING_ARGS: &[&str] = &[
    "--no-pager",
    "-c",
    "core.fsmonitor=",
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "core.askPass=",
    "-c",
    "protocol.allow=user",
];

/// `-c` overrides for the two dangerous keys that also have a *legitimate*
/// user-level configuration, applied only when the **repo-local** config sets
/// them (issue #290).
///
/// `core.sshCommand` and `credential.helper` are the two keys #290 lists that a
/// user may reasonably set globally: a per-identity `ssh -i …`, and — on stock
/// macOS — `credential.helper = osxkeychain`, which ships in the *system*
/// config. Clearing them unconditionally (as #290's prescription does, and as
/// `GIT_CONFIG_NOSYSTEM=1` also does) was measured to break `git credential
/// fill` outright, i.e. every HTTPS remote, which is the exact capability this
/// module exists to provide (ADR-0009 §3). So they are neutralised only when
/// the untrusted side — the repository's own local/worktree config — sets them.
///
/// A repo that legitimately sets a *local* helper or sshCommand loses it inside
/// kagi and falls back to nothing (the empty `-c` resets the whole list); the
/// operation then fails loudly rather than silently executing repo-supplied
/// commands.
// ponytail: presence check only, no attempt to re-add the user's global helpers
// after the reset. Add that if local-credential.helper repos turn out common.
fn repo_local_overrides(repo_dir: &Path) -> Vec<&'static str> {
    let Ok(repo) = git2::Repository::discover(repo_dir) else {
        return Vec::new();
    };
    let Ok(cfg) = repo.config() else {
        return Vec::new();
    };

    let (mut ssh, mut cred) = (false, false);
    for level in [git2::ConfigLevel::Local, git2::ConfigLevel::Worktree] {
        let Ok(snapshot) = cfg.open_level(level) else {
            continue;
        };
        let Ok(entries) = snapshot.entries(None) else {
            continue;
        };
        let _ = entries.for_each(|e| {
            let Ok(name) = e.name() else { return };
            let name = name.to_ascii_lowercase();
            if name == "core.sshcommand" {
                ssh = true;
            }
            // `credential.helper` and the per-URL `credential.<url>.helper`.
            if name == "credential.helper"
                || (name.starts_with("credential.") && name.ends_with(".helper"))
            {
                cred = true;
            }
        });
    }

    let mut out = Vec::new();
    if ssh {
        out.extend_from_slice(&["-c", "core.sshCommand=ssh"]);
    }
    if cred {
        out.extend_from_slice(&["-c", "credential.helper="]);
    }
    out
}

/// True when `name` would be read by git as a command-line option instead of an
/// operand (issue #291).
///
/// The single definition of the "leading `-`" rule: `ops/branch.rs` and
/// `ops/tag.rs` use it to reject names kagi is about to *create*, and
/// [`check_operand`] uses it to reject names kagi *read back* from an untrusted
/// repository's config or refs.
pub fn is_flag_like(name: &str) -> bool {
    name.starts_with('-')
}

/// Reject a remote/ref name that came out of the repository (config, refs) and
/// would be parsed as an option — e.g. a remote literally named
/// `--upload-pack=touch /tmp/PWNED;git-upload-pack` (issue #291).
///
/// This is applied by the call sites to the *name* values specifically. It
/// cannot live inside [`run_git`], because callers legitimately pass real flags
/// (`--prune`, `-u`, `--force-with-lease=…`) that a blanket check would reject.
///
/// # Errors
///
/// Returns [`GitError::Other`] when `name` starts with `-`.
pub fn check_operand(kind: &str, name: &str) -> Result<(), GitError> {
    if is_flag_like(name) {
        return Err(GitError::Other(format!(
            "refusing to run git: {} '{}' starts with '-', which git would read \
             as a command-line option",
            kind, name
        )));
    }
    Ok(())
}

/// A `git` subprocess command with kagi's standard hardened environment.
///
/// `GIT_ADVICE=0` suppresses git's own advice text on subprocess paths (#353):
/// kagi already writes its own human-facing guidance in the UI, so git's advice
/// would only double up. The other vars keep the child non-interactive.
pub fn git_command(repo_dir: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(repo_dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ADVICE", "0")
        .env("LC_ALL", "C")
        .env("GIT_EDITOR", "true")
        .env("GIT_ASKPASS", "/bin/false");
    cmd
}

/// A `gh` (GitHub CLI) subprocess command with `GIT_ADVICE=0` set (#353), so
/// the git advice `gh` shells out to does not double up with kagi's own UI.
pub fn gh_command() -> std::process::Command {
    let mut cmd = std::process::Command::new("gh");
    cmd.env("GIT_ADVICE", "0");
    cmd
}

/// Run `git <args>` inside `repo_dir` and return the combined output.
///
/// The [`HARDENING_ARGS`] and [`repo_local_overrides`] `-c` flags are prepended
/// to `args`, so every caller is hardened against a hostile repo config without
/// having to remember (issue #290).
///
/// # Environment
///
/// | Variable             | Value | Effect                                  |
/// |----------------------|-------|-----------------------------------------|
/// | `GIT_TERMINAL_PROMPT`| `0`   | Disable interactive credential prompts  |
/// | `LC_ALL`             | `C`   | Stable locale for output parsing        |
/// | `GIT_EDITOR`         | `true`| No interactive editor (e.g. `--continue` message prompts) |
/// | `GIT_ASKPASS`        | `/bin/false` | No askpass helper is ever run     |
///
/// `GIT_CONFIG_NOSYSTEM` is deliberately **not** set: the system config is not
/// attacker-controlled (it needs root to write) and on macOS it is where
/// `credential.helper = osxkeychain` lives.
///
/// # Errors
///
/// Returns [`GitError::Other`] when the `git` binary is not found or fails to
/// start (nothing ran), and [`GitError::TerminationUnknown`] when the 60-second
/// deadline expires — the wait was cut short, which is not proof the operation
/// did not happen (issue #507).
pub fn run_git(repo_dir: &Path, args: &[&str]) -> Result<GitCliOutput, GitError> {
    let mut full: Vec<&str> = HARDENING_ARGS.to_vec();
    let local = repo_local_overrides(repo_dir);
    full.extend_from_slice(&local);
    full.extend_from_slice(args);

    let mut cmd = git_command(repo_dir);
    cmd.args(&full);

    let run = run_child(&mut cmd, Duration::from_secs(GIT_CLI_TIMEOUT_SECS), None)
        .map_err(|e| GitError::Other(format!("failed to start git {}: {}", args.join(" "), e)))?;

    // A deadline that expires is not an exit: `git push` may already have moved
    // the remote. Keep it a `TerminationUnknown` so the app records `Unknown`
    // and never auto-retries (ADR-0177).
    let status = run
        .status
        .clone()
        .map_err(|stop| GitError::TerminationUnknown(format!("git {} {}", args.join(" "), stop)))?;

    Ok(GitCliOutput {
        status,
        stdout: run.stdout_lossy(),
        stderr: run.stderr_lossy(),
    })
}

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

/// The result of one subprocess run: the drained output, plus **either** a real
/// exit code or the reason there is none.
///
/// The split is the point of [`run_child`]: a caller cannot accidentally read a
/// cut-short wait as an exit code, because there is no code to read.
#[derive(Debug, Clone)]
pub struct ProcRun {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// `Ok(code)` — the process really exited. `Err(_)` — see [`ProcStop`].
    pub status: Result<i32, ProcStop>,
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
///   comes back with [`ProcStop`] instead of an exit code. Killing closes the
///   pipes, which unblocks the reader/writer threads so they join instead of
///   leaking.
///
/// The caller supplies the program, args, env and cwd; the runner sets the
/// three `Stdio`s itself (they are what it owns).
///
/// # Errors
///
/// `Err(io::Error)` **only** when the child never started — nothing ran, so
/// nothing can have changed. Everything else is a `ProcRun`.
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

    let mut child = cmd.spawn()?;

    let writer = match (stdin, child.stdin.take()) {
        (Some(data), Some(mut pipe)) => {
            let data = data.to_vec();
            Some(std::thread::spawn(move || {
                let _ = pipe.write_all(&data);
                // `pipe` is dropped here → the child sees EOF on stdin.
            }))
        }
        _ => None,
    };
    let out_reader = drain(child.stdout.take());
    let err_reader = drain(child.stderr.take());

    let status = wait_or_kill(&mut child, timeout);

    // Every pipe is closed once the child is gone, so the threads have finished
    // (or are about to) and joining them is bounded. The one case where the
    // child may still be holding them open is a kill we could not confirm:
    // joining there would hang the caller on a process it does not control, so
    // the threads are detached and the output is dropped — the status already
    // says the outcome is unknown.
    let confirmed = status.as_ref().err().is_none_or(ProcStop::reaped);
    let (stdout, stderr) = if confirmed {
        if let Some(w) = writer {
            let _ = w.join();
        }
        (
            out_reader.join().unwrap_or_default(),
            err_reader.join().unwrap_or_default(),
        )
    } else {
        (Vec::new(), Vec::new())
    };

    Ok(ProcRun {
        stdout,
        stderr,
        status: status.map(|s| s.code().unwrap_or(-1)),
    })
}

/// Read a child pipe to EOF on its own thread.
fn drain<R: std::io::Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    })
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

    /// `Command::get_envs` yields `(key, Some(val))` for each `.env(...)`.
    fn has_env(cmd: &Command, key: &str, val: &str) -> bool {
        cmd.get_envs()
            .any(|(k, v)| k == std::ffi::OsStr::new(key) && v == Some(std::ffi::OsStr::new(val)))
    }

    // #353: git and gh subprocesses must carry GIT_ADVICE=0 so git's own advice
    // text does not double up with kagi's UI guidance.
    #[test]
    fn subprocess_builders_set_git_advice_zero() {
        let git = git_command(std::path::Path::new("/tmp"));
        assert!(
            has_env(&git, "GIT_ADVICE", "0"),
            "git subprocess must set GIT_ADVICE=0"
        );
        let gh = gh_command();
        assert!(
            has_env(&gh, "GIT_ADVICE", "0"),
            "gh subprocess must set GIT_ADVICE=0"
        );
    }

    #[test]
    fn wait_or_kill_kills_and_reaps_on_timeout() {
        // A child that would otherwise run for 30s.
        let mut child = Command::new("sleep")
            .arg("30")
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
            start.elapsed() < Duration::from_secs(5),
            "wait_or_kill must return promptly, not wait out the full sleep"
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
    /// The runner kills its child, so nothing it spawned may still match.
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
            start.elapsed() < Duration::from_secs(10),
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
            !any_process_matching("sleep 3071"),
            "the child leaked: it is still running after the deadline (issue #507)"
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
        assert_eq!(run.stdout, prompt, "stdin was not fully delivered/echoed");
    }

    #[test]
    #[cfg(unix)]
    fn run_child_reports_exit_code_and_stderr_unchanged() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo out; echo err >&2; exit 7"]);

        let run = run_child(&mut cmd, Duration::from_secs(30), None).expect("spawn");

        assert_eq!(run.status, Ok(7));
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
