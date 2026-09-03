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
/// After the index surgery the working tree is **reconciled** so it agrees with
/// the index at `path` (#407): the losing namespace is removed from disk and the
/// kept side is written back (a kept symlink file side is written as a symlink,
/// never dereferenced — #298). The two namespaces cannot coexist,
/// so leaving the working tree holding the other side would make `git status`
/// report a spurious deletion + untracked leftover and break a later checkout.
///
/// A savepoint under `refs/kagi/snapshots/` is taken first (#408) — a real commit
/// capturing worktree + index (`add -A`) so every removed blob (including the
/// untracked directory-side children) stays referenced and gc-safe. Its id is
/// the recovery handle recorded in the oplog.
pub fn execute_dir_file_resolution(
    repo: &Repository,
    repo_path: &Path,
    plan: &DirFilePlan,
) -> Result<(), GitError> {
    preflight_dir_file_resolution(repo, plan)?;

    // #408: the resolution drops one namespace from the index and rewrites the
    // working tree (removing the losing side's on-disk files). Take a savepoint
    // FIRST so the operation is recoverable in git's own terms — mirrors the
    // auto-snapshot `Backend::run` takes before any destructive op.
    let recovery = match crate::ops::create_snapshot(
        repo,
        &format!(
            "auto snapshot before conflict-dir-file:{} {}",
            plan.choice.slug(),
            plan.path.display()
        ),
    ) {
        Ok(s) => format!("snapshot={} commit={}", s.id, s.commit),
        Err(e) => {
            // Non-fatal, matching `Backend::run` — a snapshot failure is logged
            // but never blocks the user's operation.
            eprintln!("kagi-git: dir-file snapshot failed (non-fatal): {}", e);
            "snapshot=unavailable".to_string()
        }
    };

    let before = StateSummary {
        head: format!("dir-file conflict {}", plan.path.display()),
        dirty: format!("choice={}; {}", plan.choice.slug(), recovery),
    };
    let op_name = format!("conflict-dir-file:{}", plan.choice.slug());

    let result = apply_to_index(repo, plan).and_then(|()| reconcile_worktree(repo, plan));

    let outcome = match &result {
        Ok(()) => OpOutcome::Success {
            after: StateSummary {
                head: format!(
                    "kept {} side of {}",
                    plan.choice.slug(),
                    plan.path.display()
                ),
                // Recovery handle: the savepoint plus the kept file OID. The
                // dropped directory-side child OIDs are all referenced by the
                // snapshot commit.
                dirty: format!("staged (stage 0); file_oid={}; {}", plan.file_oid, recovery),
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

/// Reconcile the working tree with the index at the resolved path (#407): remove
/// the losing namespace from disk, then materialize the kept side — a direct blob
/// write for the file side, `checkout_index` for the directory children. Without
/// this the index says `path` is a file (or directory) while the working tree
/// still holds the other namespace — a spurious deletion + untracked leftover
/// that a later checkout cannot clear.
///
/// The savepoint taken in [`execute_dir_file_resolution`] already captured the
/// removed content, so the on-disk removal here is recoverable.
fn reconcile_worktree(repo: &Repository, plan: &DirFilePlan) -> Result<(), GitError> {
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("bare repository has no working tree".to_string()))?;
    let abs = workdir.join(&plan.path);
    let on_disk = std::fs::symlink_metadata(&abs).ok();

    match plan.choice {
        DirFileChoice::KeepFile => {
            // Losing side is the directory: remove it if present on disk, then
            // write the kept file directly from its blob. `checkout_index` is
            // unreliable here — HEAD's tree still records `path` as a *tree*, so
            // the checkout sees a D/F clash and skips writing the blob — so the
            // single known file side is written by hand (symlink-safe, #298).
            if on_disk.as_ref().is_some_and(|m| m.is_dir()) {
                std::fs::remove_dir_all(&abs).map_err(|e| {
                    GitError::Other(format!(
                        "reconcile: remove directory {} failed: {}",
                        plan.path.display(),
                        e
                    ))
                })?;
            }
            write_file_side(repo, &abs, plan)
        }
        DirFileChoice::KeepDirectory => {
            // Losing side is the file (regular or symlink): remove it if present,
            // then write the kept directory children from the index.
            if on_disk.as_ref().is_some_and(|m| !m.is_dir()) {
                std::fs::remove_file(&abs).map_err(|e| {
                    GitError::Other(format!(
                        "reconcile: remove file {} failed: {}",
                        plan.path.display(),
                        e
                    ))
                })?;
            }
            checkout_paths(repo, &plan.dir_children)
        }
    }
}

/// Write the kept file side directly to `abs` from its blob, preserving the git
/// mode: a symlink (0o120000) is recreated as a symlink (never dereferenced,
/// #298), an executable (0o100755) keeps its exec bit, everything else is a plain
/// file. Parent dirs are created as needed.
fn write_file_side(repo: &Repository, abs: &Path, plan: &DirFilePlan) -> Result<(), GitError> {
    let blob = repo.find_blob(plan.file_oid).map_err(|e| {
        GitError::Other(format!(
            "reconcile: blob {} lookup failed: {}",
            plan.file_oid,
            e.message()
        ))
    })?;
    let content = blob.content().to_vec();
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            GitError::Other(format!(
                "reconcile: mkdir {} failed: {}",
                parent.display(),
                e
            ))
        })?;
    }

    if plan.file_mode == 0o120000 {
        // Symlink: the blob bytes ARE the link target.
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let target = PathBuf::from(std::ffi::OsStr::from_bytes(&content));
            // Remove any existing node first so symlink() does not fail on EEXIST.
            let _ = std::fs::remove_file(abs);
            std::os::unix::fs::symlink(&target, abs).map_err(|e| {
                GitError::Other(format!(
                    "reconcile: symlink {} failed: {}",
                    abs.display(),
                    e
                ))
            })?;
            return Ok(());
        }
    }

    std::fs::write(abs, &content).map_err(|e| {
        GitError::Other(format!("reconcile: write {} failed: {}", abs.display(), e))
    })?;
    #[cfg(unix)]
    if plan.file_mode == 0o100755 {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(abs, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// `checkout_index` restricted to `paths`, force + `update_index(false)` so the
/// working tree is rewritten from the (already-written) index without touching
/// the staged state — the same shape `ops::discard` uses (discard.rs). Used for
/// the directory side (KeepDirectory), whose children can be many and nested.
fn checkout_paths(repo: &Repository, paths: &[PathBuf]) -> Result<(), GitError> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.force();
    cb.recreate_missing(true);
    cb.update_index(false);
    cb.disable_pathspec_match(true);
    for p in paths {
        cb.path(p);
    }
    repo.checkout_index(None, Some(&mut cb))
        .map_err(|e| GitError::Other(format!("reconcile: checkout_index failed: {}", e.message())))
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
