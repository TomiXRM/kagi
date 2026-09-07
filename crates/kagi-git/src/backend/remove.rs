//! Recorded remove boundary. Owns progress across executor unwind, never a UI callback.
use super::recording::finalize as record;
pub use super::recording::Recording;
use super::*;
use crate::oplog::{Actor, OpLogEntry, OpOutcome};
use kagi_domain::remove::{RemoveFaultPoint, RemoveProgress};
use kagi_domain::remove::{RepoId, WorktreeId};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    inode: u64,
    birth: Option<SystemTime>,
    gitdir: Vec<u8>,
    config: Option<String>,
    head: String,
    head_ref: String,
}

#[derive(Clone, Debug)]
pub struct RemovePlan {
    pub preview: Arc<OperationPlan>,
    pub repo: PathBuf,
    pub common_dir: RepoId,
    /// Identity of the *managing* worktree this plan was resolved against
    /// (#482); `worktree_id` below is the removal target. Compared with the
    /// frozen `Attachment` before adoption and again before approval.
    pub worktree: WorktreeId,
    pub target: PathBuf,
    pub worktree_id: WorktreeId,
    name: String,
    delete_branch: bool,
    fingerprint: Option<Fingerprint>,
}

#[derive(Clone, Debug)]
pub struct RemoveReport {
    pub name: String,
    pub target_exists: Option<bool>,
    pub recording: Recording,
    pub progress: RemoveProgress,
}
#[derive(Clone, Copy)]
pub enum RemoveEvent {
    ConfigTrusted,
    ExecutionStarting,
    ConfigRefused,
}

fn io(error: impl std::fmt::Display) -> GitError {
    GitError::Other(error.to_string())
}

fn optional_birth(created: std::io::Result<SystemTime>) -> Result<Option<SystemTime>, GitError> {
    match created {
        Ok(time) => Ok(Some(time)),
        Err(error) if error.kind() == std::io::ErrorKind::Unsupported => Ok(None),
        Err(error) => Err(io(error)),
    }
}

fn fingerprint(repo: &Repository, name: &str) -> Result<Fingerprint, GitError> {
    let admin = repo.commondir().join("worktrees").join(name);
    let meta = std::fs::symlink_metadata(&admin).map_err(io)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(io("invalid admin directory"));
    }
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        meta.ino()
    };
    #[cfg(not(unix))]
    let inode = return Err(io("admin identity unsupported on this platform"));
    let wt = repo.find_worktree(name).map_err(io)?;
    let linked = Repository::open(wt.path()).map_err(io)?;
    let head = linked
        .head()
        .map_err(io)?
        .target()
        .ok_or_else(|| io("missing linked HEAD"))?
        .to_string();
    let head_ref = linked.head().map_err(io)?.name().unwrap_or("").to_string();
    Ok(Fingerprint {
        inode,
        birth: optional_birth(meta.created())?,
        gitdir: std::fs::read(admin.join("gitdir")).map_err(io)?,
        config: ops::load_worktree_config(wt.path())?.map(|cfg| cfg.sha256),
        head,
        head_ref,
    })
}

impl Backend {
    pub fn plan_recorded_remove(
        path: &Path,
        name: &str,
        delete_branch: bool,
    ) -> Result<RemovePlan, GitError> {
        let backend = Self::open(path)?;
        let preview = backend.plan_remove_worktree(name, delete_branch)?;
        let common_dir = std::fs::canonicalize(backend.repo.commondir()).map_err(io)?;
        let repo = std::fs::canonicalize(&backend.path).map_err(io)?;
        // Canonicalized like `repo` above: the registered worktree path can
        // still be the pre-symlink one (`/tmp/…` vs `/private/tmp/…` on macOS),
        // and callers match it against canonical paths — the UI closes the
        // removed worktree's tab by this path (#528). Falls back to the raw
        // path so a target that cannot be resolved still plans (and refuses).
        let target = backend
            .repo
            .find_worktree(name)
            .ok()
            .map(|w| {
                let path = w.path();
                std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
            })
            .unwrap_or_else(|| repo.clone());
        let worktree_id = WorktreeId {
            repo: RepoId(common_dir.clone()),
            git_dir: common_dir.join("worktrees").join(name),
        };
        // Blocked missing/main plans still have a preview and a recorded refusal.
        let fingerprint = if preview.blockers.is_empty() {
            Some(fingerprint(&backend.repo, name)?)
        } else {
            None
        };
        Ok(RemovePlan {
            preview: Arc::new(preview),
            repo,
            worktree: backend.write_worktree_id()?,
            common_dir: RepoId(common_dir),
            target,
            worktree_id,
            name: name.into(),
            delete_branch,
            fingerprint,
        })
    }

    pub fn run_recorded_remove(
        plan: &RemovePlan,
        actor: Actor,
        fault: Option<RemoveFaultPoint>,
    ) -> RemoveReport {
        Self::run_recorded_remove_with_events(plan, actor, fault, |_| {})
    }

    pub fn run_recorded_remove_with_events(
        plan: &RemovePlan,
        actor: Actor,
        fault: Option<RemoveFaultPoint>,
        mut event: impl FnMut(RemoveEvent),
    ) -> RemoveReport {
        let mut progress = RemoveProgress::default();
        let mut opened = false;
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<DiscardOutcome, GitError> {
            let mut backend = Self::open(&plan.repo)?;
            opened = true;
            if fault == Some(RemoveFaultPoint::UntrustedMain) {
                backend.set_trust_for_test(crate::trust::RepoTrust::Untrusted);
            }
            backend.require_trust()?;
            if !plan.preview.blockers.is_empty() {
                return Err(io("plan has blockers"));
            }
            let mut linked = Self::open(&plan.target)?;
            if fault == Some(RemoveFaultPoint::UntrustedTarget) {
                linked.set_trust_for_test(crate::trust::RepoTrust::Untrusted);
            }
            linked.require_trust()?;
            if Some(fingerprint(&backend.repo, &plan.name)?) != plan.fingerprint {
                return Err(io("remove-worktree admin identity changed after plan"));
            }
            backend.preflight_check(&plan.preview)?;
            // Re-plan target dirt/lock before any trust write or hook.
            if !backend
                .plan_remove_worktree(&plan.name, plan.delete_branch)?
                .blockers
                .is_empty()
            {
                return Err(io("remove-worktree target changed after plan"));
            }
            if fault == Some(RemoveFaultPoint::PanicBeforeMutation) {
                panic!("injected before mutation");
            }
            if ops::plan_requires_worktree_trust(&plan.preview) {
                progress.config_granted = true; // write may partially succeed
                if let Err(error) = backend.trust_worktree_config_for_worktree(
                    &plan.name,
                    ops::plan_worktree_config_sha(&plan.preview).unwrap_or(""),
                ) {
                    event(RemoveEvent::ConfigRefused);
                    return Err(error);
                }
                progress.observations.push("config trust granted".into());
                event(RemoveEvent::ConfigTrusted);
            }
            event(RemoveEvent::ExecutionStarting);
            ops::execute_remove_worktree_progress(
                &backend.repo,
                &plan.preview,
                &plan.name,
                plan.delete_branch,
                &mut progress,
                fault,
            )
        }));
        let mut after = recovery_after(&progress);
        // Observe the management HEAD; never substitute the predicted state.
        if let Ok(head) = Self::open(&plan.repo).and_then(|backend| resolve_head(&backend.repo)) {
            after.head = head.display();
        }
        let outcome = match result {
            Ok(Ok(value)) if value.error.is_none() => OpOutcome::Success { after },
            Ok(Ok(value)) => OpOutcome::Partial {
                after,
                error: evidence(&progress, &value.error.unwrap_or_default()),
            },
            Ok(Err(error)) if progress.termination_unknown => OpOutcome::Unknown {
                after,
                evidence: evidence(&progress, &error.to_string()),
            },
            Ok(Err(error)) if progress.policy_rejected && !progress.started() => {
                OpOutcome::Failed {
                    error: error.to_string(),
                }
            }
            Ok(Err(error)) if progress.started() => OpOutcome::Partial {
                after,
                error: evidence(&progress, &error.to_string()),
            },
            Ok(Err(error)) if !opened => OpOutcome::Failed {
                error: error.to_string(),
            },
            Ok(Err(error)) => OpOutcome::Refused {
                blockers: vec![error.to_string()],
            },
            Err(_) if progress.started() => OpOutcome::Unknown {
                after,
                evidence: evidence(&progress, "executor panic; do not retry"),
            },
            Err(_) => OpOutcome::Failed {
                error: "executor panic before mutation".into(),
            },
        };
        let mut entry = OpLogEntry::new(
            "remove-worktree",
            plan.repo.display().to_string(),
            plan.preview.current.clone(),
            outcome,
        );
        entry.backup_refs = progress
            .backups
            .iter()
            .map(|b| b.reference.clone())
            .collect();
        entry.actor = actor;
        entry.worktree = Some(plan.target.display().to_string());
        RemoveReport {
            name: plan.name.clone(),
            target_exists: plan.target.try_exists().ok(),
            recording: record(entry),
            progress,
        }
    }

    /// Non-mutating reconciliation read. A successful read is NOT proof of process termination.
    pub fn read_remove_status(plan: &RemovePlan) -> Result<String, GitError> {
        let mut backend = Self::open(&plan.repo)?;
        let snapshot = backend.snapshot(1)?;
        Ok(format!(
            "head={:?}; registered={}; target_exists={}",
            snapshot.head,
            backend.repo.find_worktree(&plan.name).is_ok(),
            plan.target.exists()
        ))
    }
    pub fn abandoned_remove(plan: &RemovePlan, actor: Actor) -> RemoveReport {
        let mut entry = OpLogEntry::new(
            "remove-worktree",
            plan.repo.display().to_string(),
            plan.preview.current.clone(),
            OpOutcome::Failed {
                error: "job dropped before execution".into(),
            },
        );
        entry.actor = actor;
        entry.worktree = Some(plan.target.display().to_string());
        RemoveReport {
            name: plan.name.clone(),
            target_exists: plan.target.try_exists().ok(),
            recording: record(entry),
            progress: Default::default(),
        }
    }
}

pub fn record_plan_error(path: &Path, actor: Actor, error: &str) -> Recording {
    let mut entry = OpLogEntry::new(
        "remove-worktree",
        path.display().to_string(),
        ops::StateSummary {
            head: "unavailable".into(),
            dirty: "unavailable".into(),
        },
        OpOutcome::Failed {
            error: error.into(),
        },
    );
    entry.actor = actor;
    record(entry)
}

fn recovery_after(progress: &RemoveProgress) -> ops::StateSummary {
    let pairs: Vec<_> = progress
        .backups
        .iter()
        .map(|b| format!("{}={}", b.path, b.blob))
        .collect();
    ops::StateSummary {
        head: progress
            .branch_tip
            .as_ref()
            .map(|id| id.0.clone())
            .unwrap_or_else(|| "unobserved".into()),
        dirty: format!(
            "stage={:?}; backup: {}; branch_tip={}",
            progress.stage,
            pairs.join(", "),
            progress
                .branch_tip
                .as_ref()
                .map(|id| id.0.as_str())
                .unwrap_or("unavailable")
        ),
    }
}
fn evidence(progress: &RemoveProgress, error: &str) -> String {
    format!(
        "stage={:?}; verification={:?}; termination_unknown={}; observations={:?}; {}",
        progress.stage,
        progress.verification,
        progress.termination_unknown,
        progress.observations,
        error
    )
}

#[cfg(test)]
mod fingerprint_tests {
    use super::*;

    #[test]
    fn unsupported_birthtime_preserves_other_fingerprint_checks() {
        let missing = || optional_birth(Err(std::io::ErrorKind::Unsupported.into())).unwrap();
        let planned = Fingerprint {
            inode: 7,
            birth: missing(),
            gitdir: b"/repo/worktree/.git".to_vec(),
            config: None,
            head: "tip".into(),
            head_ref: "refs/heads/topic".into(),
        };
        let mut fresh = planned.clone();
        fresh.birth = missing();
        assert_eq!(planned, fresh);
        for changed in [
            Fingerprint {
                inode: 8,
                ..fresh.clone()
            },
            Fingerprint {
                gitdir: b"/other/.git".to_vec(),
                ..fresh.clone()
            },
            Fingerprint {
                config: Some("config-sha".into()),
                ..fresh.clone()
            },
            Fingerprint {
                head: "new-tip".into(),
                ..fresh.clone()
            },
            Fingerprint {
                head_ref: "refs/heads/other".into(),
                ..fresh.clone()
            },
            Fingerprint {
                birth: Some(SystemTime::UNIX_EPOCH),
                ..fresh.clone()
            },
        ] {
            assert_ne!(planned, changed);
        }
    }

    #[test]
    fn birthtime_retains_supported_values_and_propagates_other_errors() {
        assert_eq!(
            optional_birth(Ok(SystemTime::UNIX_EPOCH)).unwrap(),
            Some(SystemTime::UNIX_EPOCH)
        );
        assert!(optional_birth(Err(std::io::ErrorKind::PermissionDenied.into())).is_err());
    }
}
