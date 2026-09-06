//! Discard execution keeps the full recording result until UI settlement.
use super::open_backend;
use crate::ui::i18n;
use kagi_git::backend::recording::RunReport;
use kagi_git::{Backend, OperationPlan, StateSummary};

type Presentation = Result<(String, StateSummary, Option<String>), String>;
pub(crate) struct DiscardReport {
    pub run: RunReport,
    pub presentation: Presentation,
}

pub(crate) fn discard_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    paths: &[String],
) -> Result<DiscardReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let op = kagi_git::Operation::Discard {
        paths: paths.to_vec(),
    };
    let run = repo.run_recorded(&op, plan);
    let presentation = present_discard(&repo, &run, plan, paths);
    Ok(DiscardReport { run, presentation })
}

fn present_discard(
    repo: &Backend,
    run: &RunReport,
    plan: &OperationPlan,
    paths: &[String],
) -> Presentation {
    let outcome = match &run.result {
        Ok(kagi_git::OperationOutcome::Discard(d)) => d,
        Ok(_) => return Err("discard: unexpected outcome variant".to_string()),
        Err(e) => return Err(i18n::op_failed(i18n::Op::Discard, e)),
    };
    let summary = outcome.oplog_summary();
    klog!("executed: {}", summary);

    // Verify: re-read status; targets must have left the unstaged set.
    // #282: compare against `outcome.backups[].path` — the repo-relative paths the
    // git layer actually acted on — not the raw UI strings, so both sides of the
    // comparison went through the same normalization.
    let mut leftover: Vec<String> = Vec::new();
    match repo.working_tree_status() {
        Ok(status) => {
            let still: std::collections::HashSet<String> = status
                .unstaged
                .iter()
                .map(|f| f.path.to_string_lossy().replace('\\', "/"))
                .collect();
            leftover = outcome
                .backups
                .iter()
                .map(|b| b.path.clone())
                .filter(|p| still.contains(p))
                .collect();
            if leftover.is_empty() {
                klog!("verified: {} target(s) left the unstaged set", paths.len());
            } else {
                klog!("verify: {} target(s) still unstaged", leftover.len());
            }
        }
        Err(e) => klog!("verify: status error: {}", e),
    }

    klog!("backup refs: {}", outcome.backup_refs_summary());

    // #281: a leftover target means the discard was only partially applied — it
    // must NOT be reported as a plain success.
    let partial = outcome.error.clone().or_else(|| {
        (!leftover.is_empty()).then(|| {
            format!(
                "discard verify failed: {} target(s) not discarded: {}",
                leftover.len(),
                leftover.join(", ")
            )
        })
    });

    // The after-state carries the recovery handle (path→blob list) into the oplog,
    // partial or not.
    let after = StateSummary {
        head: plan.current.head.clone(),
        dirty: summary,
    };
    let human = if outcome.backups.len() == 1 {
        format!("{} discarded", outcome.backups[0].path)
    } else {
        format!("{} files discarded", outcome.backups.len())
    };
    Ok((human, after, partial))
}
