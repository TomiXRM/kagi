//! P3 semantic-matrix probe for #627.
use kagi_git::benchmark::{HarnessError, ProbeContext, ProbeOperation};
use kagi_git::{plan_stash_apply, plan_stash_push, Backend, Operation};
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
        if c.candidate() != Some("b8-clean-apply") {
            return Err(HarnessError::new("candidate must be b8-clean-apply"));
        }
        let p = c.repo();
        let changed = p.join("file-000000.bin");
        fs::write(&changed, b"B8 clean apply\n").map_err(|e| HarnessError::io(&changed, e))?;
        let mut raw = git2::Repository::open(p)?;
        let push_plan = plan_stash_push(&mut raw, None, false)?;
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
        let apply_plan = plan_stash_apply(&mut raw, 0)?;
        let apply = backend.run_recorded(&Operation::StashApply { index: 0 }, &apply_plan);
        apply
            .result
            .as_ref()
            .map_err(|error| HarnessError::new(error.to_string()))?;
        Ok(
            json!({"scenario":"b8-clean-apply","stash_entries_after_apply":stash_count(p),"push_recovery_handles":push.recording.entry().recovery.len(),"apply_recovery_handles":apply.recording.entry().recovery.len(),"push_stash_evidence":format!("{:?}",push.stash),"apply_stash_evidence":format!("{:?}",apply.stash)}),
        )
    }
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
