use super::*;

pub(super) fn snapshot_after(
    repo_path: &std::path::Path,
) -> Result<kagi_git::RepoSnapshot, kagi_git::GitError> {
    open_backend(repo_path)?.snapshot(10_000)
}

pub(super) fn summary_after(snap: &kagi_git::RepoSnapshot) -> StateSummary {
    StateSummary {
        head: snap.head.display(),
        dirty: if snap.status.is_dirty() {
            "dirty"
        } else {
            "clean"
        }
        .to_string(),
    }
}

pub(super) fn verify_merge_after_snapshot(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    new_tip: &CommitId,
    into: Option<&str>,
    verified: &std::sync::atomic::AtomicBool,
) -> StateSummary {
    match snapshot_after(repo_path) {
        Ok(snap) => {
            let matches = match into {
                Some(branch) => {
                    let local = snap.branches.iter().find(|b| b.name == branch).or_else(|| {
                        let (remote, name) = branch.split_once('/')?;
                        snap.remote_branches
                            .iter()
                            .any(|b| b.remote == remote && b.name == name)
                            .then(|| snap.branches.iter().find(|b| b.name == name))
                            .flatten()
                    });
                    local.is_some_and(|b| b.target == *new_tip)
                }
                None => {
                    matches!(&snap.head, Head::Attached { target, .. } | Head::Detached { target } if *target == new_tip.0)
                }
            };
            if matches {
                verified.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            summary_after(&snap)
        }
        Err(_) => plan.predicted.clone(),
    }
}
