//! P3 semantic-matrix probe for #627.
use kagi_git::benchmark::{HarnessError, ProbeContext, ProbeOperation};
use kagi_git::{plan_stash_apply, plan_stash_pop, plan_stash_push, Backend, Operation};
use serde_json::{json, Value};
use std::{fs, process::Command};
pub struct SemanticMatrix;
impl ProbeOperation for SemanticMatrix {
    fn name(&self) -> &'static str {
        "semantic-matrix"
    }
    fn mutates_fixture(&self) -> bool {
        true
    }
    fn requires_pristine_copy_per_iteration(&self) -> bool {
        true
    }
    fn execute(&self, c: &ProbeContext<'_>) -> Result<Value, HarnessError> {
        let candidate = c
            .candidate()
            .ok_or_else(|| HarnessError::new("candidate is required"))?;
        if !matches!(
            candidate,
            "b8-clean-apply" | "b8-clean-pop" | "b8-conflict-apply" | "b8-conflict-pop"
        ) {
            return Err(HarnessError::new(
                "candidate must be a supported B8 stash scenario",
            ));
        }
        let p = c.repo();
        let raw = git2::Repository::open(p)?;
        let workdir = raw
            .workdir()
            .ok_or_else(|| HarnessError::new("fixture is bare"))?
            .to_path_buf();
        let changed = first_tracked_path(&raw)?;
        let mut content = fs::read(&changed).map_err(|e| HarnessError::io(&changed, e))?;
        content.extend_from_slice(b"\nB8 clean apply\n");
        fs::write(&changed, content).map_err(|e| HarnessError::io(&changed, e))?;
        let backend_kind = c.backend();
        if !matches!(backend_kind, "libgit2" | "cli") {
            return Err(HarnessError::new("backend must be libgit2 or cli"));
        }

        let (push_recovery_handles, push_evidence) = if backend_kind == "libgit2" {
            let mut raw = git2::Repository::open(p)?;
            let push_plan = plan_stash_push(&mut raw, None, false)?;
            if !push_plan.blockers.is_empty() {
                return Err(HarnessError::new(format!(
                    "stash push blocked after modifying {}: {:?}",
                    changed.display(),
                    push_plan.blockers
                )));
            }
            let mut backend = Backend::open(p)?;
            let push = backend.run_recorded(
                &Operation::StashPush {
                    message: None,
                    include_untracked: false,
                },
                &push_plan,
            );
            push.result
                .as_ref()
                .map_err(|error| HarnessError::new(error.to_string()))?;
            (
                json!(push.recording.entry().recovery.len()),
                json!({
                    "oid": push.stash.as_ref().and_then(|evidence| evidence.oid.as_deref()),
                    "verified": push.stash.as_ref().is_some_and(|evidence| evidence.verified),
                }),
            )
        } else {
            run_git(&workdir, &["stash", "push", "--quiet"])?;
            (Value::Null, json!({"oplog": "not-recorded-by-direct-cli"}))
        };

        if matches!(candidate, "b8-conflict-apply" | "b8-conflict-pop") {
            fs::write(&changed, b"B8 divergent HEAD change\n")
                .map_err(|error| HarnessError::io(&changed, error))?;
            let relative = changed
                .strip_prefix(&workdir)
                .map_err(|error| HarnessError::new(error.to_string()))?;
            run_git(&workdir, &["add", &relative.to_string_lossy()])?;
            run_git(
                &workdir,
                &["commit", "--no-gpg-sign", "-m", "B8 divergent HEAD change"],
            )?;
        }

        let (action_error, action_recovery_handles, action_plan) = if backend_kind == "libgit2" {
            let mut backend = Backend::open(p)?;
            let mut raw = git2::Repository::open(p)?;
            let (plan, action) = match candidate {
                "b8-clean-apply" | "b8-conflict-apply" => (
                    plan_stash_apply(&mut raw, 0)?,
                    Operation::StashApply { index: 0 },
                ),
                "b8-clean-pop" | "b8-conflict-pop" => (
                    plan_stash_pop(&mut raw, 0)?,
                    Operation::StashPop { index: 0 },
                ),
                _ => unreachable!("candidate was validated"),
            };
            let report = backend.run_recorded(&action, &plan);
            (
                report.result.as_ref().err().map(ToString::to_string),
                json!(report.recording.entry().recovery.len()),
                json!({
                    "blockers": plan.blockers.iter().map(|note| note.message_en()).collect::<Vec<_>>(),
                    "warnings": plan.warnings.iter().map(|note| note.message_en()).collect::<Vec<_>>(),
                    "stash_oid": report.stash.as_ref().and_then(|evidence| evidence.oid.as_deref()),
                    "conflicts": report.stash.as_ref().map(|evidence| &evidence.conflicts),
                }),
            )
        } else {
            let command = match candidate {
                "b8-clean-apply" | "b8-conflict-apply" => ["stash", "apply", "stash@{0}"],
                "b8-clean-pop" | "b8-conflict-pop" => ["stash", "pop", "stash@{0}"],
                _ => unreachable!("candidate was validated"),
            };
            let output = run_git_output(&workdir, &command)?;
            let action_error = (!output.status.success()).then(|| {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                format!("{}{}", stderr, stdout).trim().to_string()
            });
            (
                action_error,
                Value::Null,
                json!({"oplog": "not-recorded-by-direct-cli"}),
            )
        };
        Ok(json!({
            "backend": backend_kind,
            "scenario": candidate,
            "action_error": action_error,
            "action": action_plan,
            "stash_entries_after_action": stash_count(p)?,
            "push_recovery_handles": push_recovery_handles,
            "action_recovery_handles": action_recovery_handles,
            "push": push_evidence,
        }))
    }
}

fn first_tracked_path(repo: &git2::Repository) -> Result<std::path::PathBuf, HarnessError> {
    let index = repo.index()?;
    let entry = index
        .iter()
        .next()
        .ok_or_else(|| HarnessError::new("fixture has no tracked file"))?;
    let path = std::str::from_utf8(&entry.path)
        .map_err(|error| HarnessError::new(format!("fixture path is not UTF-8: {error}")))?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| HarnessError::new("fixture is bare"))?;
    Ok(workdir.join(path))
}

fn run_git(repo: &std::path::Path, args: &[&str]) -> Result<(), HarnessError> {
    let output = run_git_output(repo, args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(HarnessError::new(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn run_git_output(
    repo: &std::path::Path,
    args: &[&str],
) -> Result<std::process::Output, HarnessError> {
    Command::new("git")
        .args(["-C", &repo.to_string_lossy()])
        .args(args)
        .output()
        .map_err(|error| HarnessError::new(error.to_string()))
}

fn stash_count(p: &std::path::Path) -> Result<usize, HarnessError> {
    let mut count = 0;
    let mut repo = git2::Repository::open(p)?;
    repo.stash_foreach(|_, _, _| {
        count += 1;
        true
    })
    .map_err(|error| HarnessError::new(error.to_string()))?;
    Ok(count)
}
