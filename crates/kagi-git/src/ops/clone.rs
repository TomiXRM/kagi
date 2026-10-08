//! Clone a GitHub repository into a new folder (#923, ADR-0219).
//!
//! The one write that starts with no repository, so it has no session, lease
//! or `Backend`: the plan is about a source (`host/owner/repo`) and a
//! destination folder, and its only effect is that folder.
//!
//! `plan_clone` → (confirm) → `preflight_clone` → `execute_clone` →
//! `verify_clone` → oplog, the receipt keyed by the destination path.
//!
//! - **Executor**: `gh repo clone`, so a private or Enterprise repository is
//!   cloned with the same `gh` login that listed it, even when `git` itself has
//!   no credentials.
//! - **Never overwrites, never deletes**: a destination that exists and is not
//!   an empty folder is refused, and whatever a failed or cut-short clone left
//!   behind stays where it is, named in the receipt.
//! - **Deadline**: [`CLONE_TIMEOUT`]. On expiry [`crate::proc::run_child`]
//!   stops the process group Kagi started for this clone — `gh` and the `git`
//!   it runs — and nothing else; the outcome is `Unknown`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kagi_domain::head::Head;
use kagi_domain::plan::{OperationPlan, StateSummary};
use kagi_domain::plan_note::{CloneNote, CloneTitle, PlanDisposition, PlanNote, PlanTitle};

use crate::backend::recording::RunReport;
use crate::oplog::{OpLogEntry, OpOutcome};
use crate::GitError;

/// How long a clone may run before Kagi stops it. Generous: a large
/// repository over a slow link takes minutes, and the usual 60 s network
/// deadline would cut ordinary clones short.
pub const CLONE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// What to clone, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneRequest {
    /// `host/owner/repo`, as `gh repo clone` takes it. The host is required:
    /// without it `gh` clones from its configured host (`GH_HOST`), which the
    /// plan and the verify step could not name (#926 review).
    pub source: String,
    /// The folder to create. Absolute; it must not exist yet, or be empty.
    pub dest: PathBuf,
    /// A fork: `gh` also adds an `upstream` remote, which the plan says.
    pub is_fork: bool,
}

/// `gh repo clone <source> <dest>`. The source is validated by the plan and
/// the destination is absolute, so neither can be read as a flag.
pub fn clone_args(request: &CloneRequest) -> Vec<OsString> {
    vec![
        "repo".into(),
        "clone".into(),
        request.source.clone().into(),
        request.dest.clone().into_os_string(),
    ]
}

/// Plan the clone: the source, the destination, and every reason it cannot
/// run. Reads the destination on disk; writes nothing.
pub fn plan_clone(request: &CloneRequest) -> OperationPlan {
    let blockers = clone_blockers(request);
    // Only for a clone that can run: a blocked plan's paths may not be
    // faithful (a non-UTF-8 destination renders lossily), and there is
    // nothing to copy (#926 review).
    let command = blockers
        .is_empty()
        .then(|| equivalent_command(request))
        .flatten();
    let dest = request.dest.display().to_string();
    let mut warnings = Vec::new();
    if request.is_fork {
        warnings.push(PlanNote::Clone(CloneNote::ForkAddsUpstream));
    }
    OperationPlan {
        disposition: PlanDisposition::for_blockers(&blockers),
        title: PlanTitle::Clone(CloneTitle::Clone {
            source: request.source.clone(),
        }),
        current: StateSummary {
            head: request.source.clone(),
            dirty: "not cloned".into(),
        },
        predicted: StateSummary {
            head: dest,
            dirty: format!("clone of {}", request.source),
        },
        warnings,
        blockers,
        recovery: None,
        head_at_plan: Head::Unborn {
            branch: String::new(),
        },
        stash_count_at_plan: 0,
        stash_identity: None,
        pull_identity: None,
        worktree_digest: None,
        destructive: false,
        equivalent_command: command,
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
    }
}

/// The command the card shows, POSIX-shell-quoted so a copied command is the
/// same command for a path with spaces or quotes (#926 review).
#[cfg(not(windows))]
fn equivalent_command(request: &CloneRequest) -> Option<String> {
    use kagi_domain::remote::shell_quote;
    Some(format!(
        "gh repo clone {} {}",
        shell_quote(&request.source),
        shell_quote(&request.dest.to_string_lossy())
    ))
}

/// None on Windows: POSIX single quotes are not quoting to `cmd.exe`, and a
/// command Kagi cannot quote faithfully is not shown at all (#926 review).
#[cfg(windows)]
fn equivalent_command(_request: &CloneRequest) -> Option<String> {
    None
}

/// Immediately before the clone runs: the request must be the one the
/// approved `plan` was made for (the same source, destination and fork
/// warning — #926 review), that plan must have had no blocker, and the
/// plan's checks must still hold (the destination may have been created or
/// filled since the card was shown).
pub fn preflight_clone(request: &CloneRequest, plan: &OperationPlan) -> Result<(), GitError> {
    if let Some(note) = plan.blockers.first() {
        return Err(GitError::Blocked(Box::new(note.clone())));
    }
    let expected = plan_clone(request);
    let same_request = plan.title == expected.title
        && plan.current == expected.current
        && plan.predicted == expected.predicted
        // The plan had no blocker (checked above), so it carries the command
        // for its request; the recomputed plan may be blocked by now.
        && plan.equivalent_command == equivalent_command(request)
        && plan.warnings == expected.warnings;
    if !same_request {
        return Err(GitError::Blocked(Box::new(PlanNote::Clone(
            CloneNote::PlanMismatch {
                source: request.source.clone(),
                path: request.dest.display().to_string(),
            },
        ))));
    }
    match expected.blockers.into_iter().next() {
        Some(note) => Err(GitError::Blocked(Box::new(note))),
        None => Ok(()),
    }
}

/// Clone, verify, and record. The receipt (`op = "clone"`, repo = the
/// destination path) is written whatever happens, refusals included.
pub fn execute_clone(request: &CloneRequest, plan: &OperationPlan) -> RunReport {
    execute_clone_within(request, plan, CLONE_TIMEOUT)
}

/// [`execute_clone`] with an explicit deadline — the integration test of the
/// timeout path cannot wait 30 minutes.
#[doc(hidden)]
pub fn execute_clone_within(
    request: &CloneRequest,
    plan: &OperationPlan,
    timeout: Duration,
) -> RunReport {
    let stage = match preflight_clone(request, plan) {
        Err(GitError::Blocked(note)) => Stage::Refused(*note),
        Err(error) => Stage::Failed(error.to_string()),
        Ok(()) => match clone_transport(request, timeout) {
            Err(stage) => stage,
            Ok(()) => match verify_clone(&request.dest, &request.source) {
                Ok(()) => Stage::Cloned,
                Err(reason) => Stage::Unverified(reason),
            },
        },
    };
    record_clone(request, plan, stage)
}

/// Is the destination a repository cloned from `source`? It must open, and
/// when its `origin` names a GitHub-style repository that must be `source`.
/// An `origin` whose host cannot be read (an ssh alias) is not a mismatch.
pub fn verify_clone(dest: &Path, source: &str) -> Result<(), String> {
    crate::open_repository(dest)
        .map_err(|error| format!("{} does not open as a repository: {error}", dest.display()))?;
    let repo = git2::Repository::open(dest)
        .map_err(|error| format!("{} does not open: {error}", dest.display()))?;
    let origin = repo
        .find_remote("origin")
        .map_err(|_| format!("{} has no 'origin' remote", dest.display()))?;
    let Some(identity) = origin
        .url()
        .ok()
        .and_then(crate::backend::remote_ref::repo_identity)
    else {
        return Ok(());
    };
    let expected = source_identity(source);
    if identity == expected {
        Ok(())
    } else {
        Err(format!("origin points to {identity}, not {expected}"))
    }
}

/// How the clone ended, before it is recorded.
enum Stage {
    Cloned,
    Refused(PlanNote),
    Failed(String),
    Unknown(crate::Termination),
    Unverified(String),
}

fn record_clone(request: &CloneRequest, plan: &OperationPlan, stage: Stage) -> RunReport {
    let dest = request.dest.display().to_string();
    let left = || leftover(&request.dest);
    let (outcome, result) = match stage {
        Stage::Cloned => (
            OpOutcome::Success {
                after: StateSummary {
                    head: dest.clone(),
                    dirty: format!("cloned from {}", request.source),
                },
            },
            Ok(crate::OperationOutcome::Clone {
                source: request.source.clone(),
                path: dest.clone(),
            }),
        ),
        Stage::Refused(note) => (
            OpOutcome::Refused {
                blockers: vec![note.message_en()],
            },
            Err(GitError::Blocked(Box::new(note))),
        ),
        Stage::Failed(error) => {
            let error = format!("{error}{}", left());
            (
                OpOutcome::Failed {
                    error: error.clone(),
                },
                Err(GitError::Other(error)),
            )
        }
        Stage::Unknown(termination) => {
            let evidence = format!(
                "{}; the clone may be incomplete{}",
                termination.reason(),
                left()
            );
            (
                OpOutcome::Unknown {
                    after: StateSummary {
                        head: dest.clone(),
                        dirty: "clone unconfirmed".into(),
                    },
                    evidence,
                },
                Err(GitError::TerminationUnknown(termination)),
            )
        }
        Stage::Unverified(reason) => {
            let error = format!("cloned, but {reason}");
            (
                OpOutcome::Partial {
                    after: StateSummary {
                        head: dest.clone(),
                        dirty: format!("cloned from {}", request.source),
                    },
                    error: error.clone(),
                },
                Err(GitError::Other(error)),
            )
        }
    };
    let entry = OpLogEntry::new("clone", dest, plan.current.clone(), outcome);
    RunReport {
        result,
        recording: crate::backend::recording::finalize(entry),
        stash: None,
    }
}

/// What a failed or cut-short clone left at the destination, for the receipt.
/// Kagi never deletes it: the user decides.
fn leftover(dest: &Path) -> String {
    let has_entries = std::fs::read_dir(dest).is_ok_and(|mut entries| entries.next().is_some());
    if has_entries {
        format!(
            "; {} was left as it is — Kagi did not delete it",
            dest.display()
        )
    } else {
        String::new()
    }
}

fn clone_transport(request: &CloneRequest, timeout: Duration) -> Result<(), Stage> {
    let mut cmd = crate::cli::gh_command();
    cmd.args(clone_args(request))
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    if let Some(parent) = request.dest.parent() {
        cmd.current_dir(parent);
    }
    let out = crate::proc::run_child(&mut cmd, timeout, None)
        .map_err(|error| Stage::Failed(format!("gh could not be started: {error}")))?;
    let status = match &out.status {
        Ok(status) => *status,
        Err(stop) => {
            return Err(Stage::Unknown(crate::Termination::from_run(
                format!("gh repo clone {stop}"),
                &out,
            )))
        }
    };
    if status < 0 {
        return Err(Stage::Unknown(crate::Termination::from_run(
            "gh repo clone terminated without an exit code",
            &out,
        )));
    }
    if let Err(io) = &out.io {
        return Err(Stage::Unknown(crate::Termination::from_run(
            format!("gh repo clone exited with status {status} but {io}"),
            &out,
        )));
    }
    // An exit status is not the end of the clone: a hook or helper `gh`
    // started may still be writing into the destination. Only a process
    // group proven empty lets the outcome be read as finished (#926 review).
    if !out.group_stopped {
        return Err(Stage::Unknown(crate::Termination::from_run(
            format!(
                "gh repo clone exited with status {status}, but a process it started \
                 may still be running"
            ),
            &out,
        )));
    }
    if status == 0 {
        return Ok(());
    }
    let stderr = out.stderr_lossy().trim().to_string();
    Err(Stage::Failed(if stderr.is_empty() {
        format!("gh repo clone exited with status {status}")
    } else {
        stderr
    }))
}

fn clone_blockers(request: &CloneRequest) -> Vec<PlanNote> {
    let mut blockers = Vec::new();
    if source_lacks_host(&request.source) {
        blockers.push(PlanNote::Clone(CloneNote::SourceWithoutHost {
            source: request.source.clone(),
        }));
    } else if !source_is_valid(&request.source) {
        blockers.push(PlanNote::Clone(CloneNote::SourceInvalid {
            source: request.source.clone(),
        }));
    }
    if let Some(note) = destination_blocker(&request.dest) {
        blockers.push(PlanNote::Clone(note));
    }
    blockers
}

/// Why the destination cannot be cloned into, if it cannot.
fn destination_blocker(dest: &Path) -> Option<CloneNote> {
    let path = dest.display().to_string();
    if !dest.is_absolute() {
        return Some(CloneNote::DestinationNotAbsolute { path });
    }
    // Refused here so that, from the plan on, the destination's string (card,
    // command, receipt, plan comparison) and the path are one-to-one: a lossy
    // rendering could name another folder (#926 review).
    if dest.to_str().is_none() {
        return Some(CloneNote::DestinationNotUtf8 { path });
    }
    // `symlink_metadata`: a symlink is "something already there", even one
    // that points at an empty folder.
    match std::fs::symlink_metadata(dest) {
        Ok(meta) if meta.is_dir() => match std::fs::read_dir(dest) {
            Ok(mut entries) => entries
                .next()
                .map(|_| CloneNote::DestinationNotEmpty { path }),
            Err(error) => Some(CloneNote::DestinationUnreadable {
                path,
                error: error.to_string(),
            }),
        },
        Ok(_) => Some(CloneNote::DestinationNotEmpty { path }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match dest.parent().filter(|parent| parent.is_dir()) {
                Some(_) => None,
                None => Some(CloneNote::ParentMissing {
                    path: dest
                        .parent()
                        .map(|parent| parent.display().to_string())
                        .unwrap_or_default(),
                }),
            }
        }
        Err(error) => Some(CloneNote::DestinationUnreadable {
            path,
            error: error.to_string(),
        }),
    }
}

/// Each part a plain name, and not `.` or `..`, which a path would read as a
/// directory step. A leading dot or dash is otherwise a real name (`.github`,
/// `-tools`): `gh` gets `host/owner/repo` as one argument that starts with the
/// host, so only the host's first character could make it read as a flag
/// (checked in [`source_is_valid`]).
fn names_ok(names: &[&str]) -> bool {
    names.iter().all(|name| {
        !name.is_empty()
            && !matches!(*name, "." | "..")
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    })
}

/// A well-formed `owner/repo` that names no host.
fn source_lacks_host(source: &str) -> bool {
    matches!(source.split('/').collect::<Vec<_>>().as_slice(), [owner, repo] if names_ok(&[owner, repo]))
}

/// `host/owner/repo`, each part a plain name.
fn source_is_valid(source: &str) -> bool {
    match source.split('/').collect::<Vec<_>>().as_slice() {
        [host, owner, repo] => {
            names_ok(&[owner, repo])
                && !host.is_empty()
                && !host.starts_with('-')
                && host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':'))
        }
        _ => false,
    }
}

/// `host/owner/repo`, lower-cased like [`crate::backend::remote_ref::repo_identity`].
fn source_identity(source: &str) -> String {
    source.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_gh_can_take_and_nothing_else() {
        for ok in [
            "github.com/acme/widgets",
            "github.com/acme/widgets.rs",
            "github.com/acme/.github",
            "github.com/acme/.dotfiles",
            "github.com/acme/-tools",
            "github.com/-acme/widgets",
            "ghe.example.com:8443/acme/my_repo",
        ] {
            assert!(source_is_valid(ok), "{ok}");
        }
        for bad in [
            "",
            "widgets",
            "acme/widgets",
            "-host/acme/widgets",
            "github.com/acme/--upload-pack=x",
            "github.com/acme/../widgets",
            "github.com/acme/..",
            "github.com/acme/.",
            "github.com/../widgets",
            "a/b/c/d",
            "github.com/acme/wid gets",
        ] {
            assert!(!source_is_valid(bad), "{bad}");
        }
    }

    /// A blocked plan offers no command to copy: its destination may only be
    /// shown lossily (a non-UTF-8 name), so the command would name another
    /// folder.
    #[cfg(unix)]
    #[test]
    fn a_blocked_plan_shows_no_command() {
        use std::os::unix::ffi::OsStrExt;
        let request = CloneRequest {
            source: "github.com/acme/widgets".to_string(),
            dest: PathBuf::from(std::ffi::OsStr::from_bytes(b"/tmp/caf\xe9")),
            is_fork: false,
        };
        let plan = plan_clone(&request);
        assert!(!plan.blockers.is_empty());
        assert_eq!(plan.equivalent_command, None);
    }

    /// `gh` would clone a host-less source from `GH_HOST`, which the plan and
    /// verify cannot name: refused with its own reason, never guessed.
    #[test]
    fn a_source_without_host_is_refused_by_the_plan() {
        let request = CloneRequest {
            source: "acme/widgets".to_string(),
            dest: PathBuf::from("/nonexistent-kagi-parent/widgets"),
            is_fork: false,
        };
        let plan = plan_clone(&request);
        assert_eq!(
            plan.blockers.first(),
            Some(&PlanNote::Clone(CloneNote::SourceWithoutHost {
                source: "acme/widgets".to_string()
            }))
        );
    }

    #[test]
    fn identity_keeps_the_host_and_lower_cases() {
        assert_eq!(
            source_identity("GHE.example.com/Acme/Widgets"),
            "ghe.example.com/acme/widgets"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn the_shown_command_quotes_a_destination_with_spaces_and_quotes() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("My Projects");
        std::fs::create_dir(&parent).unwrap();
        let request = CloneRequest {
            source: "github.com/acme/widgets".to_string(),
            dest: parent.join("it's here"),
            is_fork: false,
        };
        assert_eq!(
            plan_clone(&request).equivalent_command,
            Some(format!(
                r"gh repo clone 'github.com/acme/widgets' '{}/My Projects/it'\''s here'",
                root.path().display()
            ))
        );
    }

    #[cfg(windows)]
    #[test]
    fn no_command_is_shown_on_windows() {
        let request = CloneRequest {
            source: "github.com/acme/widgets".to_string(),
            dest: PathBuf::from(r"C:\Users\me\My Projects\widgets"),
            is_fork: false,
        };
        assert_eq!(plan_clone(&request).equivalent_command, None);
    }
}
