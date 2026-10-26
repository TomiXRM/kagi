//! Explicit absence resolution. The recorded conflict boundary owns the receipt.
use std::path::{Component, Path, PathBuf};

use git2::{Oid, Repository};
use kagi_domain::conflict_family::ConflictProgress;

use super::conflict_index::{ConflictIndexTransaction, IndexIdentity};
use crate::GitError;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    oid: Oid,
    mode: u32,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64, u32),
}

#[derive(Clone, Debug)]
pub(crate) struct ConflictDeletePlan {
    pub path: PathBuf,
    pub removal_paths: Vec<PathBuf>,
    stages: [Option<(Oid, u32)>; 3],
    target: Option<Target>,
    unaffected: IndexIdentity,
}

fn error(message: &str) -> GitError {
    GitError::Other(message.into())
}

// Never traverse a symlink parent, including one pointing within the worktree.
fn target_path(repo: &Repository, path: &Path) -> Result<PathBuf, GitError> {
    let mut full = repo
        .workdir()
        .ok_or_else(|| error("repository has no working tree"))?
        .to_path_buf();
    let mut components = path.components().peekable();
    if components.peek().is_none() {
        return Err(error("empty deletion path"));
    }
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(error("deletion path must be repository relative"));
        };
        if name.as_encoded_bytes().eq_ignore_ascii_case(b".git") {
            return Err(error("cannot delete repository metadata"));
        }
        full.push(name);
        if components.peek().is_some() {
            match std::fs::symlink_metadata(&full) {
                Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(error("deletion parent is not a real directory")),
            }
        }
    }
    Ok(full)
}

fn target(repo: &Repository, path: &Path) -> Result<Option<Target>, GitError> {
    let full = target_path(repo, path)?;
    let meta = match std::fs::symlink_metadata(&full) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(&e.to_string())),
    };
    let (oid, mode) = if meta.file_type().is_symlink() {
        let link = std::fs::read_link(full).map_err(|e| error(&e.to_string()))?;
        (
            Oid::hash_object(git2::ObjectType::Blob, link.as_os_str().as_encoded_bytes())
                .map_err(|e| error(&e.to_string()))?,
            0o120000,
        )
    } else if meta.is_file() {
        #[cfg(unix)]
        let executable = {
            use std::os::unix::fs::PermissionsExt;
            meta.permissions().mode() & 0o111 != 0
        };
        #[cfg(not(unix))]
        let executable = false;
        (
            Oid::hash_file(git2::ObjectType::Blob, &full).map_err(|e| error(&e.to_string()))?,
            if executable { 0o100755 } else { 0o100644 },
        )
    } else {
        return Err(error("deletion refuses directories and special files"));
    };
    Ok(Some(Target {
        oid,
        mode,
        modified: meta.modified().ok(),
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            (
                meta.dev(),
                meta.ino(),
                meta.ctime(),
                meta.ctime_nsec(),
                meta.mode(),
            )
        },
    }))
}

pub(crate) fn plan_conflict_delete(
    repo: &Repository,
    path: &Path,
) -> Result<ConflictDeletePlan, GitError> {
    let mut index = repo.index().map_err(|e| error(&e.to_string()))?;
    index.read(true).map_err(|e| error(&e.to_string()))?;
    let conflict = index
        .conflict_get(path)
        .map_err(|e| error(&e.to_string()))?;
    if conflict.our.is_some() && conflict.their.is_some() {
        return Err(error("deletion is not a route for a two-sided conflict"));
    }
    let stages = [conflict.ancestor, conflict.our, conflict.their]
        .map(|entry| entry.map(|entry| (entry.id, entry.mode)));
    if stages
        .iter()
        .flatten()
        .any(|(_, mode)| !matches!(*mode, 0o100644 | 0o100755 | 0o120000))
    {
        return Err(error("deletion refuses unsupported conflict modes"));
    }
    if index.get_path(path, 0).is_some() {
        return Err(error("deletion refuses a staged replacement"));
    }
    let target = target(repo, path)?;
    // A DD path has no surviving side: only an exact ancestor artifact is
    // owned. Never infer ownership from a suffix, filename, or mere presence.
    if stages[1].is_none()
        && stages[2].is_none()
        && target
            .as_ref()
            .is_some_and(|value| stages[0] != Some((value.oid, value.mode)))
    {
        return Err(error(
            "both-deleted path contains an unrelated worktree artifact",
        ));
    }
    Ok(ConflictDeletePlan {
        path: path.to_path_buf(),
        removal_paths: if target.is_some() {
            vec![path.to_path_buf()]
        } else {
            Vec::new()
        },
        stages,
        target,
        unaffected: IndexIdentity::capture(&index, |p| p == path.as_os_str().as_encoded_bytes()),
    })
}

pub(crate) fn preflight_conflict_delete(
    repo: &Repository,
    plan: &ConflictDeletePlan,
) -> Result<(), GitError> {
    let live = plan_conflict_delete(repo, &plan.path)?;
    if live.stages != plan.stages
        || live.target != plan.target
        || live.removal_paths != plan.removal_paths
        || live.unaffected != plan.unaffected
    {
        return Err(error("deletion targets changed after approval"));
    }
    Ok(())
}

pub(crate) fn execute_conflict_delete(
    repo: &Repository,
    plan: &ConflictDeletePlan,
    mut progress: impl FnMut(ConflictProgress) -> Result<(), GitError>,
) -> Result<String, GitError> {
    let _lock = crate::write_lock::acquire(repo)?;
    preflight_conflict_delete(repo, plan)?;
    let snapshot = super::create_snapshot(
        repo,
        &format!(
            "auto snapshot before conflict-delete {}",
            plan.path.display()
        ),
    )
    .map_err(|_| super::conflict_abort::draft_preservation_failed())?;
    progress(ConflictProgress::RecoveryCaptured)?;
    // Snapshot acquisition is not permission to overwrite a concurrent edit.
    let transaction = ConflictIndexTransaction::acquire(repo)?;
    preflight_conflict_delete(repo, plan)?;
    for path in &plan.removal_paths {
        std::fs::remove_file(target_path(repo, path)?).map_err(|e| error(&e.to_string()))?;
        progress(ConflictProgress::WorktreeWritten)?;
    }
    let mut index = transaction.read()?;
    index
        .conflict_remove(&plan.path)
        .map_err(|e| error(&e.to_string()))?;
    transaction.write(&mut index)?;
    progress(ConflictProgress::IndexAndWorktreeWritten)?;
    verify_conflict_delete(repo, plan)?;
    Ok(format!(
        "snapshot={} commit={}",
        snapshot.id, snapshot.commit
    ))
}

pub(crate) fn verify_conflict_delete(
    repo: &Repository,
    plan: &ConflictDeletePlan,
) -> Result<(), GitError> {
    let mut index = repo.index().map_err(|e| error(&e.to_string()))?;
    index.read(true).map_err(|e| error(&e.to_string()))?;
    if IndexIdentity::capture(&index, |p| p == plan.path.as_os_str().as_encoded_bytes())
        != plan.unaffected
    {
        return Err(error("unaffected index changed during deletion"));
    }
    if (0..=3).any(|stage| index.get_path(&plan.path, stage).is_some())
        || target(repo, &plan.path)?.is_some()
    {
        return Err(error("deleted conflict remains in index or worktree"));
    }
    Ok(())
}
