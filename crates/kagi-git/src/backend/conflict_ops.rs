//! Conflict execution stays behind owner trust, including frozen D/F plans.
use super::*;
use kagi_domain::conflict_family::{
    ConflictDraft, ConflictEvidence, ConflictObservation, ConflictProgress, ConflictRequest,
    ConflictRevision,
};

// Reading the repository — the one operation observation, the fingerprints,
// and the post-execute verification — lives next door so the per-tab read
// model and this family can never disagree about what is in progress
// (#704 / ADR-0196).
#[path = "conflict_observe.rs"]
pub mod conflict_observe;
pub use conflict_observe::ConflictSnapshot;
use conflict_observe::{
    buffer_revision, observation, revision_label, short_text_hash, text_mode, verify_abort,
    verify_dir_file, verify_save,
};

#[derive(Clone, Debug)]
enum ConflictPreparedAction {
    Save {
        path: PathBuf,
        draft: ConflictDraft,
        expected_mode: u32,
    },
    DirFile(ops::DirFilePlan),
    /// #704: end the operation. Boxed — a `ConflictSession` carries every
    /// conflicting file, and this is the rare variant.
    Abort {
        session: Box<conflicts::ConflictSession>,
    },
}

#[derive(Clone, Debug)]
/// Opaque prepared conflict operation. Callers may inspect the frozen identity
/// through accessors but cannot replace a revision while retaining old bytes.
///
/// ```compile_fail
/// use kagi_git::backend::conflict_ops::ConflictPlan;
/// fn forge(plan: &mut ConflictPlan) {
///     plan.request = panic!("a public caller cannot replace frozen request bytes");
/// }
/// ```
pub struct ConflictPlan {
    repo: PathBuf,
    common_dir: kagi_domain::remove::RepoId,
    worktree: kagi_domain::remove::WorktreeId,
    request: ConflictRequest,
    before: ops::StateSummary,
    observed: ConflictObservation,
    op_name: String,
    action: ConflictPreparedAction,
}

impl ConflictPlan {
    pub fn repo(&self) -> &Path {
        &self.repo
    }

    pub fn common_dir(&self) -> &kagi_domain::remove::RepoId {
        &self.common_dir
    }

    pub fn worktree(&self) -> &kagi_domain::remove::WorktreeId {
        &self.worktree
    }

    pub fn request(&self) -> &ConflictRequest {
        &self.request
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictFaultPoint {
    BeforeMutation,
    AfterWorktreeWrite,
    /// #704: the abort ran, and then the repository could not be read back.
    /// The one case the receipt must call `Unknown` rather than success or a
    /// retryable failure — there is no other way to reach it deterministically.
    AbortAfterStateUnreadable,
}

#[derive(Clone, Debug)]
pub struct ConflictReport {
    pub recording: recording::Recording,
    pub evidence: ConflictEvidence,
}

impl Backend {
    pub fn record_conflict_save_refusal(
        repo: &Path,
        policy: ExecutionPolicy,
        operation: &str,
        path: &Path,
        error: &str,
    ) -> recording::Recording {
        recording::finalize(
            crate::oplog::OpLogEntry::new(
                format!("conflict-save:{operation}"),
                repo.display().to_string(),
                ops::StateSummary {
                    head: format!("session={operation} file={}", path.display()),
                    dirty: format!("hunks=[] before={}", short_text_hash(b"")),
                },
                crate::oplog::OpOutcome::Refused {
                    blockers: vec![error.into()],
                },
            )
            .with_actor(policy.actor)
            .with_worktree(Some(repo.display().to_string())),
        )
    }

    pub fn record_conflict_refusal(
        path: &Path,
        policy: ExecutionPolicy,
        request: &ConflictRequest,
        error: &str,
    ) -> recording::Recording {
        let op_name = match request {
            ConflictRequest::Save { operation, .. } => format!("conflict-save:{operation}"),
            ConflictRequest::ResolveDirFile { choice, .. } => {
                format!("conflict-dir-file:{}", choice.slug())
            }
            ConflictRequest::Abort { operation, .. } => format!("{operation}-abort"),
        };
        recording::finalize(
            crate::oplog::OpLogEntry::new(
                op_name,
                path.display().to_string(),
                ops::StateSummary {
                    head: format!("conflict revision {}", revision_label(request.revision())),
                    dirty: match request.path() {
                        Some(path) => format!("{} {}", request.action().name(), path.display()),
                        None => request.action().name(),
                    },
                },
                crate::oplog::OpOutcome::Refused {
                    blockers: vec![error.into()],
                },
            )
            .with_actor(policy.actor)
            .with_worktree(Some(path.display().to_string())),
        )
    }

    pub fn conflict_abandoned(plan: &ConflictPlan, policy: ExecutionPolicy) -> ConflictReport {
        let detail = "conflict job dropped before execution".to_string();
        let recording = recording::finalize(
            crate::oplog::OpLogEntry::new(
                plan.op_name.clone(),
                plan.repo.display().to_string(),
                plan.before.clone(),
                crate::oplog::OpOutcome::Failed {
                    error: detail.clone(),
                },
            )
            .with_actor(policy.actor)
            .with_worktree(Some(plan.repo.display().to_string())),
        );
        ConflictReport {
            recording,
            evidence: ConflictEvidence {
                action: plan.request.action(),
                progress: ConflictProgress::NotStarted,
                before: ConflictObservation {
                    revision: plan.request.revision().clone(),
                    operation: "abandoned".into(),
                    paths: plan
                        .request
                        .path()
                        .map(Path::to_path_buf)
                        .into_iter()
                        .collect(),
                },
                after: None,
                detail,
            },
        }
    }

    pub fn conflict_snapshot(&self) -> Result<Option<ConflictSnapshot>, GitError> {
        observation(&self.repo)
    }

    pub fn conflict_save_request(
        revision: ConflictRevision,
        buffer: &ResolutionBuffer,
        path: &Path,
        operation: &str,
        before_text: &str,
    ) -> Result<ConflictRequest, GitError> {
        let draft = buffer.conflict_draft(path).ok_or_else(|| {
            GitError::Other(format!("no resolution draft for {}", path.display()))
        })?;
        Ok(ConflictRequest::Save {
            path: path.to_path_buf(),
            revision,
            buffer_revision: buffer_revision(path, &draft),
            draft,
            operation: operation.to_string(),
            before_hash: short_text_hash(before_text.as_bytes()),
            actions: buffer.conflict_action_summary(path),
        })
    }

    /// The Abort request for an observed in-progress operation (#704).
    ///
    /// Takes the pure observation the read model carries, so no caller needs a
    /// `ConflictSession`, an open editor, or a repository handle to ask for
    /// one. Whether the frozen revision still describes the repository is the
    /// Backend's business, at plan and again at execute.
    pub fn conflict_abort_request(
        operation: &kagi_domain::conflict_family::InProgressOperation,
    ) -> ConflictRequest {
        ConflictRequest::Abort {
            revision: operation.revision().clone(),
            operation: operation.slug().to_string(),
        }
    }

    pub fn plan_recorded_conflict(
        path: &Path,
        request: ConflictRequest,
    ) -> Result<ConflictPlan, GitError> {
        let backend = Self::open(path)?;
        let repo =
            std::fs::canonicalize(&backend.path).map_err(|e| GitError::Other(e.to_string()))?;
        let worktree = backend.write_worktree_id()?;
        let snapshot = observation(&backend.repo)?
            .ok_or_else(|| GitError::Other("the repository is not in conflict".into()))?;
        if &snapshot.observation.revision != request.revision() {
            return Err(GitError::Other(
                "conflict changed since it was observed — re-open the conflict".into(),
            ));
        }
        let action = match &request {
            ConflictRequest::Save {
                path,
                buffer_revision: expected,
                draft,
                operation,
                ..
            } => {
                if operation != &snapshot.observation.operation {
                    return Err(GitError::Other(
                        "conflict operation changed since it was observed".into(),
                    ));
                }
                if buffer_revision(path, draft) != *expected {
                    return Err(GitError::Other(
                        "resolution buffer changed since it was prepared".into(),
                    ));
                }
                if !snapshot.session.files.iter().any(|file| &file.path == path) {
                    return Err(GitError::Other(format!(
                        "{} is not in the observed conflict",
                        path.display()
                    )));
                }
                if let ConflictDraft::Text(bytes) = draft {
                    let text = std::str::from_utf8(bytes).map_err(|_| {
                        GitError::Other("text resolution is not valid UTF-8".into())
                    })?;
                    if crate::checklist::text_has_conflict_marker(text) {
                        return Err(GitError::Other(
                            "conflict markers remain in the resolution buffer".into(),
                        ));
                    }
                }
                ConflictPreparedAction::Save {
                    path: path.clone(),
                    draft: draft.clone(),
                    expected_mode: match draft {
                        ConflictDraft::Raw { mode, .. } => *mode,
                        ConflictDraft::Text(_) => text_mode(&backend.repo, path),
                    },
                }
            }
            ConflictRequest::ResolveDirFile { path, choice, .. } => {
                ConflictPreparedAction::DirFile(ops::plan_dir_file_resolution(
                    &backend.repo,
                    path,
                    *choice,
                )?)
            }
            // #704: abort is about the operation, so the only thing to freeze
            // is which operation it is. The revision check above already
            // proved the repository has not moved since the read model
            // observed it; `run_recorded_conflict` re-reads it live.
            ConflictRequest::Abort { operation, .. } => {
                if operation != &snapshot.observation.operation {
                    return Err(GitError::Other(
                        "conflict operation changed since it was observed".into(),
                    ));
                }
                ConflictPreparedAction::Abort {
                    session: Box::new(snapshot.session.clone()),
                }
            }
        };
        let op_name = match &request {
            ConflictRequest::Save { operation, .. } => format!("conflict-save:{operation}"),
            ConflictRequest::ResolveDirFile { choice, .. } => {
                format!("conflict-dir-file:{}", choice.slug())
            }
            // The oplog name every abort has carried since ADR-0056.
            ConflictRequest::Abort { operation, .. } => format!("{operation}-abort"),
        };
        let before = match &request {
            ConflictRequest::Save {
                path,
                operation,
                before_hash,
                actions,
                ..
            } => ops::StateSummary {
                head: format!("session={operation} file={}", path.display()),
                dirty: format!("hunks=[{actions}] before={before_hash}"),
            },
            ConflictRequest::ResolveDirFile { path, choice, .. } => ops::StateSummary {
                head: format!("dir-file conflict {}", path.display()),
                dirty: format!("choice={}", choice.slug()),
            },
            ConflictRequest::Abort { .. } => {
                crate::conflicts::current_state_summary(&backend.repo)?
            }
        };
        Ok(ConflictPlan {
            repo,
            common_dir: worktree.repo.clone(),
            worktree,
            before,
            observed: snapshot.observation,
            op_name,
            request,
            action,
        })
    }

    pub fn run_recorded_conflict(
        plan: &ConflictPlan,
        policy: ExecutionPolicy,
        fault: Option<ConflictFaultPoint>,
    ) -> ConflictReport {
        let action = plan.request.action();
        let mut progress = ConflictProgress::NotStarted;
        let mut after = None;
        let mut recovery = None;
        let backend = match Self::open_with_policy(&plan.repo, policy) {
            Ok(backend) => backend,
            Err(error) => {
                let outcome = crate::oplog::OpOutcome::Failed {
                    error: error.to_string(),
                };
                let recording = recording::finalize(
                    crate::oplog::OpLogEntry::new(
                        plan.op_name.clone(),
                        plan.repo.display().to_string(),
                        plan.before.clone(),
                        outcome,
                    )
                    .with_actor(policy.actor)
                    .with_worktree(Some(plan.repo.display().to_string())),
                );
                return ConflictReport {
                    recording,
                    evidence: ConflictEvidence {
                        action,
                        progress,
                        before: ConflictObservation {
                            revision: plan.request.revision().clone(),
                            operation: "unknown".into(),
                            paths: plan
                                .request
                                .path()
                                .map(Path::to_path_buf)
                                .into_iter()
                                .collect(),
                        },
                        after,
                        detail: error.to_string(),
                    },
                };
            }
        };
        let before = plan.observed.clone();
        let result = (|| -> Result<(), GitError> {
            backend.require_trust()?;
            if backend.write_worktree_id()? != plan.worktree
                || backend.write_repo_id()? != plan.common_dir
            {
                return Err(GitError::Other(
                    "repository identity changed after planning".into(),
                ));
            }
            let live = observation(&backend.repo)?
                .ok_or_else(|| GitError::Other("the conflict is no longer present".into()))?;
            if live.observation.revision != *plan.request.revision() {
                return Err(GitError::Other(
                    "conflict changed since planning; no files were modified".into(),
                ));
            }
            if fault == Some(ConflictFaultPoint::BeforeMutation) {
                return Err(GitError::Other("injected before conflict mutation".into()));
            }
            match &plan.action {
                ConflictPreparedAction::Save {
                    path,
                    draft,
                    expected_mode,
                } => {
                    if fault == Some(ConflictFaultPoint::AfterWorktreeWrite) {
                        let ConflictDraft::Text(bytes) = draft else {
                            return Err(GitError::Other(
                                "after-worktree fault requires a text draft".into(),
                            ));
                        };
                        let root = backend.repo.workdir().ok_or_else(|| {
                            GitError::Other("repository has no working tree".into())
                        })?;
                        std::fs::write(root.join(path), bytes)
                            .map_err(|e| GitError::Other(e.to_string()))?;
                        progress = ConflictProgress::WorktreeWritten;
                        return Err(GitError::Other(
                            "injected index failure after worktree write".into(),
                        ));
                    }
                    let buffer = ResolutionBuffer::from_conflict_draft(&plan.repo, path, draft)?;
                    conflicts::execute_conflict_save_with_progress(
                        &backend.repo,
                        &buffer,
                        path,
                        |value| progress = value,
                    )?;
                    verify_save(&backend.repo, path, draft, *expected_mode)?;
                }
                ConflictPreparedAction::DirFile(dir_file) => {
                    recovery = Some(ops::apply_dir_file_resolution_with_progress(
                        &backend.repo,
                        dir_file,
                        |value| progress = value,
                    )?);
                    verify_dir_file(&backend.repo, dir_file)?;
                }
                // #704: the partial resolution is preserved by the executor
                // (ADR-0057) from the buffer on disk — the editor autosaves
                // every edit, so this is the same bytes the UI held, and the
                // abort is admissible with no editor open at all.
                ConflictPreparedAction::Abort { session } => {
                    let buffer = backend
                        .resolution_buffer_from_repo_with_autosave()
                        .unwrap_or_else(|_| ResolutionBuffer::new(&plan.repo));
                    let stash = matches!(session.op, conflicts::ConflictOp::StashConflict);
                    let restored = if stash {
                        crate::conflict_abort::execute_stash_conflict_abort_with_progress(
                            &backend.repo,
                            session,
                            &buffer,
                            |value| progress = value,
                        )?
                    } else {
                        crate::conflict_abort::execute_conflict_abort_with_progress(
                            &backend.repo,
                            session,
                            &buffer,
                            |value| progress = value,
                        )?
                    };
                    verify_abort(&backend.repo, &restored)?;
                    recovery = restored
                        .buffer_preserved_at
                        .as_ref()
                        .map(|path| format!("resolution buffer preserved at {}", path.display()));
                }
            }
            progress = ConflictProgress::Verified;
            after = observation(&backend.repo)?.map(|snapshot| snapshot.observation);
            Ok(())
        })();
        if after.is_none() {
            after = observation(&backend.repo)
                .ok()
                .flatten()
                .map(|snapshot| snapshot.observation);
        }
        let detail = result
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_else(|| recovery.clone().unwrap_or_else(|| "verified".into()));
        // #704 / ADR-0196: abort records what the repository *is*, measured
        // after the attempt. `after` above cannot stand in for it — for an
        // abort `None` is the successful outcome, so it says nothing about
        // whether the repository could be read at all.
        let after_state = if fault == Some(ConflictFaultPoint::AbortAfterStateUnreadable) {
            Err(GitError::Other("injected unreadable after-state".into()))
        } else {
            conflicts::current_state_summary(&backend.repo)
        };
        let observed_after = match &plan.request {
            ConflictRequest::Save {
                draft, before_hash, ..
            } => ops::StateSummary {
                head: format!(
                    "staged (stage 0) before={before_hash} after={}",
                    match draft {
                        ConflictDraft::Text(bytes) => short_text_hash(bytes),
                        ConflictDraft::Raw { oid, .. } => oid.chars().take(16).collect(),
                    }
                ),
                dirty: "clean".into(),
            },
            ConflictRequest::ResolveDirFile { path, choice, .. } => ops::StateSummary {
                head: format!("kept {} side of {}", choice.slug(), path.display()),
                dirty: "staged (stage 0)".into(),
            },
            ConflictRequest::Abort { operation, .. } => {
                after_state.clone().unwrap_or_else(|_| ops::StateSummary {
                    head: format!("{operation}: state after the abort is unreadable"),
                    dirty: "unknown".into(),
                })
            }
        };
        // A restore whose result cannot be measured is `Unknown`, never a
        // retryable `Failed`: retrying an abort that may already have moved
        // the ref is the way to lose the commits it detached. `apply` parks a
        // reconcile entry for it (ADR-0196 決定 2).
        let unmeasurable = matches!(plan.request, ConflictRequest::Abort { .. })
            && progress != ConflictProgress::NotStarted
            && after_state.is_err();
        let outcome = match &result {
            _ if unmeasurable => crate::oplog::OpOutcome::Unknown {
                after: observed_after,
                evidence: format!(
                    "{}: the abort had started when the repository became unreadable",
                    detail
                ),
            },
            Ok(()) => crate::oplog::OpOutcome::Success {
                after: observed_after,
            },
            Err(error) if progress != ConflictProgress::NotStarted => {
                crate::oplog::OpOutcome::Partial {
                    after: observed_after,
                    error: error.to_string(),
                }
            }
            Err(error) => crate::oplog::OpOutcome::Refused {
                blockers: vec![error.to_string()],
            },
        };
        let recording = backend.record_run_oplog(&plan.op_name, &plan.before, outcome);
        ConflictReport {
            recording,
            evidence: ConflictEvidence {
                action,
                progress,
                before,
                after,
                detail,
            },
        }
    }

    pub fn continue_blockers(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
    ) -> Vec<conflicts::ContinueBlocker> {
        conflicts::continue_blockers(&self.repo, session, buffer)
    }

    pub fn plan_conflict_continue(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
    ) -> Result<OperationPlan, GitError> {
        conflicts::plan_conflict_continue(&self.repo, session, buffer)
    }

    pub fn plan_conflict_continue_route(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
        current_branch: &str,
    ) -> Result<conflicts::ContinueRoute, GitError> {
        conflicts::plan_conflict_continue_route(&self.repo, session, buffer, current_branch)
    }

    pub fn execute_conflict_continue(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
    ) -> Result<conflicts::ContinueResult, GitError> {
        self.require_trust()?;
        conflicts::execute_conflict_continue(&self.repo, &self.path, session, buffer)
    }

    pub fn execute_conflict_save(
        &self,
        buffer: &ResolutionBuffer,
        path: &Path,
    ) -> Result<conflicts::SaveOutcome, GitError> {
        self.require_trust()?;
        conflicts::execute_conflict_save(&self.repo, buffer, path)
    }

    /// Resolve a directory/file conflict (#320) by keeping one side wholesale,
    /// staging the result into the index and recording it to the oplog. Mirrors
    /// [`Self::execute_conflict_save`]: no commit, the caller re-detects so the
    /// resolved path leaves the conflict set.
    pub fn execute_dir_file_resolution(
        &self,
        path: &Path,
        choice: crate::ops::DirFileChoice,
    ) -> Result<(), GitError> {
        self.require_trust()?;
        let plan = crate::ops::plan_dir_file_resolution(&self.repo, path, choice)?;
        self.execute_planned_dir_file_resolution(&plan)
    }

    /// Execute the reviewed D/F plan without replacing its frozen index facts.
    pub fn execute_planned_dir_file_resolution(
        &self,
        plan: &ops::DirFilePlan,
    ) -> Result<(), GitError> {
        self.require_trust()?;
        ops::execute_dir_file_resolution(&self.repo, &self.path, plan)
    }

    /// Materialize + stage every resolved buffer file (collapsing unmerged index
    /// stages → stage 0) without creating a commit. Used by the UI merge route
    /// before opening the commit panel, so the index carries no unmerged entries
    /// and the staged resolutions are visible to the Commit button.
    pub fn stage_conflict_resolution(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
    ) -> Result<(), GitError> {
        self.require_trust()?;
        conflicts::stage_conflict_resolution(&self.repo, session, buffer)
    }

    pub(crate) fn execute_merge_commit(&self, message: &str) -> Result<CommitId, GitError> {
        conflicts::execute_merge_commit(&self.repo, message)
    }

    pub fn plan_conflict_abort(
        &self,
        session: &conflicts::ConflictSession,
    ) -> Result<OperationPlan, GitError> {
        conflicts::plan_conflict_abort(&self.repo, session)
    }

    /// The abort preview for whatever this repository is in the middle of
    /// (#704). Detects the session itself, so the header operation strip can
    /// open the confirmation with no `ConflictView` and no conflict editor —
    /// the state the issue got stuck in.
    pub fn plan_operation_abort(&self) -> Result<OperationPlan, GitError> {
        let snapshot = observation(&self.repo)?
            .ok_or_else(|| GitError::Other("no operation is in progress".into()))?;
        conflicts::plan_conflict_abort(&self.repo, &snapshot.session)
    }

    pub fn execute_conflict_abort(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
    ) -> Result<conflicts::AbortOutcome, GitError> {
        self.require_trust()?;
        conflicts::execute_conflict_abort(&self.repo, session, buffer)
    }

    /// #309: abort a stash-conflict — restore HEAD for the conflicted paths and
    /// clear their unmerged index entries, leaving the stash entry intact. This
    /// is NOT [`Self::execute_conflict_abort`] (which moves refs via ORIG_HEAD);
    /// a conflicted stash apply writes no ORIG_HEAD / sequencer state.
    pub fn execute_stash_conflict_abort(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
    ) -> Result<conflicts::AbortOutcome, GitError> {
        self.require_trust()?;
        conflicts::execute_stash_conflict_abort(&self.repo, session, buffer)
    }

    pub fn plan_conflict_skip(
        &self,
        session: &conflicts::ConflictSession,
    ) -> Result<OperationPlan, GitError> {
        conflicts::plan_conflict_skip(&self.repo, session)
    }

    pub fn execute_conflict_skip(
        &self,
        session: &conflicts::ConflictSession,
        buffer: &ResolutionBuffer,
    ) -> Result<conflicts::SkipOutcome, GitError> {
        self.require_trust()?;
        conflicts::execute_conflict_skip(&self.repo, session, buffer)
    }
}
