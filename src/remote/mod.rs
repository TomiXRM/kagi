//! Remote-over-SSH I/O (ADR-0089) — the read-only foundational slice.
//!
//! The **only** layer that spawns the system `ssh` binary. The pure model
//! (connection parsing, argv construction, output parsing) lives in
//! [`kagi_domain::remote`]; callers use *this* module, never `std::process`
//! directly. This mirrors how `src/git/cli.rs` (network git) pairs the system
//! `git` binary with pure parsing — same shell-bypass, same whole-command
//! timeout, same non-interactive hardening (ADR-0009).
//!
//! How VS Code's Remote-SSH does it: it bootstraps over the system `ssh`
//! binary, then *pushes a `vscode-server`* to the host and talks to it over a
//! multiplexed channel. Kagi's MVP is **agentless** — it deploys nothing and
//! instead runs short, read-only commands (`true`, `pwd`, `ls`, `git
//! rev-parse`, `git log`) over `ssh` and parses their output. A resident helper
//! (the VS Code-server analogue) is a deliberate later step (ADR-0089
//! "Future"), not part of this slice.
//!
//! Everything here is **read-only**: it inspects the remote host (reachability,
//! directory listing, repository detection, HEAD summary). No write/operation
//! path exists yet — that goes through the `OperationController` pipeline in a
//! later phase, never directly from here.

use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use kagi_domain::remote::{
    self, RemoteDirEntry, RemoteHost, RemoteRepoSummary, RepoProbe, SSH_CONNECT_TIMEOUT_SECS,
};

/// Whole-command backstop timeout. ssh's own `ConnectTimeout`
/// ([`SSH_CONNECT_TIMEOUT_SECS`]) bounds the handshake; this bounds the entire
/// invocation (a hung remote command, a stalled transfer) so the UI never waits
/// forever. Comfortably larger than the connect timeout.
const SSH_COMMAND_TIMEOUT_SECS: u64 = SSH_CONNECT_TIMEOUT_SECS as u64 + 20;

/// A failure running a remote command over SSH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteError {
    /// The `ssh` binary could not be started (not installed, etc.).
    Spawn(String),
    /// The command exceeded [`SSH_COMMAND_TIMEOUT_SECS`].
    Timeout,
    /// ssh / the remote command exited non-zero. `stderr` is the captured
    /// message (e.g. "Host key verification failed", "Permission denied",
    /// "No such file or directory").
    NonZero { code: i32, stderr: String },
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteError::Spawn(e) => write!(f, "failed to start ssh: {e}"),
            RemoteError::Timeout => {
                write!(f, "ssh command timed out after {SSH_COMMAND_TIMEOUT_SECS}s")
            }
            RemoteError::NonZero { code, stderr } => {
                write!(f, "ssh exited {code}: {}", stderr.trim())
            }
        }
    }
}

impl std::error::Error for RemoteError {}

/// The captured result of one `ssh` invocation.
struct SshOutput {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Run `remote_tokens` on `host` via the system `ssh` binary and capture output.
///
/// Hardening (parallels `git/cli.rs`):
/// - **Shell-bypass locally**: args are an argv array, never an interpolated
///   shell string. The *remote* command is shell-quoted by
///   [`kagi_domain::remote`] so it survives the remote login shell intact.
/// - **Non-interactive**: `BatchMode=yes` (from the domain layer) + `LC_ALL=C`
///   for stable, parseable output — ssh never blocks on a prompt.
/// - **Timeout**: a background thread + `recv_timeout` backstop.
fn run_ssh(host: &RemoteHost, remote_tokens: &[&str]) -> Result<SshOutput, RemoteError> {
    let args = host.ssh_invocation(remote_tokens);

    let child = Command::new("ssh")
        .args(&args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| RemoteError::Spawn(e.to_string()))?;

    // `child.wait_with_output()` on a worker thread so we can time out.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    let output = rx
        .recv_timeout(Duration::from_secs(SSH_COMMAND_TIMEOUT_SECS))
        .map_err(|_| RemoteError::Timeout)?
        .map_err(|e| RemoteError::Spawn(e.to_string()))?;

    Ok(SshOutput {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Run a remote command, returning stdout on success (exit 0) or
/// [`RemoteError::NonZero`] otherwise.
fn run_checked(host: &RemoteHost, remote_tokens: &[&str]) -> Result<String, RemoteError> {
    let out = run_ssh(host, remote_tokens)?;
    if out.code == 0 {
        Ok(out.stdout)
    } else {
        Err(RemoteError::NonZero {
            code: out.code,
            stderr: out.stderr,
        })
    }
}

/// Verify the host is reachable and authentication succeeds, by running the
/// remote no-op `true`. A clean `Ok(())` means Kagi can run further commands.
///
/// On an unknown host key or password-only host this returns
/// [`RemoteError::NonZero`] (BatchMode never prompts) — the caller should tell
/// the user to connect once via a terminal to record the key / set up a key.
pub fn check_connection(host: &RemoteHost) -> Result<(), RemoteError> {
    run_checked(host, &["true"]).map(|_| ())
}

/// The remote login directory (the user's home), via `pwd`. A natural starting
/// point for the directory picker.
pub fn home_dir(host: &RemoteHost) -> Result<String, RemoteError> {
    let out = run_checked(host, &["pwd"])?;
    let dir = out.lines().next().unwrap_or("").trim().to_string();
    if dir.is_empty() {
        Ok("/".to_string())
    } else {
        Ok(dir)
    }
}

/// List a remote directory (`ls -1ApL <path>`), classified into dirs/files.
pub fn list_dir(host: &RemoteHost, path: &str) -> Result<Vec<RemoteDirEntry>, RemoteError> {
    let stdout = run_checked(host, &["ls", "-1ApL", path])?;
    Ok(remote::parse_ls(&stdout))
}

/// Detect whether `path` is inside a Git work tree, and its toplevel if so.
///
/// `git -C <path> rev-parse --is-inside-work-tree --show-toplevel`. A non-zero
/// exit ("not a git repository") is **not** an error here — it is the expected
/// "no repo" answer, so it maps to [`RepoProbe::not_a_repo`]. Only a transport
/// failure (unreachable host, timeout) is a [`RemoteError`].
pub fn probe_repo(host: &RemoteHost, path: &str) -> Result<RepoProbe, RemoteError> {
    let out = run_ssh(
        host,
        &[
            "git",
            "-C",
            path,
            "rev-parse",
            "--is-inside-work-tree",
            "--show-toplevel",
        ],
    )?;
    if out.code == 0 {
        Ok(remote::parse_repo_probe(&out.stdout))
    } else if is_transport_failure(&out) {
        Err(RemoteError::NonZero {
            code: out.code,
            stderr: out.stderr,
        })
    } else {
        // git ran and said "not a repository" (or git missing on the path) —
        // a definitive negative answer, not a connection problem.
        Ok(RepoProbe::not_a_repo())
    }
}

/// A one-line HEAD summary of the remote repository at `path`
/// (`git -C <path> log -1 --format=%h%x1f%s%x1f%D`). `Ok(None)` means an
/// empty/unborn repository (no HEAD commit yet).
pub fn repo_summary(
    host: &RemoteHost,
    path: &str,
) -> Result<Option<RemoteRepoSummary>, RemoteError> {
    let stdout = run_checked(
        host,
        &["git", "-C", path, "log", "-1", "--format=%h%x1f%s%x1f%D"],
    )?;
    Ok(remote::parse_repo_summary(&stdout))
}

/// Heuristic: did the SSH transport itself fail (so we should surface an
/// error), versus the remote `git` running and reporting "not a repository"?
///
/// ssh emits a recognizable banner on transport-level failure; `git`'s "not a
/// repository" goes to stderr without those markers. Conservative: anything
/// matching a known ssh-failure phrase is treated as a real error.
fn is_transport_failure(out: &SshOutput) -> bool {
    const SSH_FAILURE_MARKERS: [&str; 6] = [
        "Permission denied",
        "Host key verification failed",
        "Could not resolve hostname",
        "Connection refused",
        "Connection timed out",
        "Operation timed out",
    ];
    let stderr = &out.stderr;
    SSH_FAILURE_MARKERS.iter().any(|m| stderr.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_failure_detection() {
        let perm = SshOutput {
            code: 255,
            stdout: String::new(),
            stderr: "alice@host: Permission denied (publickey).".into(),
        };
        assert!(is_transport_failure(&perm));

        let not_repo = SshOutput {
            code: 128,
            stdout: String::new(),
            stderr: "fatal: not a git repository (or any of the parent directories): .git".into(),
        };
        assert!(!is_transport_failure(&not_repo));
    }

    #[test]
    fn error_display_is_readable() {
        let e = RemoteError::NonZero {
            code: 255,
            stderr: "  Host key verification failed.\n".into(),
        };
        assert_eq!(
            e.to_string(),
            "ssh exited 255: Host key verification failed."
        );
    }
}
