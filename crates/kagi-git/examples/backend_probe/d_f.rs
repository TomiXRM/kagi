//! P5 execution-path and mixed-backend probe for #627.
//!
//! P0 owns the root dispatcher; this registered module owns only P5 operation
//! behaviour.

use kagi_git::benchmark::{HarnessError, ProbeContext, ProbeOperation};
use kagi_git::{run_git, Backend};
use serde_json::{json, Value};

use std::{
    fs,
    path::{Path, PathBuf},
};

use kagi_git::Operation;

pub struct ExecutionMixed;

impl ProbeOperation for ExecutionMixed {
    fn name(&self) -> &'static str {
        "execution-mixed"
    }

    fn mutates_fixture(&self) -> bool {
        false
    }

    fn prepare_series(&self, context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
        if context.candidate() != Some("d1-working-tree-status") {
            return Err(HarnessError::new(
                "candidate must be d1-working-tree-status",
            ));
        }
        let index_path = context.repo().join(".git/index");
        let index_mtime_before = fs::metadata(&index_path)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        let output = run_git(
            context.repo(),
            &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
        )
        .map_err(|error| HarnessError::new(error.to_string()))?;
        if output.status != 0 {
            return Err(HarnessError::new(format!(
                "index stat-cache prime exited {}: {}",
                output.status,
                output.stderr.trim()
            )));
        }
        let index_mtime_after = fs::metadata(&index_path)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        Ok(json!({
            "index_mtime_changed": index_mtime_before != index_mtime_after,
            "index_stat_cache_primed": true,
            "priming_executor": "run_git status --porcelain=v2 -z",
            "repo_dynamic_settings_disabled": output.repo_dynamic_settings_disabled,
        }))
    }

    fn execute(&self, context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
        if context.candidate() != Some("d1-working-tree-status") {
            return Err(HarnessError::new(
                "candidate must be d1-working-tree-status",
            ));
        }

        match context.backend() {
            "libgit2" => {
                let backend = Backend::open(context.repo())
                    .map_err(|error| HarnessError::new(error.to_string()))?;
                let status = backend
                    .working_tree_status()
                    .map_err(|error| HarnessError::new(error.to_string()))?;
                Ok(json!({
                    "backend": "libgit2",
                    "executor": "Backend::working_tree_status",
                    "staged": status.staged.len(),
                    "unstaged": status.unstaged.len(),
                    "untracked": status.untracked.len(),
                    "conflicted": status.conflicted.len(),
                    "porcelain_entries": Value::Null,
                    "repo_dynamic_settings_disabled": Value::Null,
                }))
            }
            "cli" => {
                let output = run_git(
                    context.repo(),
                    &[
                        "--no-optional-locks",
                        "status",
                        "--porcelain=v2",
                        "-z",
                        "--untracked-files=all",
                    ],
                )
                .map_err(|error| HarnessError::new(error.to_string()))?;
                if output.status != 0 {
                    return Err(HarnessError::new(format!(
                        "git status exited {}: {}",
                        output.status,
                        output.stderr.trim()
                    )));
                }
                let counts = parse_porcelain_v2(&output.stdout);
                Ok(json!({
                    "backend": "cli",
                    "executor": "run_git --no-optional-locks status --porcelain=v2 -z",
                    "staged": counts.staged,
                    "unstaged": counts.unstaged,
                    "untracked": counts.untracked,
                    "conflicted": counts.conflicted,
                    "porcelain_entries": counts.entries,
                    "repo_dynamic_settings_disabled": output.repo_dynamic_settings_disabled,
                }))
            }
            other => Err(HarnessError::new(format!(
                "backend must be libgit2 or cli, got {other}"
            ))),
        }
    }
}

/// F's deterministic mixed-backend stash derivation.
///
/// Setup runs inside `execute`; F records divergence evidence, not timing.
/// Kagi's libgit2 action follows plan, preflight, execute, verify, and oplog.
pub struct MixedStash;

impl ProbeOperation for MixedStash {
    fn name(&self) -> &'static str {
        "mixed-stash"
    }

    fn mutates_fixture(&self) -> bool {
        true
    }

    fn execute(&self, context: &ProbeContext<'_>) -> Result<Value, HarnessError> {
        let candidate = context
            .candidate()
            .ok_or_else(|| HarnessError::new("candidate is required"))?;
        let operation = match candidate {
            "f-apply" => Operation::StashApply { index: 0 },
            "f-pop" => Operation::StashPop { index: 0 },
            "f-drop" => Operation::StashDrop { index: 0 },
            _ => {
                return Err(HarnessError::new(
                    "candidate must be f-apply, f-pop, or f-drop",
                ));
            }
        };
        if context.backend() != "libgit2" {
            return Err(HarnessError::new("F mixed action backend must be libgit2"));
        }

        let (workdir, relative) = first_tracked_path(context.repo())?;
        let tracked = workdir.join(&relative);
        let original = fs::read(&tracked).map_err(|error| HarnessError::io(&tracked, error))?;
        let mut staged = original.clone();
        staged.extend_from_slice(b"\nP5 staged stash content\n");
        fs::write(&tracked, staged).map_err(|error| HarnessError::io(&tracked, error))?;
        run_git_success(
            context.repo(),
            &["add", relative.to_string_lossy().as_ref()],
        )?;
        let mut unstaged = original;
        unstaged.extend_from_slice(b"\nP5 unstaged stash content\n");
        fs::write(&tracked, unstaged).map_err(|error| HarnessError::io(&tracked, error))?;
        run_git_success(
            context.repo(),
            &[
                "stash",
                "push",
                "--include-untracked",
                "-m",
                "p5-mixed-stash",
            ],
        )?;

        let mut backend =
            Backend::open(context.repo()).map_err(|error| HarnessError::new(error.to_string()))?;
        let plan = backend
            .plan(&operation)
            .map_err(|error| HarnessError::new(error.to_string()))?;
        let report = backend.run_recorded(&operation, &plan);
        let action_error = report.result.as_ref().err().map(ToString::to_string);
        let status = backend
            .working_tree_status()
            .map_err(|error| HarnessError::new(error.to_string()))?;
        Ok(json!({
            "backend": "libgit2",
            "setup_executor": "run_git stash push --include-untracked",
            "wall_ns_includes_setup": true,
            "candidate": candidate,
            "plan_blockers": plan.blockers.iter().map(|note| note.message_en()).collect::<Vec<_>>(),
            "plan_warnings": plan.warnings.iter().map(|note| note.message_en()).collect::<Vec<_>>(),
            "action_error": action_error,
            "oplog_recovery_handles": report.recording.entry().recovery.len(),
            "verified": report.stash.as_ref().is_some_and(|evidence| evidence.verified),
            "staged": status.staged.len(),
            "unstaged": status.unstaged.len(),
            "untracked": status.untracked.len(),
            "conflicted": status.conflicted.len(),
        }))
    }
}

fn first_tracked_path(repo_path: &Path) -> Result<(PathBuf, PathBuf), HarnessError> {
    let repo =
        git2::Repository::open(repo_path).map_err(|error| HarnessError::new(error.to_string()))?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| HarnessError::new("fixture is bare"))?
        .to_path_buf();
    let index = repo
        .index()
        .map_err(|error| HarnessError::new(error.to_string()))?;
    let entry = index
        .iter()
        .next()
        .ok_or_else(|| HarnessError::new("fixture has no tracked file"))?;
    let relative = PathBuf::from(
        std::str::from_utf8(&entry.path)
            .map_err(|error| HarnessError::new(format!("fixture path is not UTF-8: {error}")))?,
    );
    Ok((workdir, relative))
}
fn run_git_success(repo: &Path, args: &[&str]) -> Result<(), HarnessError> {
    let output = run_git(repo, args).map_err(|error| HarnessError::new(error.to_string()))?;
    if output.status == 0 {
        Ok(())
    } else {
        Err(HarnessError::new(format!(
            "git {} exited {}: {}",
            args.join(" "),
            output.status,
            output.stderr.trim()
        )))
    }
}

#[derive(Default)]
struct PorcelainCounts {
    staged: usize,
    unstaged: usize,
    untracked: usize,
    conflicted: usize,
    entries: usize,
}

fn parse_porcelain_v2(output: &str) -> PorcelainCounts {
    let mut counts = PorcelainCounts::default();
    for record in output.split('\0').filter(|record| !record.is_empty()) {
        counts.entries += 1;
        match record.as_bytes().first() {
            Some(b'1' | b'2') => {
                let xy = record
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .as_bytes();
                counts.staged += usize::from(xy.first().is_some_and(|code| *code != b'.'));
                counts.unstaged += usize::from(xy.get(1).is_some_and(|code| *code != b'.'));
            }
            Some(b'u') => counts.conflicted += 1,
            Some(b'?') => counts.untracked += 1,
            _ => {}
        }
    }
    counts
}
