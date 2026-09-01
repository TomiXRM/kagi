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
//! - A 60-second timeout implemented as a background thread + `mpsc::recv_timeout`.
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
use std::sync::mpsc;
use std::time::Duration;

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
    use std::process::{Command, Stdio};

    let mut full: Vec<&str> = HARDENING_ARGS.to_vec();
    let local = repo_local_overrides(repo_dir);
    full.extend_from_slice(&local);
    full.extend_from_slice(args);

    let mut cmd = Command::new("git");
    cmd.args(&full)
        .current_dir(repo_dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .env("GIT_EDITOR", "true")
        .env("GIT_ASKPASS", "/bin/false")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| GitError::Other(format!("failed to start git {}: {}", args.join(" "), e)))?;

    // Run `child.wait_with_output()` on a background thread so we can apply a
    // timeout.  `std::process::Child` is not `Send` in all configurations, so
    // we use a channel to receive the result.
    let (tx, rx) = mpsc::channel::<Result<std::process::Output, std::io::Error>>();

    // Spawn a thread that waits for the child to finish.
    std::thread::spawn(move || {
        let result = child.wait_with_output();
        let _ = tx.send(result);
    });

    // Wait up to `GIT_CLI_TIMEOUT_SECS` for the result.
    let output = rx
        .recv_timeout(Duration::from_secs(GIT_CLI_TIMEOUT_SECS))
        .map_err(|_| {
            GitError::Other(format!(
                "git {} timed out after {}s",
                args.join(" "),
                GIT_CLI_TIMEOUT_SECS
            ))
        })?
        .map_err(|e| GitError::Other(format!("git {} I/O error: {}", args.join(" "), e)))?;

    let status = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    Ok(GitCliOutput {
        status,
        stdout,
        stderr,
    })
}
