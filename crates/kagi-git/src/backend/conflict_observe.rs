//! The single observation of "is this repository in the middle of something?"
//!
//! One detector (`conflicts::detect_conflict_session`), one fingerprint. Every
//! consumer reads this: the conflict family freezes [`ConflictRevision`] into
//! its requests and re-reads it live at preflight, and the per-tab read model
//! carries the pure [`InProgressOperation`] the header strip renders (#704 /
//! ADR-0196). Split out of `conflict_ops.rs` so both sides name the same
//! function rather than growing a second implementation over the same `.git`
//! files.

use super::*;
use kagi_domain::conflict_family::{
    BufferRevision, ConflictDraft, ConflictObservation, ConflictRevision, InProgressOperation,
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
    pub fn in_progress(&self) -> InProgressOperation {
        InProgressOperation {
            slug: self.session.op.slug().to_string(),
            step: match self.session.op {
                conflicts::ConflictOp::Rebase { step, total, .. } => Some((step, total)),
                _ => None,
            },
            unmerged: self.session.files.len(),
            revision: self.observation.revision.clone(),
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
