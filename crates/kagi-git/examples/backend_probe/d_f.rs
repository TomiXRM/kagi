//! P5 execution-path and mixed-backend probe for #627.
//!
//! P0 owns the root dispatcher; this registered module owns only P5 operation
//! behaviour.

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
