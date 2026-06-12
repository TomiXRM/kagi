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

/// Run `git <args>` inside `repo_dir` and return the combined output.
///
/// # Environment
///
/// | Variable             | Value | Effect                                  |
/// |----------------------|-------|-----------------------------------------|
/// | `GIT_TERMINAL_PROMPT`| `0`   | Disable interactive credential prompts  |
/// | `LC_ALL`             | `C`   | Stable locale for output parsing        |
///
/// # Errors
///
/// Returns [`GitError::Other`] when:
/// - The `git` binary is not found or fails to start.
/// - The operation times out after 60 seconds.
pub fn run_git(repo_dir: &Path, args: &[&str]) -> Result<GitCliOutput, GitError> {
    use std::process::{Command, Stdio};

    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(repo_dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = cmd.spawn().map_err(|e| {
        GitError::Other(format!(
            "failed to start git {}: {}",
            args.join(" "),
            e
        ))
    })?;

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
        .map_err(|e| {
            GitError::Other(format!("git {} I/O error: {}", args.join(" "), e))
        })?;

    let status = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    Ok(GitCliOutput {
        status,
        stdout,
        stderr,
    })
}
