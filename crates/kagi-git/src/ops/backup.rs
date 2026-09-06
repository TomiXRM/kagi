//! Mandatory discard/remove recovery roots, independent of snapshot caps (#523).
use crate::{DiscardBackup, GitError};
use git2::Repository;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const PREFIX: &str = "refs/kagi/backups/";

/// Attempt identity, unrelated to the oplog's post-execution sequence number.
/// Ref creation is exclusive: even a clock/PID collision fails before deletion.
pub(crate) fn operation_id() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{nanos:x}-{:x}-{:x}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

pub(crate) fn write_blob(
    repo: &Repository,
    operation_id: &str,
    index: usize,
    path: String,
    content: &[u8],
) -> Result<DiscardBackup, GitError> {
    let oid = repo.blob(content).map_err(io)?;
    // Ordinals avoid interpreting filenames as ref paths or revision syntax.
    let reference = format!("{PREFIX}{operation_id}/{index}");
    repo.reference(&reference, oid, false, "kagi mandatory recovery backup")
        .map_err(io)?;
    // Never unlink on Drop: partial mutation, unwind and failed recording must
    // retain their recovery roots. Explicit oplog retirement owns cleanup.
    Ok(DiscardBackup {
        path,
        blob: oid.to_string(),
        reference,
    })
}

pub(crate) fn validate_reference(reference: &str) -> Result<(), GitError> {
    if !reference.starts_with(PREFIX) || !git2::Reference::is_valid_name(reference) {
        return Err(GitError::Other("not a Kagi backup reference".into()));
    }
    Ok(())
}

pub(crate) fn read_blob(repo: &Repository, reference: &str) -> Result<Vec<u8>, GitError> {
    validate_reference(reference)?;
    // Exact ref lookup, not revparse: never accept a bare OID or revision suffix.
    let object = repo
        .find_reference(reference)
        .map_err(io)?
        .peel(git2::ObjectType::Blob)
        .map_err(io)?;
    let blob = object
        .as_blob()
        .ok_or_else(|| GitError::Other("backup is not a blob".into()))?;
    Ok(blob.content().to_vec())
}

fn io(error: impl std::fmt::Display) -> GitError {
    GitError::Other(format!("backup: {error}"))
}
