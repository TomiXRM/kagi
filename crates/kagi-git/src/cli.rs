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
//! - Config hardening: [`HARDENING_ARGS`] plus [`repo_local_overrides`] are
//!   injected as `-c KEY=VALUE` *before* the subcommand so a hostile
//!   `.git/config` cannot turn `git status`/`git fetch` into code execution
//!   (issue #290).
//! - [`check_operand`]: call sites validate remote/ref names read back from the
//!   repository, and pass `--` before positional operands (issue #291).
//!
//! The machinery itself lives in [`crate::proc`]: **the** subprocess runner for
//! the whole codebase (`git`, `ssh`, worktree `command` steps, the message-gen
//! CLIs — issue #507). One owner for the child, its three pipes and its
//! deadline; no caller polls `try_wait` or reads a pipe itself.
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
use std::time::Duration;

use crate::proc::run_child;

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
    "core.hooksPath=/dev/null",
    "-c",
    "core.askPass=",
    "-c",
    "protocol.allow=user",
];

/// The only fsmonitor values a hardened CLI invocation may select.
///
/// `EnabledBuiltin` forces Git's built-in monitor rather than honoring a
/// repository-defined executable command.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FsmonitorMode {
    #[default]
    Disabled,
    EnabledBuiltin,
}

/// Explicit process settings for a hardened Git CLI invocation.
///
/// Production callers use [`run_git`]'s default: the system `git` and a
/// disabled fsmonitor. Probes may select a binary and built-in fsmonitor mode
/// without weakening repository-config hardening.
#[derive(Clone, Copy, Debug, Default)]
pub struct GitCliOptions<'a> {
    pub executable: Option<&'a Path>,
    pub fsmonitor: FsmonitorMode,
}

fn hardening_args(fsmonitor: FsmonitorMode) -> Vec<&'static str> {
    let fsmonitor = match fsmonitor {
        FsmonitorMode::Disabled => "core.fsmonitor=",
        FsmonitorMode::EnabledBuiltin => "core.fsmonitor=true",
    };
    let mut args = Vec::with_capacity(HARDENING_ARGS.len() + 2);
    args.extend(["--no-pager", "-c", fsmonitor]);
    args.extend_from_slice(&HARDENING_ARGS[1..]);
    args
}

/// `-c` overrides for dangerous keys that can legitimately be configured
/// globally, applied only when the **repo-local** or worktree config sets them
/// (issues #290, #647).
///
/// `core.sshCommand` and `credential.helper` deliberately retain a legitimate
/// user-level configuration. `merge.<name>.driver` is similarly dynamic: an
/// attribute may select any driver name, but only a matching config entry gives
/// it an executable command. Scan the untrusted config levels for those entries
/// and disable each discovered driver, while leaving global-only drivers intact.
///
/// If Kagi cannot inspect an untrusted config level, it refuses to start Git.
/// Running an operation after a failed inspection would make the hardening
/// conditional on the attacker-controlled input being readable.
fn repo_local_overrides(repo_dir: &Path) -> Result<Vec<String>, GitError> {
    let repo = git2::Repository::discover(repo_dir)
        .map_err(|error| GitError::Other(format!("cannot inspect repository config: {error}")))?;
    let cfg = repo
        .config()
        .map_err(|error| GitError::Other(format!("cannot read repository config: {error}")))?;

    let (mut ssh, mut cred) = (false, false);
    let mut merge_drivers = std::collections::BTreeSet::new();
    for level in [git2::ConfigLevel::Local, git2::ConfigLevel::Worktree] {
        let snapshot = match cfg.open_level(level) {
            Ok(snapshot) => snapshot,
            Err(error) if error.code() == git2::ErrorCode::NotFound => continue,
            Err(error) => {
                return Err(GitError::Other(format!(
                    "cannot inspect {level:?} repository config: {error}"
                )));
            }
        };
        let entries = snapshot.entries(None).map_err(|error| {
            GitError::Other(format!(
                "cannot enumerate {level:?} repository config: {error}"
            ))
        })?;
        let mut entry_error = None;
        entries
            .for_each(|entry| {
                let name = match entry.name() {
                    Ok(name) => name,
                    Err(error) => {
                        entry_error = Some(error.message().to_owned());
                        return;
                    }
                };
                let lowercase = name.to_ascii_lowercase();
                if lowercase == "core.sshcommand" {
                    ssh = true;
                }
                // `credential.helper` and the per-URL `credential.<url>.helper`.
                if lowercase == "credential.helper"
                    || (lowercase.starts_with("credential.") && lowercase.ends_with(".helper"))
                {
                    cred = true;
                }
                if let Some(driver) = merge_driver_name(name) {
                    merge_drivers.insert(driver.to_owned());
                }
            })
            .map_err(|error| {
                GitError::Other(format!(
                    "cannot enumerate {level:?} repository config: {error}"
                ))
            })?;
        if let Some(error) = entry_error {
            return Err(GitError::Other(format!(
                "cannot inspect {level:?} repository config entry: {error}"
            )));
        }
    }

    let mut out = Vec::new();
    if ssh {
        out.extend(["-c".to_owned(), "core.sshCommand=ssh".to_owned()]);
    }
    if cred {
        out.extend(["-c".to_owned(), "credential.helper=".to_owned()]);
    }
    for driver in merge_drivers {
        out.extend(["-c".to_owned(), format!("merge.{driver}.driver=")]);
    }
    Ok(out)
}

/// Return the case-preserving `<name>` in `merge.<name>.driver`.
///
/// Git configuration section and variable names are case-insensitive, while
/// the driver subsection remains the value selected by `.gitattributes`.
fn merge_driver_name(key: &str) -> Option<&str> {
    const PREFIX: &str = "merge.";
    const SUFFIX: &str = ".driver";
    let driver_end = key.len().checked_sub(SUFFIX.len())?;
    if key.len() <= PREFIX.len() + SUFFIX.len()
        || !key[..PREFIX.len()].eq_ignore_ascii_case(PREFIX)
        || !key[driver_end..].eq_ignore_ascii_case(SUFFIX)
    {
        return None;
    }
    Some(&key[PREFIX.len()..driver_end])
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

/// Repository-local Git environment variables, cleared on every child process.
///
/// Kagi always names the repository explicitly (`current_dir`, plus `-C` where
/// a call site needs it), but naming it is not enough: an inherited `GIT_DIR` /
/// `GIT_WORK_TREE` / `GIT_INDEX_FILE` **overrides** both. `GIT_DIR=<b>/.git
/// GIT_WORK_TREE=<b> git -C <a> stash push` writes the stash into `b` — so a
/// kagi started from a shell (or editor, or hook) that exported them would
/// plan and preflight against one repository and mutate another, which no
/// amount of verification downstream can undo (#623, Codex review).
///
/// The list is `git rev-parse --local-env-vars` (git 2.50.1 — git's own
/// definition of "repository-local", the set it strips when it recurses into a
/// submodule) plus four more it does not list: `GIT_NAMESPACE`,
/// `GIT_CEILING_DIRECTORIES`, and the `GIT_CONFIG_GLOBAL` /
/// `GIT_CONFIG_SYSTEM` file redirects. Removing `GIT_CONFIG_COUNT` also
/// neutralises any `GIT_CONFIG_KEY_<n>` / `GIT_CONFIG_VALUE_<n>` pairs, since
/// git reads them only up to the count.
///
/// This is a removal, not an override, so kagi's own `-c` hardening
/// ([`HARDENING_ARGS`], [`repo_local_overrides`]) stays the only configuration
/// kagi injects.
const REPO_LOCAL_ENV: &[&str] = &[
    // git rev-parse --local-env-vars
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
    // Not listed by git, same class of redirect.
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
];

/// A `git` subprocess command with kagi's standard hardened environment.
///
/// `GIT_ADVICE=0` suppresses git's own advice text on subprocess paths (#353):
/// kagi already writes its own human-facing guidance in the UI, so git's advice
/// would only double up. The other vars keep the child non-interactive, and
/// [`REPO_LOCAL_ENV`] is cleared so the child cannot be pointed at a different
/// repository than the one this command names (#623).
pub fn git_command(repo_dir: &Path) -> std::process::Command {
    git_command_with_executable(repo_dir, Path::new("git"))
}

fn git_command_with_executable(repo_dir: &Path, executable: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(executable);
    for var in REPO_LOCAL_ENV {
        cmd.env_remove(var);
    }
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
///
/// `gh` runs `git` itself, so it inherits the same exposure and gets the same
/// [`REPO_LOCAL_ENV`] clearing (#623).
pub fn gh_command() -> std::process::Command {
    let mut cmd = std::process::Command::new("gh");
    for var in REPO_LOCAL_ENV {
        cmd.env_remove(var);
    }
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
    run_git_with_options(repo_dir, args, GitCliOptions::default())
}

/// Run a hardened Git command with an explicit executable and fsmonitor mode.
///
/// This is for measurement code that must report the exact Git binary and
/// built-in fsmonitor setting it invoked. It still applies every other
/// repository-config override that [`run_git`] does.
pub fn run_git_with_options(
    repo_dir: &Path,
    args: &[&str],
    options: GitCliOptions<'_>,
) -> Result<GitCliOutput, GitError> {
    let local = repo_local_overrides(repo_dir)?;
    let mut full = hardening_args(options.fsmonitor);
    full.extend(local.iter().map(String::as_str));
    full.extend_from_slice(args);

    let executable = options.executable.unwrap_or_else(|| Path::new("git"));
    let mut cmd = git_command_with_executable(repo_dir, executable);
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
    // Exit 0 with a truncated capture is not a successful read: the caller
    // parses this output. Unknown, not success and not a plain failure.
    if let Err(io) = &run.io {
        return Err(GitError::TerminationUnknown(format!(
            "git {}: {io}",
            args.join(" ")
        )));
    }

    Ok(GitCliOutput {
        status,
        stdout: run.stdout_lossy(),
        stderr: run.stderr_lossy(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

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

    /// `get_envs` yields `(key, None)` for each `.env_remove(...)`.
    fn clears_env(cmd: &Command, key: &str) -> bool {
        cmd.get_envs()
            .any(|(k, v)| k == std::ffi::OsStr::new(key) && v.is_none())
    }

    // #623: the hardening is only worth anything if it is on the *shared*
    // builder, so assert the whole set on both builders rather than trusting
    // one operation's end-to-end test. A variable quietly dropped from
    // REPO_LOCAL_ENV would otherwise re-expose every git caller at once.
    #[test]
    fn subprocess_builders_clear_every_repository_local_var() {
        let git = git_command(std::path::Path::new("/tmp"));
        let gh = gh_command();
        for var in REPO_LOCAL_ENV {
            assert!(clears_env(&git, var), "git subprocess must clear {var}");
            assert!(clears_env(&gh, var), "gh subprocess must clear {var}");
        }
        // The ones that actually redirect a repository, named explicitly so a
        // shortened list cannot pass this test by shrinking.
        for var in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_COMMON_DIR",
            "GIT_NAMESPACE",
            "GIT_CEILING_DIRECTORIES",
            "GIT_PREFIX",
            "GIT_CONFIG",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_SYSTEM",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
        ] {
            assert!(
                REPO_LOCAL_ENV.contains(&var),
                "{var} must stay in REPO_LOCAL_ENV"
            );
            assert!(clears_env(&git, var), "git subprocess must clear {var}");
        }
        // Clearing must not have taken the non-interactive hardening with it.
        assert!(has_env(&git, "GIT_TERMINAL_PROMPT", "0"));
        assert!(has_env(&git, "GIT_ASKPASS", "/bin/false"));
        assert!(has_env(&git, "LC_ALL", "C"));
    }
    #[test]
    fn explicit_cli_options_preserve_hardening_and_select_the_binary() {
        assert_eq!(
            &hardening_args(FsmonitorMode::Disabled)[..3],
            ["--no-pager", "-c", "core.fsmonitor="]
        );
        assert_eq!(
            &hardening_args(FsmonitorMode::EnabledBuiltin)[..3],
            ["--no-pager", "-c", "core.fsmonitor=true"]
        );

        let executable = std::path::Path::new("/custom/git");
        let command = git_command_with_executable(std::path::Path::new("/tmp"), executable);
        assert_eq!(command.get_program(), executable);
        assert!(has_env(&command, "GIT_TERMINAL_PROMPT", "0"));
    }
}
