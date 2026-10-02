//! Clone a GitHub repository into a new folder (#923, ADR-0219).
//!
//! The one write that starts with no repository, so it has no session, lease
//! or `Backend`: the plan is about a source (`[host/]owner/repo`) and a
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
use kagi_domain::remote::shell_quote;

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
    /// `[host/]owner/repo`, as `gh repo clone` takes it.
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
        worktree_digest: None,
        destructive: false,
        // Shell-quoted so a copied command is the same command for a path
        // with spaces or quotes (#926 review).
        equivalent_command: Some(format!(
            "gh repo clone {} {}",
            shell_quote(&request.source),
            shell_quote(&request.dest.to_string_lossy())
        )),
        preview_files: Vec::new(),
        preview_commits: Vec::new(),
    }
}

/// The plan's checks again, immediately before the clone runs: the
/// destination may have been created or filled since the card was shown.
pub fn preflight_clone(request: &CloneRequest) -> Result<(), GitError> {
    match clone_blockers(request).into_iter().next() {
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
    let stage = match preflight_clone(request) {
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
    if !source_is_valid(&request.source) {
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

/// `owner/repo` or `host/owner/repo`, each part a plain name: nothing `gh`
/// could read as a flag or a path.
fn source_is_valid(source: &str) -> bool {
    let parts: Vec<&str> = source.split('/').collect();
    let names_ok = |names: &[&str]| {
        names.iter().all(|name| {
            !name.is_empty()
                && !name.starts_with(['-', '.'])
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
    };
    match parts.as_slice() {
        [owner, repo] => names_ok(&[owner, repo]),
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

/// `host/owner/repo`, lower-cased like [`crate::backend::remote_ref::repo_identity`];
/// `gh`'s default host when the source names none.
fn source_identity(source: &str) -> String {
    let identity = if source.matches('/').count() == 1 {
        format!("github.com/{source}")
    } else {
        source.to_string()
    };
    identity.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_gh_can_take_and_nothing_else() {
        for ok in [
            "acme/widgets",
            "acme/widgets.rs",
            "github.com/acme/widgets",
            "ghe.example.com:8443/acme/my_repo",
        ] {
            assert!(source_is_valid(ok), "{ok}");
        }
        for bad in [
            "",
            "widgets",
            "-acme/widgets",
            "acme/--upload-pack=x",
            "acme/../widgets",
            "a/b/c/d",
            "acme/wid gets",
            "acme/.hidden",
        ] {
            assert!(!source_is_valid(bad), "{bad}");
        }
    }

    #[test]
    fn identity_defaults_to_github_and_lower_cases() {
        assert_eq!(source_identity("Acme/Widgets"), "github.com/acme/widgets");
        assert_eq!(
            source_identity("GHE.example.com/acme/widgets"),
            "ghe.example.com/acme/widgets"
        );
    }

    #[test]
    fn the_shown_command_quotes_a_destination_with_spaces_and_quotes() {
        let request = CloneRequest {
            source: "acme/widgets".to_string(),
            dest: PathBuf::from("/Users/me/My Projects/it's here"),
            is_fork: false,
        };
        assert_eq!(
            plan_clone(&request).equivalent_command.as_deref(),
            Some(r"gh repo clone 'acme/widgets' '/Users/me/My Projects/it'\''s here'")
        );
    }
}
