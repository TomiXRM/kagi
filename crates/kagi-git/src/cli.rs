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
//! - A 60-second timeout implemented by polling `try_wait`; on timeout the child
//!   is killed and reaped so no `git` process or pipe-reader thread leaks
//!   (issue #294).
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
/// Returns [`GitError::Other`] when:
/// - The `git` binary is not found or fails to start.
/// - The operation times out after 60 seconds.
pub fn run_git(repo_dir: &Path, args: &[&str]) -> Result<GitCliOutput, GitError> {
    use std::io::Read;
    use std::process::Stdio;

    let mut full: Vec<&str> = HARDENING_ARGS.to_vec();
    let local = repo_local_overrides(repo_dir);
    full.extend_from_slice(&local);
    full.extend_from_slice(args);

    let mut cmd = git_command(repo_dir);
    cmd.args(&full)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| GitError::Other(format!("failed to start git {}: {}", args.join(" "), e)))?;

    // Drain stdout and stderr on dedicated threads so a child that fills a pipe
    // buffer can never deadlock while we wait for it to exit (issue #294): the
    // readers keep draining regardless of what the wait loop is doing.
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });

    // Wait for the child, killing and reaping it on timeout so no git process
    // leaks (issue #294). Killing closes the pipes, which unblocks the two
    // reader threads so they can be joined cleanly.
    let Some(status) = wait_or_kill(&mut child, Duration::from_secs(GIT_CLI_TIMEOUT_SECS)) else {
        let _ = out_reader.join();
        let _ = err_reader.join();
        return Err(GitError::Other(format!(
            "git {} timed out after {}s",
            args.join(" "),
            GIT_CLI_TIMEOUT_SECS
        )));
    };

    let stdout = String::from_utf8_lossy(&out_reader.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned();

    Ok(GitCliOutput {
        status: status.code().unwrap_or(-1),
        stdout,
        stderr,
    })
}

/// Wait up to `timeout` for `child` to exit, polling `try_wait`.
///
/// Returns `Some(status)` if it exits in time. On timeout (or a `try_wait`
/// error) the child is **killed and reaped** and `None` is returned, so a hung
/// `git` process never leaks (issue #294). The reap is bounded — after a kill,
/// the child exits promptly — so this never blocks indefinitely.
pub(crate) fn wait_or_kill(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            // Timed out, or try_wait failed: kill and reap.
            _ => {
                let _ = child.kill();
                // Bounded reap: the child exits promptly once killed.
                for _ in 0..200 {
                    match child.try_wait() {
                        Ok(Some(_)) | Err(_) => break,
                        Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                    }
                }
                return None;
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

        assert!(result.is_none(), "a timed-out child returns None");
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
        assert_eq!(status.and_then(|s| s.code()), Some(0));
    }
}
