//! P2 CLI-environment probes for #627.
//!
//! This module is deliberately independent of the root `backend_probe` registry.
//! P0's integration owner registers [`CliCapability`] after the P2 review. Until
//! then it is compiled through a temporary wrapper, so its probe contract cannot
//! silently rot while the dispatcher remains P0-owned.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use kagi_git::benchmark::{HarnessError, ProbeContext, ProbeOperation};
use kagi_git::run_child;
use serde_json::{json, Value};

const CANDIDATE: &str = "merge-tree-write-tree";
const REQUIRED_VERSION: Version = Version::new(2, 38, 0);
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Probes the only version-gated CLI candidate established before #627's
/// experiments: `git merge-tree --write-tree`.
///
/// The probe records process facts. It must not claim that Kagi already has a
/// capability gate: current production `run_git` has neither an executable
/// injection seam nor a Git-minimum-version error variant.
pub struct CliCapability;

impl ProbeOperation for CliCapability {
    fn name(&self) -> &'static str {
        "cli-capability"
    }

    fn mutates_fixture(&self) -> bool {
        // Git 2.38+ writes merge result objects for `--write-tree`. The
        // capability check must therefore receive a fresh copy per iteration;
        // P0 fingerprints the object database, not only HEAD/index/worktree.
        true
    }

    fn execute(&self, context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
        let executable = context.git_executable().unwrap_or_else(|| Path::new("git"));
        let candidate = context.candidate().unwrap_or(CANDIDATE);

        if context.backend() != "cli" {
            return Ok(json!({
                "candidate": candidate,
                "result": "invalid-backend",
                "required_backend": "cli",
                "actual_backend": context.backend(),
            }));
        }
        if candidate != CANDIDATE {
            return Ok(json!({
                "candidate": candidate,
                "result": "unknown-candidate",
                "known_candidates": [CANDIDATE],
            }));
        }

        let version_run = run(executable, context.repo(), &["--version"]);
        let version_text = version_run.stdout_text();
        let parsed_version = version_text.as_deref().and_then(parse_git_version);
        let capability_run = run(
            executable,
            context.repo(),
            &["merge-tree", "--write-tree", "HEAD", "HEAD"],
        );

        Ok(json!({
            "candidate": CANDIDATE,
            "required_minimum_version": REQUIRED_VERSION,
            "executable": executable.to_string_lossy(),
            "version": {
                "text": version_text,
                "parsed": parsed_version,
                "probe": version_run.to_json(),
            },
            "capability_probe": capability_run.to_json(),
            "assessment": assess(parsed_version, &version_run, &capability_run),
            "production_delivery_observed": {
                "git_error_variant": Value::Null,
                "stable_error_code": Value::Null,
                "note": "P2 invokes the selected executable directly; production run_git currently has no Git-version capability gate. Tier A observes UI delivery separately."
            },
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl serde::Serialize for Version {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        [self.major, self.minor, self.patch].serialize(serializer)
    }
}

fn parse_git_version(text: &str) -> Option<Version> {
    let mut words = text.split_whitespace();
    (words.next()? == "git").then_some(())?;
    (words.next()? == "version").then_some(())?;
    let mut parts = words.next()?.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts
        .next()
        .map(|part| {
            part.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .filter(|part| !part.is_empty())
        .and_then(|part| part.parse().ok())
        .unwrap_or(0);
    Some(Version::new(major, minor, patch))
}

enum CommandObservation {
    SpawnError(String),
    Completed {
        status: Result<i32, String>,
        io: Result<(), String>,
        stdout: String,
        stderr: String,
    },
}

impl CommandObservation {
    fn stdout_text(&self) -> Option<String> {
        match self {
            Self::Completed { stdout, .. } => Some(stdout.trim().to_owned()),
            Self::SpawnError(_) => None,
        }
    }

    fn to_json(&self) -> Value {
        match self {
            Self::SpawnError(error) => json!({
                "exit_class": "spawn-error",
                "stderr_class": "spawn-failed",
                "error": error,
            }),
            Self::Completed {
                status,
                io,
                stdout,
                stderr,
            } => json!({
                "exit_class": match status {
                    Ok(0) => "success",
                    Ok(_) => "nonzero-exit",
                    Err(_) => "deadline-or-wait-error",
                },
                "status": status.as_ref().ok(),
                "wait_error": status.as_ref().err(),
                "io_error": io.as_ref().err(),
                "stderr_class": classify_stderr(stderr),
                "stdout": stdout,
                "stderr": stderr,
            }),
        }
    }
}

fn run(executable: &Path, repo: &Path, args: &[&str]) -> CommandObservation {
    let mut command = Command::new(executable);
    command.current_dir(repo).args(args);
    match run_child(&mut command, PROBE_TIMEOUT, None) {
        Err(error) => CommandObservation::SpawnError(error.to_string()),
        Ok(run) => {
            let stdout = run.stdout_lossy();
            let stderr = run.stderr_lossy();
            CommandObservation::Completed {
                status: run.status.map_err(|error| error.to_string()),
                io: run.io.map_err(|error| error.to_string()),
                stdout,
                stderr,
            }
        }
    }
}

fn assess(
    version: Option<Version>,
    version_run: &CommandObservation,
    capability_run: &CommandObservation,
) -> Value {
    let version_exit = version_run.to_json();
    let capability_exit = capability_run.to_json();
    let version_succeeded = version_exit["exit_class"] == "success";
    let capability_succeeded = capability_exit["exit_class"] == "success";
    let result = match version {
        None if version_exit["exit_class"] == "spawn-error" => "git-unavailable",
        None => "version-unparseable",
        Some(version) if version < REQUIRED_VERSION => "version-unsupported",
        Some(_) if capability_succeeded => "supported",
        Some(_) => "version-sufficient-command-unavailable",
    };
    json!({
        "result": result,
        "version_command_succeeded": version_succeeded,
        "capability_command_succeeded": capability_succeeded,
    })
}

fn classify_stderr(stderr: &str) -> &'static str {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("unknown option") || lower.contains("unrecognized option") {
        "unsupported-option"
    } else if lower.contains("usage:") {
        "usage"
    } else if lower.trim().is_empty() {
        "empty"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_apple_and_suffixed_git_versions() {
        assert_eq!(
            parse_git_version("git version 2.50.1 (Apple Git-155)"),
            Some(Version::new(2, 50, 1))
        );
        assert_eq!(
            parse_git_version("git version 2.38.0-rc1"),
            Some(Version::new(2, 38, 0))
        );
    }

    #[test]
    fn rejects_unrecognised_version_text() {
        assert_eq!(parse_git_version("git version two.point.three"), None);
        assert_eq!(parse_git_version("fixture git 9.9"), None);
    }

    #[test]
    fn classifies_option_failures_without_matching_localised_text() {
        assert_eq!(
            classify_stderr("error: unknown option `write-tree'"),
            "unsupported-option"
        );
        assert_eq!(classify_stderr("usage: git merge-tree"), "usage");
    }
}
