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
    let reference = retain_object(repo, operation_id, index, oid)?;
    // Never unlink on Drop: partial mutation, unwind and failed recording must
    // retain their recovery roots. Explicit oplog retirement owns cleanup.
    Ok(DiscardBackup {
        path,
        blob: oid.to_string(),
        reference,
    })
}

/// Discard recovery retains the directory-entry type alongside its content.
/// A fixed name avoids interpreting the receipt's literal path as tree syntax.
pub(crate) fn write_file(
    repo: &Repository,
    operation_id: &str,
    index: usize,
    path: String,
    content: &[u8],
    mode: i32,
) -> Result<DiscardBackup, GitError> {
    let oid = repo.blob(content).map_err(io)?;
    let mut tree = repo.treebuilder(None).map_err(io)?;
    tree.insert("file", oid, mode).map_err(io)?;
    let root = tree.write().map_err(io)?;
    let reference = retain_object(repo, operation_id, index, root)?;
    Ok(DiscardBackup {
        path,
        blob: oid.to_string(),
        reference,
    })
}

/// Pin a blob, tree, commit or tag object before mutation; the receipt owns its retention.
pub(crate) fn retain_object(
    repo: &Repository,
    operation_id: &str,
    index: usize,
    oid: git2::Oid,
) -> Result<String, GitError> {
    let reference = format!("{PREFIX}{operation_id}/{index}");
    repo.reference(&reference, oid, false, "kagi mandatory recovery backup")
        .map_err(io)?;
    Ok(reference)
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
        .peel(git2::ObjectType::Any)
        .map_err(io)?;
    // Legacy discard and other families retain a direct blob root. New discard
    // roots are one-entry trees; never confuse arbitrary branch/tag trees with
    // this format or accept a nested tree/gitlink as file bytes.
    let oid = if let Some(tree) = object.as_tree() {
        let entry = tree
            .get_name("file")
            .filter(|entry| {
                tree.len() == 1 && matches!(entry.filemode(), 0o100644 | 0o100755 | 0o120000)
            })
            .ok_or_else(|| GitError::Other("backup: invalid file backup tree".into()))?;
        entry.id()
    } else if object.as_blob().is_some() {
        object.id()
    } else {
        return Err(GitError::Other("backup is not a file backup".into()));
    };
    let blob = repo.find_blob(oid).map_err(io)?;
    Ok(blob.content().to_vec())
}

fn io(error: impl std::fmt::Display) -> GitError {
    GitError::Other(format!("backup: {error}"))
}
