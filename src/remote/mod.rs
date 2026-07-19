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

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use kagi_domain::refs::Worktree;
use kagi_domain::remote::{
    self, RemoteDirEntry, RemoteHost, RemoteRepoSummary, RepoProbe, SSH_CONNECT_TIMEOUT_SECS,
};
use kagi_domain::remote_diff as rd;
use kagi_domain::remote_snapshot as rs;
use kagi_domain::status::FileStatus;

use kagi_git::{FileDiff, Head, RepoSnapshot};

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

/// Run a remote read whose **non-zero exit is acceptable** (an empty repo makes
/// `git log`/`for-each-ref`/`stash list` fail or print nothing). A real
/// transport failure is still surfaced; any other non-zero is treated as empty
/// output. Used by [`remote_snapshot`].
fn run_lenient(host: &RemoteHost, remote_tokens: &[&str]) -> Result<String, RemoteError> {
    let out = run_ssh(host, remote_tokens)?;
    if out.code == 0 {
        Ok(out.stdout)
    } else if is_transport_failure(&out) {
        Err(RemoteError::NonZero {
            code: out.code,
            stderr: out.stderr,
        })
    } else {
        Ok(String::new())
    }
}

/// Build a full [`RepoSnapshot`] for the remote repository at `repo` over SSH —
/// the `GitBackend` read path for a remote repo (ADR-0089 Phase 2). It runs the
/// same reads the local `git2` snapshot does (HEAD, log, branches, remote
/// branches, tags, stashes) as `git` commands and assembles the identical
/// domain types, so the existing graph/diff views can render it unchanged.
///
/// Working-tree `status` is left empty for now (a remote read-only view does not
/// need a porcelain parse yet — a deliberate follow-up), and `worktrees` is the
/// single main entry. Both are valid for the graph view.
pub fn remote_snapshot(
    host: &RemoteHost,
    repo: &str,
    commit_limit: usize,
) -> Result<RepoSnapshot, RemoteError> {
    // HEAD: branch (attached) + commit (exists?) → Head.
    let branch_out = run_ssh(
        host,
        &["git", "-C", repo, "symbolic-ref", "-q", "--short", "HEAD"],
    )?;
    let branch = (branch_out.code == 0).then(|| branch_out.stdout.trim().to_string());
    let sha_out = run_ssh(
        host,
        &["git", "-C", repo, "rev-parse", "-q", "--verify", "HEAD"],
    )?;
    let sha = (sha_out.code == 0).then(|| sha_out.stdout.trim().to_string());
    let head = rs::head_from(branch.as_deref(), sha.as_deref());

    // Commits, topological order, capped. Match the local `git2` backend's ref
    // set exactly — branches + tags + remote-tracking branches + HEAD — and in
    // particular do NOT use `--all`, which would pull in `refs/stash` and render
    // stash commits as ordinary commits (parity bug; ADR-0089). HEAD is passed as
    // its resolved SHA (when one exists) so an unborn HEAD can't fail the command.
    let limit_arg = format!("-{commit_limit}");
    let log_fmt = format!("--pretty=format:{}", rs::LOG_FORMAT);
    let mut log_args: Vec<&str> = vec![
        "git",
        "-C",
        repo,
        "log",
        "--topo-order",
        limit_arg.as_str(),
        log_fmt.as_str(),
        "--branches",
        "--tags",
        "--remotes",
    ];
    if let Some(s) = sha.as_deref() {
        log_args.push(s);
    }
    let commits = rs::parse_commits(&run_lenient(host, &log_args)?);

    // Refs.
    let branch_fmt = format!("--format={}", rs::BRANCH_FORMAT);
    let branches = rs::parse_local_branches(&run_lenient(
        host,
        &[
            "git",
            "-C",
            repo,
            "for-each-ref",
            branch_fmt.as_str(),
            "refs/heads",
        ],
    )?);
    let remote_fmt = format!("--format={}", rs::REMOTE_BRANCH_FORMAT);
    let remote_branches = rs::parse_remote_branches(&run_lenient(
        host,
        &[
            "git",
            "-C",
            repo,
            "for-each-ref",
            remote_fmt.as_str(),
            "refs/remotes",
        ],
    )?);
    let tag_fmt = format!("--format={}", rs::TAG_FORMAT);
    let tags = rs::parse_tags(&run_lenient(
        host,
        &[
            "git",
            "-C",
            repo,
            "for-each-ref",
            tag_fmt.as_str(),
            "refs/tags",
        ],
    )?);

    // Stashes.
    let stash_fmt = format!("--format={}", rs::STASH_FORMAT);
    let stashes = rs::parse_stashes(&run_lenient(
        host,
        &["git", "-C", repo, "stash", "list", stash_fmt.as_str()],
    )?);

    // Working-tree status (porcelain v1).
    let status = rs::parse_status_v1(&run_lenient(
        host,
        &["git", "-C", repo, "status", "--porcelain"],
    )?);

    let branch_name = match &head {
        Head::Attached { branch, .. } | Head::Unborn { branch } => Some(branch.clone()),
        Head::Detached { .. } => None,
    };
    let worktrees = vec![Worktree {
        name: "main".to_string(),
        path: PathBuf::from(repo),
        branch: branch_name,
        is_current: true,
        is_main: true,
        wip: None,
        locked: false,
        lock_reason: None,
    }];

    Ok(RepoSnapshot {
        head,
        commits,
        branches,
        remote_branches,
        tags,
        status,
        stashes,
        worktrees,
        // ADR-0128: no cleanup classification over SSH — the walk would need a
        // local object store. The remote view just shows an empty table.
        cleanup_rows: Vec::new(),
        // Remote read-only views have no local FETCH_HEAD to date; the
        // fetch-age indicator (ADR-0127) stays hidden for them.
        last_fetch_secs: None,
    })
}

/// The files changed in commit `sha` of the remote repo (first-parent diff,
/// rename-detected), via `git diff-tree … --name-status` (ADR-0089 Phase 2c).
pub fn remote_commit_changed_files(
    host: &RemoteHost,
    repo: &str,
    sha: &str,
) -> Result<Vec<FileStatus>, RemoteError> {
    let stdout = run_checked(
        host,
        &[
            "git",
            "-C",
            repo,
            "diff-tree",
            "--no-commit-id",
            "--first-parent",
            "-r",
            "-M",
            "--root",
            "--name-status",
            sha,
        ],
    )?;
    Ok(rd::parse_name_status(&stdout))
}

/// The unified diff of a single `path` in commit `sha` of the remote repo
/// (first-parent), via `git show … -- <path>` (ADR-0089 Phase 2c).
pub fn remote_commit_file_diff(
    host: &RemoteHost,
    repo: &str,
    sha: &str,
    path: &str,
) -> Result<FileDiff, RemoteError> {
    let stdout = run_checked(
        host,
        &[
            "git",
            "-C",
            repo,
            "show",
            "--first-parent",
            "-M",
            "--format=",
            sha,
            "--",
            path,
        ],
    )?;
    Ok(rd::parse_file_diff(&stdout))
}

/// Drop the stash entry `stash@{index}` on the remote repository over SSH
/// (ADR-0089 Phase 3 — the first remote *write*). Mirrors the local
/// `execute_stash_drop` (ADR-0087, Destructive): it only removes the stash ref,
/// never touches the working tree, and is gated behind the danger-confirm modal
/// + oplog in the UI. `git stash drop` prints the dropped entry to stdout on
/// success; a non-zero exit (e.g. the index no longer exists) is surfaced as a
/// [`RemoteError`].
pub fn remote_stash_drop(
    host: &RemoteHost,
    repo: &str,
    index: usize,
) -> Result<String, RemoteError> {
    let stash_ref = format!("stash@{{{index}}}");
    let stdout = run_checked(
        host,
        &["git", "-C", repo, "stash", "drop", stash_ref.as_str()],
    )?;
    Ok(stdout.trim().to_string())
}

/// Pull the current branch of the remote repository over SSH (ADR-0089 Phase 3).
/// Runs `git -C <repo> pull` *on the host*, so the host's own credentials,
/// network, and config reach its `origin` — Kagi only carries the command over
/// the system-ssh transport. Returns git's summary (`Fast-forward`,
/// `Already up to date.`, merge text) on success. A non-zero exit (no upstream,
/// auth failure, or a merge conflict that leaves the host mid-merge) is surfaced
/// as a [`RemoteError`] for the UI to show; the user resolves conflicts on the
/// host (a remote conflict editor is out of scope for this slice).
pub fn remote_pull(host: &RemoteHost, repo: &str) -> Result<String, RemoteError> {
    // Combine stdout+stderr in the message: git prints progress to stderr but
    // the "Fast-forward" / "Already up to date." summary to stdout.
    let out = run_ssh(host, &["git", "-C", repo, "pull"])?;
    if out.code == 0 {
        let summary = out.stdout.trim();
        Ok(if summary.is_empty() {
            "pull complete".to_string()
        } else {
            summary.lines().last().unwrap_or(summary).to_string()
        })
    } else {
        Err(RemoteError::NonZero {
            code: out.code,
            stderr: out.stderr,
        })
    }
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
