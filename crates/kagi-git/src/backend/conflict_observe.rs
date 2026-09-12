//! The single observation of "is this repository in the middle of something?"
//!
//! One detector (`conflicts::detect_conflict_session`), one fingerprint. Every
//! consumer reads this: the conflict family freezes [`ConflictRevision`] into
//! its requests and re-reads it live at preflight, and the per-tab read model
//! carries the pure [`ObservedOperation`] the header strip renders (#704 /
//! ADR-0196). Split out of `conflict_ops.rs` so both sides name the same
//! function rather than growing a second implementation over the same `.git`
//! files.

use crate::backend::*;
use kagi_domain::conflict_family::{
    BufferRevision, ConflictDraft, ConflictObservation, ConflictOperationKind, ConflictRevision,
    ObservedOperation,
};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug)]
pub struct ConflictSnapshot {
    pub session: conflicts::ConflictSession,
    pub observation: ConflictObservation,
}

impl ConflictSnapshot {
    /// The pure value the per-tab read model carries (#704). Everything the
    /// header strip needs to name the operation, show its progress and freeze
    /// an abort request — and nothing that would tempt the UI to re-derive the
    /// observation for itself.
    pub fn in_progress(&self) -> ObservedOperation {
        use kagi_domain::plan_note::InProgressOp;
        ObservedOperation {
            kind: match self.session.op {
                conflicts::ConflictOp::Merge { .. } => {
                    ConflictOperationKind::Repository(InProgressOp::Merge)
                }
                conflicts::ConflictOp::Rebase { .. } => {
                    ConflictOperationKind::Repository(InProgressOp::Rebase)
                }
                conflicts::ConflictOp::CherryPick { .. } => {
                    ConflictOperationKind::Repository(InProgressOp::CherryPick)
                }
                conflicts::ConflictOp::Revert { .. } => {
                    ConflictOperationKind::Repository(InProgressOp::Revert)
                }
                conflicts::ConflictOp::StashConflict => ConflictOperationKind::StashApply,
            },
            observation: self.observation.clone(),
            step: match self.session.op {
                conflicts::ConflictOp::Rebase { step, total, .. } => Some((step, total)),
                _ => None,
            },
        }
    }
}

pub(crate) fn digest(parts: impl IntoIterator<Item = Vec<u8>>) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    hex::encode(hash.finalize())
}

pub(crate) fn buffer_revision(path: &Path, draft: &ConflictDraft) -> BufferRevision {
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

pub(crate) fn revision_label(revision: &ConflictRevision) -> String {
    revision.as_str().chars().take(12).collect()
}

pub(crate) fn short_text_hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub(crate) fn observation(repo: &Repository) -> Result<Option<ConflictSnapshot>, GitError> {
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
pub(crate) fn text_mode(repo: &Repository, path: &Path) -> u32 {
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

pub(crate) fn verify_save(
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

pub(crate) fn verify_dir_file(repo: &Repository, plan: &ops::DirFilePlan) -> Result<(), GitError> {
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

/// #704 / ADR-0196: measure the abort rather than trusting it. The operation
/// state has to be gone, and HEAD has to stand where the restore said it put
/// it — a `cleanup_state` that silently left `MERGE_HEAD` behind is exactly
/// the dead end this issue is about, and must not be recorded as a success.
pub(crate) fn verify_abort(
    repo: &Repository,
    restored: &conflicts::AbortOutcome,
) -> Result<(), GitError> {
    if let Some(live) = observation(repo)? {
        return Err(GitError::Other(format!(
            "{} is still in progress after the abort",
            live.observation.operation
        )));
    }
    let Some(target) = &restored.restored_to else {
        return Ok(());
    };
    let head = repo
        .head()
        .ok()
        .and_then(|head| head.target())
        .map(|oid| oid.to_string());
    if head.as_deref() != Some(target.as_str()) {
        return Err(GitError::Other(format!(
            "HEAD is {} after the abort, not the restored {target}",
            head.unwrap_or_else(|| "unresolvable".into())
        )));
    }
    Ok(())
}
