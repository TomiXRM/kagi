//! Directory/file conflict resolution (#320 / ADR-0164).
//!
//! A directory/file ("D/F") conflict is a path that one side committed as a
//! **file** and the other as a **directory** (entries under `path/`). libgit2's
//! `repo.merge` records it (both merge directions, verified) as:
//!
//! - a **one-sided** unmerged entry at `path` — the file that lost the race
//!   (stage 2 if ours, stage 3 if theirs), and
//! - clean **stage-0** entries under `path/…` — the directory side.
//!
//! The two namespaces cannot coexist in one tree, so there is no text merge:
//! resolution is a single binary choice ([`DirFileChoice`]). This module owns
//! the `plan_ / preflight_ / execute_` triple (CLAUDE.md invariant #4). Because
//! a D/F resolution is a conflict-lane operation (like `conflict-save`, it stages
//! into the index and is re-detected away) rather than a [`crate::Backend::run`]
//! `Operation`, `execute_` writes its own oplog entry — the single record for the
//! op — so no write path is unlogged.

use std::path::{Path, PathBuf};

use git2::Repository;

use crate::conflicts::path_to_index_bytes;
use crate::oplog::{append_oplog, OpLogEntry, OpOutcome};
use crate::ops::StateSummary;
use crate::GitError;

pub use kagi_domain::resolution::DirFileChoice;

/// The concrete plan for resolving one directory/file conflict: which side to
/// keep, plus the index facts needed to apply it. Built by
/// [`plan_dir_file_resolution`] from the live index.
#[derive(Debug, Clone)]
pub struct DirFilePlan {
    /// Repository-relative conflicting path.
    pub path: PathBuf,
    /// Which side survives.
    pub choice: DirFileChoice,
    /// The file side's blob OID + git mode (the present unmerged stage). Staged
    /// at stage 0 when keeping the file.
    pub file_oid: git2::Oid,
    /// The file side's git mode (0o100644 / 0o100755 / 0o120000 / …), preserved.
    pub file_mode: u32,
    /// The clean `path/…` child entries (the directory side). Removed when
    /// keeping the file.
    pub dir_children: Vec<PathBuf>,
}

/// Inspect the live index and build a resolution plan for the D/F conflict at
/// `path`, or fail if `path` is not a directory/file conflict.
pub fn plan_dir_file_resolution(
    repo: &Repository,
    path: &Path,
    choice: DirFileChoice,
) -> Result<DirFilePlan, GitError> {
    let index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;

    // The one present side of the D/F file conflict (stage 2 our, or stage 3
    // their). `get_path` returns whichever stage matches; try both.
    let (file_oid, file_mode) = index
        .get_path(path, 2)
        .or_else(|| index.get_path(path, 3))
        .map(|e| (e.id, e.mode))
        .ok_or_else(|| {
            GitError::Other(format!(
                "{} is not a directory/file conflict (no unmerged file side)",
                path.display()
            ))
        })?;

    // The directory side: clean children under `path/`.
    let mut prefix = path_to_index_bytes(path)
        .ok_or_else(|| GitError::Other(format!("non-representable path {}", path.display())))?;
    prefix.push(b'/');
    let dir_children: Vec<PathBuf> = index
        .iter()
        .filter(|e| e.path.starts_with(&prefix))
        .filter_map(|e| bytes_to_pathbuf(&e.path))
        .collect();

    if dir_children.is_empty() {
        return Err(GitError::Other(format!(
            "{} is not a directory/file conflict (no directory side)",
            path.display()
        )));
    }

    Ok(DirFilePlan {
        path: path.to_path_buf(),
        choice,
        file_oid,
        file_mode,
        dir_children,
    })
}

/// Re-verify the plan against the current index just before executing: the file
/// side must still be unmerged and the directory side must still be present.
/// (TOCTOU guard — mirrors the preflight step of the `Backend::run` path.)
pub fn preflight_dir_file_resolution(
    repo: &Repository,
    plan: &DirFilePlan,
) -> Result<(), GitError> {
    let fresh = plan_dir_file_resolution(repo, &plan.path, plan.choice)?;
    if fresh.file_oid != plan.file_oid || fresh.file_mode != plan.file_mode {
        return Err(GitError::Other(format!(
            "{} changed since it was planned — re-plan before executing",
            plan.path.display()
        )));
    }
    Ok(())
}

/// Apply a directory/file resolution to the index and record it to the oplog.
///
/// - **KeepDirectory**: drop the unmerged file entry; the clean `path/…`
///   children remain, so the resulting tree keeps the directory.
/// - **KeepFile**: remove every `path/…` child and the unmerged file entry, then
///   stage the file blob (OID + mode) at stage 0, so the tree keeps the file.
///
/// No working-tree write happens for the index surgery (a kept symlink file side
/// is never dereferenced, #298); the checkout of the resolved path is left to the
/// caller's re-detection / continue, exactly as `conflict-save` does.
pub fn execute_dir_file_resolution(
    repo: &Repository,
    repo_path: &Path,
    plan: &DirFilePlan,
) -> Result<(), GitError> {
    preflight_dir_file_resolution(repo, plan)?;

    let before = StateSummary {
        head: format!("dir-file conflict {}", plan.path.display()),
        dirty: format!("choice={}", plan.choice.slug()),
    };
    let op_name = format!("conflict-dir-file:{}", plan.choice.slug());

    let result = apply_to_index(repo, plan);

    let outcome = match &result {
        Ok(()) => OpOutcome::Success {
            after: StateSummary {
                head: format!(
                    "kept {} side of {}",
                    plan.choice.slug(),
                    plan.path.display()
                ),
                dirty: "staged (stage 0)".to_string(),
            },
        },
        Err(e) => OpOutcome::Failed {
            error: e.to_string(),
        },
    };
    let entry = OpLogEntry::new(op_name, repo_path.display().to_string(), before, outcome);
    if let Err(e) = append_oplog(&entry) {
        eprintln!("kagi-git: dir-file oplog write failed (non-fatal): {}", e);
    }

    result
}

/// The index surgery, kept separate so [`execute_dir_file_resolution`] can log
/// both success and failure around it.
fn apply_to_index(repo: &Repository, plan: &DirFilePlan) -> Result<(), GitError> {
    let mut index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;

    match plan.choice {
        DirFileChoice::KeepDirectory => {
            // Drop the unmerged file entry; the stage-0 `path/…` children stay.
            index.conflict_remove(&plan.path).map_err(|e| {
                GitError::Other(format!(
                    "clear conflict {} failed: {}",
                    plan.path.display(),
                    e.message()
                ))
            })?;
        }
        DirFileChoice::KeepFile => {
            // Remove the directory side, then stage the file blob at stage 0.
            index.remove_dir(&plan.path, 0).map_err(|e| {
                GitError::Other(format!(
                    "remove directory {} failed: {}",
                    plan.path.display(),
                    e.message()
                ))
            })?;
            index.conflict_remove(&plan.path).map_err(|e| {
                GitError::Other(format!(
                    "clear conflict {} failed: {}",
                    plan.path.display(),
                    e.message()
                ))
            })?;
            let path_bytes = path_to_index_bytes(&plan.path).ok_or_else(|| {
                GitError::Other(format!("non-representable path {}", plan.path.display()))
            })?;
            let entry = git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: plan.file_mode,
                uid: 0,
                gid: 0,
                file_size: 0,
                id: plan.file_oid,
                flags: 0,
                flags_extended: 0,
                path: path_bytes,
            };
            index.add(&entry).map_err(|e| {
                GitError::Other(format!(
                    "stage {} failed: {}",
                    plan.path.display(),
                    e.message()
                ))
            })?;
        }
    }

    index
        .write()
        .map_err(|e| GitError::Other(format!("index.write() failed: {}", e.message())))
}

/// Local byte→path (Unix-faithful) copy for reading child entry paths.
fn bytes_to_pathbuf(bytes: &[u8]) -> Option<PathBuf> {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Some(PathBuf::from(s));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
    }
    #[cfg(not(unix))]
    {
        None
    }
}
