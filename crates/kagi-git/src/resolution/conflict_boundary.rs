//! Conversion between the mutable resolution buffer and C1's frozen draft.

use super::*;
use kagi_domain::conflict_family::{BufferRevision, ConflictRequest};
use kagi_domain::plan_note::{ConflictsNote, PlanNote};

/// Owned facts from the exact file/payload binding check, not a later read.
pub(crate) struct SaveBinding {
    identity: ConflictRevision,
    context: ConflictRevision,
    git_dir: PathBuf,
}

fn observation_changed() -> GitError {
    GitError::Blocked(Box::new(PlanNote::Conflicts(
        ConflictsNote::ObservationChanged,
    )))
}

fn identity_context(repo: &Repository) -> Result<ConflictRevision, GitError> {
    crate::backend::conflict_ops::conflict_observe::draft_context(repo)
        .ok_or_else(observation_changed)
}

fn current_file_identity(
    index: &git2::Index,
    context: &ConflictRevision,
    git_dir: &Path,
    path: &Path,
) -> Result<ConflictRevision, GitError> {
    let conflict = index.conflict_get(path).map_err(|error| {
        if error.code() == git2::ErrorCode::NotFound {
            observation_changed()
        } else {
            GitError::Other(error.message().to_owned())
        }
    })?;
    Ok(drafts::file_identity(git_dir, context, path, &conflict))
}

impl ResolutionBuffer {
    /// Live write authority is the same exact identity as draft adoption, not
    /// merely a path still present in an older session. The caller reloads the
    /// index before this check; missing/unknown identities never authorize writes.
    pub(crate) fn validate_current_paths<'a>(
        &self,
        repo: &Repository,
        index: &git2::Index,
        paths: impl IntoIterator<Item = &'a Path>,
    ) -> Result<(), GitError> {
        // Even a fully staged session needs a known operation before Continue
        // can write the index/tree or advance the sequencer.
        let context = identity_context(repo)?;
        let git_dir = repo
            .path()
            .canonicalize()
            .map_err(|error| GitError::Other(error.to_string()))?;
        let workdir = repo
            .workdir()
            .ok_or_else(observation_changed)?
            .canonicalize()
            .map_err(|error| GitError::Other(error.to_string()))?;
        let Some((origin_context, origin_git_dir)) = &self.live_context else {
            return Err(observation_changed());
        };
        let buffer_workdir = self
            .repo_path
            .canonicalize()
            .map_err(|error| GitError::Other(error.to_string()))?;
        if origin_context != &context || origin_git_dir != &git_dir || buffer_workdir != workdir {
            return Err(observation_changed());
        }
        for path in paths {
            let current = current_file_identity(index, &context, &git_dir, path)?;
            if self.file_identity(path) != Some(&current) {
                return Err(observation_changed());
            }
            self.validate_worktree(repo, path)?;
        }
        Ok(())
    }

    /// A prepared Save freezes bytes AND their originating file identity.
    /// Never grant old bytes today's identity merely because today's global
    /// observation was supplied to the request factory.
    pub(crate) fn validate_save_binding(
        repo: &Repository,
        path: &Path,
        draft: &kagi_domain::conflict_family::ConflictDraft,
        expected: &BufferRevision,
        worktree_fingerprint: Option<&str>,
    ) -> Result<SaveBinding, GitError> {
        let mut index = repo
            .index()
            .map_err(|error| GitError::Other(error.to_string()))?;
        index
            .read(true)
            .map_err(|error| GitError::Other(error.to_string()))?;
        let context = identity_context(repo)?;
        let git_dir = repo
            .path()
            .canonicalize()
            .map_err(|error| GitError::Other(error.to_string()))?;
        let identity = current_file_identity(&index, &context, &git_dir, path)?;
        if crate::backend::conflict_ops::conflict_observe::buffer_revision(path, draft, &identity)
            != *expected
        {
            return Err(observation_changed());
        }
        super::worktree_state::validate(repo, path, worktree_fingerprint, || match draft {
            kagi_domain::conflict_family::ConflictDraft::Text { bytes, .. } => {
                Some(super::worktree_state::bytes_fingerprint(bytes))
            }
            _ => None,
        })?;
        Ok(SaveBinding {
            identity,
            context,
            git_dir,
        })
    }

    pub fn conflict_action_summary(&self, path: &Path) -> String {
        self.hunk_model(path)
            .map(|model| {
                model
                    .hunks()
                    .iter()
                    .enumerate()
                    .map(|(index, hunk)| {
                        use kagi_domain::resolution::HunkChoice;
                        let choice = match hunk.choice {
                            HunkChoice::Unresolved => "unresolved",
                            HunkChoice::AcceptCurrent => "current",
                            HunkChoice::AcceptIncoming => "incoming",
                            HunkChoice::BothCurrentFirst => "both-cf",
                            HunkChoice::BothIncomingFirst => "both-if",
                            HunkChoice::Manual(_) => "manual",
                        };
                        format!("{index}:{choice}")
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default()
    }

    /// Freeze one file's current resolution draft without repository I/O.
    pub fn conflict_draft(
        &self,
        path: &Path,
    ) -> Option<kagi_domain::conflict_family::ConflictDraft> {
        if let Some(raw) = self.raw_resolution(path) {
            return Some(kagi_domain::conflict_family::ConflictDraft::Raw {
                oid: raw.oid.to_string(),
                mode: raw.mode,
            });
        }
        let file = self.files.get(path)?;
        let lines = file.result.as_ref()?;
        Some(kagi_domain::conflict_family::ConflictDraft::Text {
            bytes: lines_to_text(lines).into_bytes(),
            origins: lines.iter().map(|line| line.origin).collect(),
        })
    }

    /// Rebuild the minimum Save buffer only after the global preflight and the
    /// frozen file-bound payload both match the live repository.
    pub(crate) fn from_conflict_save(
        repo: &Repository,
        request: &ConflictRequest,
    ) -> Result<Self, GitError> {
        let ConflictRequest::Save {
            path,
            draft,
            buffer_revision,
            worktree_fingerprint,
            ..
        } = request
        else {
            return Err(observation_changed());
        };
        let snapshot = crate::backend::conflict_ops::conflict_observe::observation(repo)?
            .ok_or_else(observation_changed)?;
        if &snapshot.observation.revision != request.revision() {
            return Err(observation_changed());
        }
        let binding = Self::validate_save_binding(
            repo,
            path,
            draft,
            buffer_revision,
            worktree_fingerprint.as_deref(),
        )?;
        let repo_path = repo
            .workdir()
            .ok_or_else(|| GitError::Other("repository has no working tree".into()))?;
        let mut buffer = Self::new(repo_path);
        buffer.live_context = Some((binding.context, binding.git_dir));
        let mut file = FileResolution::empty();
        file.identity = Some(binding.identity);
        file.worktree_fingerprint = worktree_fingerprint.clone();
        file.draft_state = true;
        match draft {
            kagi_domain::conflict_family::ConflictDraft::Text { bytes, origins } => {
                let text = std::str::from_utf8(bytes).map_err(|_| {
                    GitError::Other(format!(
                        "resolution for {} is not UTF-8 text",
                        path.display()
                    ))
                })?;
                let mut lines = text_to_lines(text, LineOrigin::Manual);
                if lines.len() != origins.len() {
                    return Err(GitError::Other(
                        "resolution provenance dimensions do not match".into(),
                    ));
                }
                for (line, origin) in lines.iter_mut().zip(origins) {
                    line.origin = *origin;
                }
                file.replace_result(Some(lines), &mut buffer.marker_residue_count);
            }
            kagi_domain::conflict_family::ConflictDraft::Raw { oid, mode } => {
                file.raw = true;
                file.raw_result = Some(RawResolution {
                    oid: git2::Oid::from_str(oid)
                        .map_err(|e| GitError::Other(format!("invalid resolution OID: {e}")))?,
                    mode: *mode,
                });
            }
        }
        buffer.files.insert(path.to_path_buf(), file);
        Ok(buffer)
    }
}
