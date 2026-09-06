//! Recorded remove boundary. Owns progress across executor unwind, never a UI callback.
use super::*;
use crate::oplog::{append_oplog_receipt, Actor, OpLogEntry, OpOutcome};
use kagi_domain::remove::{RemoveFaultPoint, RemoveProgress};
use kagi_domain::remove::{RepoId, WorktreeId};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    inode: u64,
    birth: SystemTime,
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
    pub target: PathBuf,
    pub worktree_id: WorktreeId,
    name: String,
    delete_branch: bool,
    fingerprint: Option<Fingerprint>,
}

#[derive(Clone, Debug)]
pub enum Recording {
    Appended {
        path: PathBuf,
        entry: OpLogEntry,
    },
    Failed {
        attempted: OpLogEntry,
        error: String,
    },
}
impl Recording {
    pub fn entry(&self) -> &OpLogEntry {
        match self {
            Self::Appended { entry, .. } => entry,
            Self::Failed { attempted, .. } => attempted,
        }
    }
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
        birth: meta.created().map_err(io)?,
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
        let target = backend
            .repo
            .find_worktree(name)
            .ok()
            .map(|w| w.path().to_path_buf())
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
            let backend = Self::open(&plan.repo)?;
            opened = true;
            backend.require_trust()?;
            if !plan.preview.blockers.is_empty() {
                return Err(io("plan has blockers"));
            }
            let linked = Self::open(&plan.target)?;
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

fn record(entry: OpLogEntry) -> Recording {
    match append_oplog_receipt(&entry) {
        Ok((path, entry)) => Recording::Appended { path, entry },
        Err(error) => Recording::Failed {
            attempted: entry,
            error: error.to_string(),
        },
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
