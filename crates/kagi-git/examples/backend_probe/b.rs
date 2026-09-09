//! P3 semantic-matrix probe for #627.
use kagi_git::benchmark::{HarnessError, ProbeContext, ProbeOperation};
use kagi_git::{plan_stash_apply, plan_stash_pop, plan_stash_push, Backend, Operation};
use serde_json::{json, Value};
use std::fs;
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
        if !matches!(candidate, "b8-clean-apply" | "b8-clean-pop") {
            return Err(HarnessError::new(
                "candidate must be b8-clean-apply or b8-clean-pop",
            ));
        }
        let p = c.repo();
        let raw = git2::Repository::open(p)?;
        let changed = first_tracked_path(&raw)?;
        let mut content = fs::read(&changed).map_err(|e| HarnessError::io(&changed, e))?;
        content.extend_from_slice(b"\nB8 clean apply\n");
        fs::write(&changed, content).map_err(|e| HarnessError::io(&changed, e))?;
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
        let mut raw = git2::Repository::open(p)?;
        let (action_plan, action) = match candidate {
            "b8-clean-apply" => (
                plan_stash_apply(&mut raw, 0)?,
                Operation::StashApply { index: 0 },
            ),
            "b8-clean-pop" => (
                plan_stash_pop(&mut raw, 0)?,
                Operation::StashPop { index: 0 },
            ),
            _ => unreachable!("candidate was validated"),
        };
        let action_report = backend.run_recorded(&action, &action_plan);
        action_report
            .result
            .as_ref()
            .map_err(|error| HarnessError::new(error.to_string()))?;
        Ok(json!({
            "scenario": candidate,
            "stash_entries_after_action": stash_count(p),
            "push_recovery_handles": push.recording.entry().recovery.len(),
            "action_recovery_handles": action_report.recording.entry().recovery.len(),
            "push_stash_evidence": format!("{:?}", push.stash),
            "action_stash_evidence": format!("{:?}", action_report.stash),
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
fn stash_count(p: &std::path::Path) -> usize {
    let mut n = 0;
    git2::Repository::open(p)
        .unwrap()
        .stash_foreach(|_, _, _| {
            n += 1;
            true
        })
        .unwrap();
    n
}
