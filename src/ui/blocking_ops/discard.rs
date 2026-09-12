//! Discard execution: the receipt is the record (ADR-0196 Wave 3); the
//! verification the backend already did (`DiscardOutcome::unverified`) is
//! logged here as evidence.
use super::open_backend;
use crate::ui::i18n;
use kagi_git::backend::recording::RunReport;
use kagi_git::OperationPlan;

pub(crate) fn discard_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    paths: &[String],
) -> Result<RunReport, String> {
    let mut repo = open_backend(repo_path).map_err(|e| i18n::op_failed(i18n::Op::RepoOpen, e))?;
    let op = kagi_git::Operation::Discard {
        paths: paths.to_vec(),
    };
    let report = repo.run_recorded(&op, plan);
    if let Ok(kagi_git::OperationOutcome::Discard(outcome)) = &report.result {
        klog!("executed: {}", outcome.oplog_summary());
        // #282: the backend compared `backups[].path` — the repo-relative paths
        // the git layer acted on — against the post-discard unstaged set.
        if outcome.unverified.is_empty() {
            klog!("verified: {} target(s) left the unstaged set", paths.len());
        } else {
            klog!(
                "verify: {} target(s) still unstaged",
                outcome.unverified.len()
            );
        }
        klog!("backup refs: {}", outcome.backup_refs_summary());
    }
    Ok(report)
}
