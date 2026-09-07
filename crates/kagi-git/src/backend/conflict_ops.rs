//! Conflict execution stays behind owner trust, including frozen D/F plans.
use super::*;
use kagi_domain::conflict_family::{
    BufferRevision, ConflictDraft, ConflictEvidence, ConflictObservation, ConflictProgress,
    ConflictRequest, ConflictRevision,
};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug)]
pub struct ConflictSnapshot {
    pub session: conflicts::ConflictSession,
    pub observation: ConflictObservation,
}

#[derive(Clone, Debug)]
enum ConflictPreparedAction {
    Save {
        path: PathBuf,
        draft: ConflictDraft,
        expected_mode: u32,
    },
    DirFile(ops::DirFilePlan),
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
}

#[derive(Clone, Debug)]
pub struct ConflictReport {
    pub recording: recording::Recording,
    pub evidence: ConflictEvidence,
}

fn digest(parts: impl IntoIterator<Item = Vec<u8>>) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    hex::encode(hash.finalize())
}

fn buffer_revision(path: &Path, draft: &ConflictDraft) -> BufferRevision {
    let mut parts = vec![path.as_os_str().as_encoded_bytes().to_vec()];
    match draft {
        ConflictDraft::Text(bytes) => {
            parts.push(b"text".to_vec());
            parts.push(bytes.clone());
        }
        ConflictDraft::Raw { oid, mode } => {
            parts.push(b"raw".to_vec());
            parts.push(oid.as_bytes().to_vec());
            parts.push(mode.to_be_bytes().to_vec());
        }
    }
    BufferRevision::from_fingerprint(digest(parts))
}

fn revision_label(revision: &ConflictRevision) -> String {
    revision.as_str().chars().take(12).collect()
}

fn short_text_hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn observation(repo: &Repository) -> Result<Option<ConflictSnapshot>, GitError> {
    let Some(session) = conflicts::detect_conflict_session(repo) else {
        return Ok(None);
    };
    let mut parts = Vec::new();
    parts.push(format!("{:?}", repo.state()).into_bytes());
    parts.push(format!("{:?}", session.op).into_bytes());
    if let Ok(head) = repo.head() {
        parts.push(head.name_bytes().to_vec());
        parts.push(
            head.target()
                .map(|oid| oid.to_string())
                .unwrap_or_default()
                .into_bytes(),
        );
    }
    let mut entries: Vec<Vec<u8>> = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?
        .iter()
        .map(|entry| {
            let mut value = entry.path;
            value.extend_from_slice(&entry.mode.to_be_bytes());
            value.extend_from_slice(&entry.flags.to_be_bytes());
            value.extend_from_slice(entry.id.as_bytes());
            value
        })
        .collect();
    entries.sort();
    parts.extend(entries);
    let git_dir = repo.path();
    for relative in [
        "MERGE_HEAD",
        "REBASE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge/done",
        "rebase-merge/git-rebase-todo",
        "rebase-merge/msgnum",
        "rebase-merge/end",
        "rebase-apply/next",
        "rebase-apply/last",
    ] {
        let path = git_dir.join(relative);
        if let Ok(bytes) = std::fs::read(path) {
            parts.push(relative.as_bytes().to_vec());
            parts.push(bytes);
        }
    }
    let revision = ConflictRevision::from_fingerprint(digest(parts));
    let paths = session.files.iter().map(|file| file.path.clone()).collect();
    Ok(Some(ConflictSnapshot {
        observation: ConflictObservation {
            revision,
            operation: session.op.slug().into(),
            paths,
        },
        session,
    }))
}

fn text_mode(repo: &Repository, path: &Path) -> u32 {
    let executable = repo
        .workdir()
        .and_then(|root| std::fs::symlink_metadata(root.join(path)).ok())
        .map(|metadata| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                let _ = metadata;
                false
            }
        })
        .unwrap_or(false);
    if executable {
        0o100755
    } else {
        0o100644
    }
}

fn verify_save(
    repo: &Repository,
    path: &Path,
    draft: &ConflictDraft,
    expected_mode: u32,
) -> Result<(), GitError> {
    let index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
    if index.get_path(path, 1).is_some()
        || index.get_path(path, 2).is_some()
        || index.get_path(path, 3).is_some()
    {
        return Err(GitError::Other(format!(
            "{} remains unmerged after save",
            path.display()
        )));
    }
    let entry = index
        .get_path(path, 0)
        .ok_or_else(|| GitError::Other(format!("{} was not staged", path.display())))?;
    match draft {
        ConflictDraft::Text(bytes) => {
            let root = repo
                .workdir()
                .ok_or_else(|| GitError::Other("repository has no working tree".into()))?;
            let actual = std::fs::read(root.join(path))
                .map_err(|e| GitError::Other(format!("verify {} failed: {e}", path.display())))?;
            let expected_oid = git2::Oid::hash_object(git2::ObjectType::Blob, bytes)
                .map_err(|e| GitError::Other(e.to_string()))?;
            if actual != *bytes || entry.id != expected_oid || entry.mode != expected_mode {
                return Err(GitError::Other(format!(
                    "{} bytes, blob, or mode differ after save",
                    path.display()
                )));
            }
        }
        ConflictDraft::Raw { oid, mode } => {
            if entry.id.to_string() != *oid || entry.mode != *mode {
                return Err(GitError::Other(format!(
                    "{} raw OID or mode differs after save",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

fn verify_dir_file(repo: &Repository, plan: &ops::DirFilePlan) -> Result<(), GitError> {
    let index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
    if index.get_path(&plan.path, 1).is_some()
        || index.get_path(&plan.path, 2).is_some()
        || index.get_path(&plan.path, 3).is_some()
    {
        return Err(GitError::Other(
            "directory/file entry remains unmerged".into(),
        ));
    }
    match plan.choice {
        ops::DirFileChoice::KeepFile => {
            let entry = index
                .get_path(&plan.path, 0)
                .ok_or_else(|| GitError::Other("kept file is absent from index".into()))?;
            if entry.id != plan.file_oid || entry.mode != plan.file_mode {
                return Err(GitError::Other("kept file identity differs".into()));
            }
            if plan
                .dir_children
                .iter()
                .any(|path| index.get_path(path, 0).is_some())
            {
                return Err(GitError::Other("directory children remain in index".into()));
            }
        }
        ops::DirFileChoice::KeepDirectory => {
            if index.get_path(&plan.path, 0).is_some()
                || plan
                    .dir_children
                    .iter()
                    .any(|path| index.get_path(path, 0).is_none())
            {
                return Err(GitError::Other("kept directory index shape differs".into()));
            }
        }
    }
    Ok(())
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
        };
        recording::finalize(
            crate::oplog::OpLogEntry::new(
                op_name,
                path.display().to_string(),
                ops::StateSummary {
                    head: format!("conflict revision {}", revision_label(request.revision())),
                    dirty: format!("{} {}", request.action().name(), request.path().display()),
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
                    paths: vec![plan.request.path().to_path_buf()],
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
        };
        let op_name = match &request {
            ConflictRequest::Save { operation, .. } => format!("conflict-save:{operation}"),
            ConflictRequest::ResolveDirFile { choice, .. } => {
                format!("conflict-dir-file:{}", choice.slug())
            }
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
                            paths: vec![plan.request.path().to_path_buf()],
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
        };
        let outcome = match &result {
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
