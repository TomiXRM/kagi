//! P5 execution-path and mixed-backend probe for #627.
//!
//! This module is intentionally unregistered. The P0 integration owner owns the
//! root dispatcher and registers it after reviewing the probe contract.

use kagi_git::benchmark::{HarnessError, ProbeContext, ProbeOperation};
use kagi_git::{run_git, Backend};
use serde_json::{json, Value};

pub struct ExecutionMixed;

impl ProbeOperation for ExecutionMixed {
    fn name(&self) -> &'static str {
        "execution-mixed"
    }

    fn mutates_fixture(&self) -> bool {
        false
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
                }))
            }
            "cli" => {
                let output = run_git(
                    context.repo(),
                    &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
                )
                .map_err(|error| HarnessError::new(error.to_string()))?;
                if output.status != 0 {
                    return Err(HarnessError::new(format!(
                        "git status exited {}: {}",
                        output.status,
                        output.stderr.trim()
                    )));
                }
                Ok(json!({
                    "backend": "cli",
                    "executor": "run_git status --porcelain=v2 -z",
                    "porcelain_entries": output.stdout.split('\0').filter(|entry| !entry.is_empty()).count(),
                    "repo_dynamic_settings_disabled": output.repo_dynamic_settings_disabled,
                }))
            }
            other => Err(HarnessError::new(format!(
                "backend must be libgit2 or cli, got {other}"
            ))),
        }
    }
}
