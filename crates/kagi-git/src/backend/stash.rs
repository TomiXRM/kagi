//! Stash execution evidence and fresh-open factory. Recording is owned by run's finalize.
use super::recording::{finalize, Recording};
use super::*;
use crate::oplog::{Actor, OpLogEntry, OpOutcome};
use kagi_domain::remove::{RepoId, WorktreeId};
pub use kagi_domain::stash::*;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct StashPlan {
    pub preview: Arc<OperationPlan>,
    pub repo: PathBuf,
    pub common_dir: RepoId,
    /// Identity of the worktree this plan was actually resolved against (#482).
    /// Compared with the frozen `Attachment` before the plan is adopted and
    /// again before approval, so an attach→plan swap cannot be executed.
    pub worktree: WorktreeId,
    pub action: StashAction,
}
#[derive(Clone, Debug)]
pub struct StashReport {
    pub action: StashAction,
    pub recording: Recording,
    pub evidence: StashEvidence,
}

/// Are all of `needles` still present in `haystack`, in the same relative
/// order? Used to verify a stash push left every pre-existing entry alone
/// while tolerating entries a *third party* pushed concurrently (#623).
fn retains_in_order(needles: &[String], haystack: &[String]) -> bool {
    let mut rest = haystack.iter();
    needles
        .iter()
        .all(|needle| rest.any(|candidate| candidate == needle))
}

impl Backend {
    fn verify_restored_stash(&self, oid: &str) -> Result<(), GitError> {
        let check = || -> Result<(), git2::Error> {
            let oid = git2::Oid::from_str(oid)?;
            let head = self.repo.head()?.peel_to_commit()?.id();
            let expected = ops::stash_apply_dry_run(&self.repo, head, oid)?;
            if expected.has_conflicts() {
                return Err(git2::Error::from_str("expected merge still has conflicts"));
            }
            let stash = self.repo.find_commit(oid)?;
            let base = stash.parent(0)?.tree()?;
            let stashed = stash.tree()?;
            let tracked_delta = self
                .repo
                .diff_tree_to_tree(Some(&base), Some(&stashed), None)?;
            let mut restored_paths = std::collections::BTreeSet::new();
            for delta in tracked_delta.deltas() {
                if let Some(path) = delta.old_file().path_bytes() {
                    restored_paths.insert(path.to_vec());
                }
                if let Some(path) = delta.new_file().path_bytes() {
                    restored_paths.insert(path.to_vec());
                }
            }
            if !restored_paths.is_empty() {
                let mut options = git2::DiffOptions::new();
                options.disable_pathspec_match(true);
                for path in &restored_paths {
                    options.pathspec(path.as_slice());
                }
                let diff = self
                    .repo
                    .diff_index_to_workdir(Some(&expected), Some(&mut options))?;
                if diff
                    .deltas()
                    .any(|delta| delta.status() != git2::Delta::Unmodified)
                {
                    return Err(git2::Error::from_str(
                        "restored tracked bytes differ from stash merge",
                    ));
                }
            }
            if stash.parent_count() > 2 {
                let tree = stash.parent(2)?.tree()?;
                let diff = self.repo.diff_tree_to_workdir(Some(&tree), None)?;
                if diff.deltas().any(|delta| {
                    !delta.old_file().id().is_zero() && delta.status() != git2::Delta::Unmodified
                }) {
                    return Err(git2::Error::from_str(
                        "restored untracked bytes differ from stash",
                    ));
                }
            }
            Ok(())
        };
        check().map_err(|e| GitError::Other(format!("stash restore verification: {e}")))
    }
    /// Index-side identity survives resolution-buffer edits, but not a new conflict.
    pub fn stash_conflict_identity(&self) -> Result<Vec<String>, GitError> {
        let index = self
            .repo
            .index()
            .map_err(|e| GitError::Other(e.to_string()))?;
        let conflicts = index
            .conflicts()
            .map_err(|e| GitError::Other(e.to_string()))?;
        let head = resolve_head(&self.repo)?.display();
        let mut result = Vec::new();
        for conflict in conflicts {
            let c = conflict.map_err(|e| GitError::Other(e.to_string()))?;
            let sides: Vec<_> = [c.ancestor, c.our, c.their]
                .into_iter()
                .map(|entry| entry.map(|e| (e.path, e.id.to_string(), e.mode)))
                .collect();
            result.push(format!("{head}:{sides:?}"));
        }
        result.sort();
        Ok(result)
    }
    /// Resolve a continuation by full OID and bind the resulting ordered list.
    /// A missing/ambiguous occurrence never falls back to an index.
    pub fn plan_stash_drop_by_oid(path: &Path, oid: &str) -> Result<Option<StashPlan>, GitError> {
        let Some(index) = Self::unique_stash_index(path, oid)? else {
            return Ok(None);
        };
        let plan = Self::plan_recorded_stash(path, StashAction::Drop { index })?;
        let identity = plan
            .preview
            .stash_identity
            .as_ref()
            .ok_or_else(|| GitError::Other("missing stash identity".into()))?;
        if identity.oids.get(index).map(String::as_str) != Some(oid)
            || identity.oids.iter().filter(|id| id.as_str() == oid).count() != 1
        {
            return Ok(None);
        }
        Ok(Some(plan))
    }
    // Verify actual index entries and tracked/untracked bytes, not only classification.
    pub(super) fn stash_worktree_fingerprint(&self) -> Result<u64, GitError> {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        let index = self
            .repo
            .index()
            .map_err(|e| GitError::Other(e.to_string()))?;
        let mut paths = std::collections::BTreeSet::new();
        for entry in index.iter() {
            (
                entry.path.clone(),
                entry.id.as_bytes(),
                entry.mode,
                entry.flags,
            )
                .hash(&mut hash);
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStringExt;
                paths.insert(PathBuf::from(std::ffi::OsString::from_vec(entry.path)));
            }
            #[cfg(not(unix))]
            {
                paths.insert(PathBuf::from(
                    String::from_utf8(entry.path).map_err(|e| GitError::Other(e.to_string()))?,
                ));
            }
        }
        paths.extend(self.working_tree_status()?.untracked);
        let root = self
            .repo
            .workdir()
            .ok_or_else(|| GitError::Other("missing worktree".into()))?;
        for path in paths {
            path.hash(&mut hash);
            let absolute = root.join(&path);
            match std::fs::symlink_metadata(&absolute) {
                Ok(meta) if meta.is_symlink() => std::fs::read_link(&absolute)
                    .map_err(|e| GitError::Other(e.to_string()))?
                    .hash(&mut hash),
                Ok(meta) if meta.is_file() => std::fs::read(&absolute)
                    .map_err(|e| GitError::Other(e.to_string()))?
                    .hash(&mut hash),
                Ok(_) => "directory".hash(&mut hash),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => "missing".hash(&mut hash),
                Err(e) => return Err(GitError::Other(e.to_string())),
            }
        }
        Ok(hash.finish())
    }
    pub fn plan_recorded_stash(path: &Path, action: StashAction) -> Result<StashPlan, GitError> {
        let backend = Self::open(path)?;
        let repo =
            std::fs::canonicalize(&backend.path).map_err(|e| GitError::Other(e.to_string()))?;
        let worktree = backend.write_worktree_id()?;
        let common_dir = worktree.repo.clone();
        let preview = backend.plan(&action.operation())?;
        Ok(StashPlan {
            preview: Arc::new(preview),
            repo,
            common_dir,
            worktree,
            action,
        })
    }
    pub fn run_recorded_stash(
        plan: &StashPlan,
        actor: Actor,
        auto_snapshot: bool,
        fault: Option<StashFaultPoint>,
        mut event: impl FnMut(StashEvent),
    ) -> StashReport {
        let mut backend = match Self::open_with_policy(
            &plan.repo,
            ExecutionPolicy {
                actor,
                auto_snapshot,
            },
        ) {
            Ok(backend) => backend,
            Err(e) => {
                event(StashEvent::Started);
                return stash_failure(plan, actor, &e.to_string(), StashStopReason::OpenFailed);
            }
        };
        if matches!(fault, Some(StashFaultPoint::Untrusted)) {
            backend.trust = crate::trust::RepoTrust::Untrusted;
        }
        if backend.write_repo_id().ok().as_ref() != Some(&plan.common_dir) {
            event(StashEvent::Started);
            return stash_failure(
                plan,
                actor,
                "repository identity changed after planning",
                StashStopReason::IdentityChanged,
            );
        }
        if backend.trust.is_trusted() && !plan.preview.blockers.is_empty() {
            event(StashEvent::PlanBlocked);
        } else {
            event(StashEvent::Started);
        }
        let report =
            backend.run_recorded_with_events(&plan.action.operation(), &plan.preview, fault, event);
        StashReport {
            action: plan.action.clone(),
            recording: report.recording,
            evidence: report.stash.unwrap_or_default(),
        }
    }
    pub fn read_stash_status(plan: &StashPlan) -> Result<String, GitError> {
        let mut backend = Self::open(&plan.repo)?;
        if backend.write_repo_id()? != plan.common_dir {
            return Err(GitError::Other("repository identity changed".into()));
        }
        Ok(format!(
            "head={}; status={:?}; stashes={:?}",
            resolve_head(&backend.repo)?.display(),
            backend.working_tree_status()?,
            ops::stash_identity(&mut backend.repo, None)?.oids
        ))
    }
    pub fn unique_stash_index(path: &Path, oid: &str) -> Result<Option<usize>, GitError> {
        let mut backend = Self::open(path)?;
        let identity = ops::stash_identity(&mut backend.repo, None)?;
        let matches: Vec<_> = identity
            .oids
            .iter()
            .enumerate()
            .filter(|(_, id)| id.as_str() == oid)
            .map(|(i, _)| i)
            .collect();
        Ok(if matches.len() == 1 {
            Some(matches[0])
        } else {
            None
        })
    }
    pub(super) fn verify_stash_run(
        &mut self,
        action: &StashAction,
        plan: &OperationPlan,
        evidence: &mut StashEvidence,
    ) -> Result<(bool, usize), GitError> {
        let actual = ops::stash_identity(&mut self.repo, None)?;
        let expected = plan
            .stash_identity
            .as_ref()
            .ok_or_else(|| GitError::Other("missing stash identity".into()))?;
        let status = self.working_tree_status()?;
        let dirty = status.is_dirty();
        let observed_conflicts: Vec<_> = status
            .conflicted
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let observed_identity = self.stash_conflict_identity()?;
        let operation_created_conflict =
            matches!(action, StashAction::Apply { .. } | StashAction::Pop { .. })
                && !observed_conflicts.is_empty()
                && (observed_identity != evidence.conflict_identity_before
                    || !evidence.conflicts.is_empty());
        if operation_created_conflict {
            evidence.conflicts = observed_conflicts;
            evidence.conflict_identity = observed_identity;
        } else {
            evidence.conflicts.clear();
            evidence.conflict_identity.clear();
        }
        evidence.after = Some(ops::StateSummary {
            head: resolve_head(&self.repo)?.display(),
            dirty: if evidence.conflicts.is_empty() {
                if dirty { "dirty" } else { "clean" }.into()
            } else {
                format!("{} conflicted (stash kept)", evidence.conflicts.len())
            },
        });
        let mut remaining = expected.oids.clone();
        match action {
            StashAction::Push {
                include_untracked, ..
            } => {
                // #623: an external `git stash push` can add entries kagi never
                // made, so "exactly one longer, ours on top" is not the
                // invariant — it failed a correctly identified push just
                // because someone else stashed in the same second, and it also
                // *replaced* the identified OID with whatever sat on top. What
                // must hold is narrower and true: the entry the executor
                // identified is on the stack exactly once, and nothing that was
                // there when kagi planned has gone.
                let created = evidence.oid.clone().ok_or_else(|| {
                    GitError::Other("stash push recorded no created stash".into())
                })?;
                let mut others = actual.oids.clone();
                let Some(at) = others.iter().position(|oid| *oid == created) else {
                    return Err(GitError::Other(
                        "stash push list verification failed".into(),
                    ));
                };
                others.remove(at);
                if others.contains(&created) || !retains_in_order(&remaining, &others) {
                    return Err(GitError::Other(
                        "stash push list verification failed".into(),
                    ));
                }
                let mut expected_untracked = if *include_untracked {
                    vec![]
                } else {
                    evidence.untracked_before.clone()
                };
                let mut actual_untracked = status.untracked.clone();
                expected_untracked.sort();
                actual_untracked.sort();
                if !status.staged.is_empty()
                    || !status.unstaged.is_empty()
                    || !status.conflicted.is_empty()
                    || expected_untracked != actual_untracked
                {
                    return Err(GitError::Other(
                        "stash push worktree verification failed".into(),
                    ));
                }
            }
            StashAction::Pop { index } if evidence.conflicts.is_empty() => {
                remaining.remove(*index);
            }
            StashAction::Drop { index } => {
                remaining.remove(*index);
                if evidence.worktree_before != Some(self.stash_worktree_fingerprint()?) {
                    return Err(GitError::Other("stash drop changed worktree/index".into()));
                }
            }
            _ => {}
        }
        if !matches!(action, StashAction::Push { .. }) && remaining != actual.oids {
            return Err(GitError::Other("stash list verification failed".into()));
        }
        if matches!(action, StashAction::Apply { .. } | StashAction::Pop { .. })
            && evidence.conflicts.is_empty()
        {
            self.verify_restored_stash(
                evidence
                    .oid
                    .as_deref()
                    .ok_or_else(|| GitError::Other("missing restored stash oid".into()))?,
            )?;
        }
        evidence.verified = true;
        Ok((dirty, actual.oids.len()))
    }
}
pub fn record_stash_plan_error(
    path: &Path,
    actor: Actor,
    action: &StashAction,
    error: &str,
) -> Recording {
    let entry = OpLogEntry::new(
        action.name(),
        path.display().to_string(),
        ops::StateSummary {
            head: "unavailable".into(),
            dirty: "unavailable".into(),
        },
        OpOutcome::Failed {
            error: error.into(),
        },
    )
    .with_actor(actor)
    .with_worktree(Some(path.display().to_string()));
    finalize(entry)
}
pub fn stash_failure(
    plan: &StashPlan,
    actor: Actor,
    error: &str,
    stop: StashStopReason,
) -> StashReport {
    let entry = OpLogEntry::new(
        plan.action.name(),
        plan.repo.display().to_string(),
        plan.preview.current.clone(),
        OpOutcome::Failed {
            error: error.into(),
        },
    )
    .with_actor(actor)
    .with_worktree(Some(plan.repo.display().to_string()));
    StashReport {
        action: plan.action.clone(),
        recording: finalize(entry),
        evidence: StashEvidence {
            plan_blocked: stop == StashStopReason::PlanBlocked,
            stop: Some(stop),
            ..Default::default()
        },
    }
}

pub(super) fn stash_outcome(
    result: &Result<OperationOutcome, GitError>,
    plan: &OperationPlan,
    evidence: &StashEvidence,
) -> OpOutcome {
    let mut after = evidence.after.clone().unwrap_or_else(|| ops::StateSummary {
        head: "unobserved".into(),
        dirty: "unobserved".into(),
    });
    after.dirty.push_str(&format!(
        "; stash oid={}; applied={}; snapshot={}",
        evidence.oid.as_deref().unwrap_or("unavailable"),
        evidence.applied,
        evidence.snapshot.as_deref().unwrap_or("none")
    ));
    if evidence.unknown {
        return OpOutcome::Unknown {
            after,
            evidence: evidence.observations.join("; "),
        };
    }
    if !evidence.started {
        if evidence.stop == Some(StashStopReason::PlanBlocked) {
            return OpOutcome::Refused {
                blockers: plan.blockers.iter().map(|b| b.message_en()).collect(),
            };
        }
        if let Err(GitError::Preflight(error)) = result {
            return OpOutcome::Refused {
                blockers: vec![error.to_string()],
            };
        }
        return match result {
            Err(e) => OpOutcome::Failed {
                error: e.to_string(),
            },
            Ok(_) => OpOutcome::Failed {
                error: "execution did not start".into(),
            },
        };
    }
    if let Err(e) = result {
        return OpOutcome::Partial {
            after,
            error: e.to_string(),
        };
    }
    if !evidence.conflicts.is_empty() {
        return OpOutcome::Partial {
            after,
            error: format!("{} conflicted (stash kept)", evidence.conflicts.len()),
        };
    }
    if let Ok(OperationOutcome::StashDrop { oid }) = result {
        after.dirty = format!("stash entry deleted (oid {oid})");
    }
    OpOutcome::Success { after }
}
