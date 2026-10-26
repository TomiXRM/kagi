//! Directory/file conflict resolution (#320 / ADR-0164).
//!
//! Keep this module's `plan_*`, `preflight_*`, and `execute_*` checks aligned:
//! drift can replace the file namespace when the confirmed plan chose the
//! directory namespace, or vice versa.
//!
//! A directory/file ("D/F") conflict is a path that one side committed as a
//! **file** and the other as a **directory** (entries under `path/`). libgit2's
//! `repo.merge` records it (both merge directions, verified) as:
//!
//! - a **one-sided** unmerged entry at `path` — the file that lost the race
//!   (stage 2 if ours, stage 3 if theirs), and
//! - clean **stage-0** entries under `path/…` — the directory side.
//!
//! CLI Git may instead park the file at `path~<side>` and leave unmerged
//! directory children. Side-tree identities attribute that parking path to the
//! original namespace; the suffix alone never authorizes grouping or removal.
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

use super::conflict_index::{ConflictIndexTransaction, IndexIdentity};
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
    /// The surviving `path/…` directory children, including children that must
    /// be promoted from an unmerged stage. Removed when keeping the file.
    pub dir_children: Vec<PathBuf>,
    /// Actual index file-side path, including a positively identified Git parking suffix.
    pub file_path: PathBuf,
    /// Exact existing worktree nodes approved for removal, children before parents.
    pub removal_paths: Vec<PathBuf>,
    /// Complete directory index identities, including ancestor and unmerged stages.
    pub child_entries: Vec<DirFileEntry>,
    /// Directory identities to stage when keeping the directory.
    pub kept_children: Vec<DirFileEntry>,
    file_stage: u16,
    worktree: Vec<(PathBuf, u32, git2::Oid)>,
    unaffected: IndexIdentity,
}

impl DirFilePlan {
    /// Approved directory removals, derived only from the captured namespace.
    pub fn removed_directories(&self) -> Vec<PathBuf> {
        self.removal_paths
            .iter()
            .filter(|path| {
                self.worktree
                    .iter()
                    .any(|(p, mode, _)| p == *path && *mode == 0o040000)
            })
            .cloned()
            .collect()
    }

    fn affects(&self, bytes: &[u8]) -> bool {
        let original = self.path.as_os_str().as_encoded_bytes();
        bytes == self.file_path.as_os_str().as_encoded_bytes()
            || bytes == original
            || (bytes.starts_with(original) && bytes.get(original.len()) == Some(&b'/'))
    }
}

/// Captured index identity; stat-cache changes do not invalidate approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirFileEntry {
    pub path: PathBuf,
    pub stage: u16,
    pub oid: git2::Oid,
    pub mode: u32,
}

/// Identify Git's parked file only with both index and side-tree evidence.
/// Suffix spelling is merely a candidate generator, never ownership evidence.
pub(crate) fn parked_dir_file_paths(
    repo: &Repository,
    index: &git2::Index,
) -> Vec<(PathBuf, PathBuf)> {
    let Some(current) = repo.head().ok().and_then(|h| h.peel_to_tree().ok()) else {
        return Vec::new();
    };
    let incoming = ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REBASE_HEAD"]
        .into_iter()
        .find_map(|name| {
            repo.revparse_single(name)
                .ok()
                .and_then(|o| o.peel_to_tree().ok())
        });
    let Some(incoming) = incoming else {
        return Vec::new();
    };
    let mut mappings = Vec::new();
    for entry in index
        .iter()
        .filter(|e| matches!((e.flags >> 12) & 3, 2 | 3))
    {
        let stage = (entry.flags >> 12) & 3;
        let Some(parked) = bytes_to_pathbuf(&entry.path) else {
            continue;
        };
        if index.get_path(&parked, 1).is_some()
            || index.get_path(&parked, 5 - stage as i32).is_some()
            || current.get_path(&parked).is_ok()
            || incoming.get_path(&parked).is_ok()
        {
            continue;
        }
        let (file_tree, dir_tree) = if stage == 2 {
            (&current, &incoming)
        } else {
            (&incoming, &current)
        };
        for (offset, byte) in entry.path.iter().enumerate() {
            if *byte != b'~' || offset + 1 == entry.path.len() {
                continue;
            }
            let Some(original) = bytes_to_pathbuf(&entry.path[..offset]) else {
                continue;
            };
            let Ok(file) = file_tree.get_path(&original) else {
                continue;
            };
            let Ok(directory) = dir_tree.get_path(&original) else {
                continue;
            };
            if file.id() != entry.id
                || file.filemode() as u32 != entry.mode
                || directory.kind() != Some(git2::ObjectType::Tree)
            {
                continue;
            }
            let mut prefix = entry.path[..offset].to_vec();
            prefix.push(b'/');
            let children: Vec<_> = index
                .iter()
                .filter(|e| e.path.starts_with(&prefix))
                .collect();
            if !children.is_empty()
                && children.iter().all(|child| {
                    let child_stage = (child.flags >> 12) & 3;
                    child_stage == 1
                        || ((child_stage == 0 || child_stage == 5 - stage)
                            && bytes_to_pathbuf(&child.path)
                                .and_then(|p| dir_tree.get_path(&p).ok())
                                .is_some_and(|e| {
                                    e.id() == child.id && e.filemode() as u32 == child.mode
                                }))
                })
            {
                mappings.push((original, parked.clone()));
            }
        }
    }
    mappings.sort();
    // Ambiguous attribution must remain ordinary conflict rows.
    let all = mappings.clone();
    mappings.retain(|(original, parked)| {
        all.iter()
            .filter(|(o, p)| o == original || p == parked)
            .count()
            == 1
    });
    mappings
}

/// Inspect the live index and build a resolution plan for the D/F conflict at
/// `path`, or fail if `path` is not a directory/file conflict.
pub fn plan_dir_file_resolution(
    repo: &Repository,
    path: &Path,
    choice: DirFileChoice,
) -> Result<DirFilePlan, GitError> {
    let mut index = repo.index().map_err(git_error)?;
    index.read(true).map_err(git_error)?;
    let file_path = parked_dir_file_paths(repo, &index)
        .into_iter()
        .find_map(|(original, parked)| (original == path).then_some(parked))
        .unwrap_or_else(|| path.to_path_buf());
    let file = index
        .get_path(&file_path, 2)
        .or_else(|| index.get_path(&file_path, 3))
        .ok_or_else(|| GitError::Other(format!("{} has no unmerged file side", path.display())))?;
    let file_stage = (file.flags >> 12) & 3;
    if index.get_path(&file_path, 5 - file_stage as i32).is_some() {
        return Err(GitError::Other(
            "not a one-sided directory/file conflict".into(),
        ));
    }
    let mut prefix = path_to_index_bytes(path)
        .ok_or_else(|| GitError::Other("non-representable conflict path".into()))?;
    prefix.push(b'/');
    let child_entries: Vec<_> = index
        .iter()
        .filter(|e| e.path.starts_with(&prefix))
        .map(|e| {
            Ok(DirFileEntry {
                path: bytes_to_pathbuf(&e.path)
                    .ok_or_else(|| GitError::Other("non-representable child".into()))?,
                stage: (e.flags >> 12) & 3,
                oid: e.id,
                mode: e.mode,
            })
        })
        .collect::<Result<_, GitError>>()?;
    let kept_children: Vec<_> = child_entries
        .iter()
        .filter(|e| e.stage == 0 || e.stage == 5 - file_stage)
        .cloned()
        .collect();
    if kept_children.is_empty() || child_entries.iter().any(|e| e.stage == file_stage) {
        return Err(GitError::Other("no unambiguous directory side".into()));
    }
    if !matches!(file.mode, 0o100644 | 0o100755 | 0o120000)
        || kept_children
            .iter()
            .any(|e| !matches!(e.mode, 0o100644 | 0o100755 | 0o120000))
    {
        return Err(GitError::Other(
            "directory/file resolution cannot safely restore submodules or special file modes"
                .into(),
        ));
    }
    let dir_children = kept_children.iter().map(|e| e.path.clone()).collect();
    let workdir = repo
        .workdir()
        .ok_or_else(|| GitError::Other("bare repository".into()))?;
    for relative in [path, file_path.as_path()] {
        if relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(GitError::Other("invalid conflict namespace".into()));
        }
        for parent in relative
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            match std::fs::symlink_metadata(workdir.join(parent)) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => {
                    return Err(GitError::Other(
                        "conflict namespace has an unsafe ancestor".into(),
                    ))
                }
            }
        }
    }
    let mut worktree = Vec::new();
    capture_worktree(workdir, path, &mut worktree)?;
    if file_path != path {
        capture_worktree(workdir, &file_path, &mut worktree)?;
    }
    if worktree.iter().any(|(p, _, _)| {
        p.components().any(|c| {
            c.as_os_str()
                .as_encoded_bytes()
                .eq_ignore_ascii_case(b".git")
        })
    }) {
        return Err(GitError::Other(
            "directory/file resolution cannot create recovery for nested repository metadata"
                .into(),
        ));
    }
    let removal_paths: Vec<PathBuf> = worktree
        .iter()
        .rev()
        .filter(|(p, mode, _)| {
            (file_path != path && p == &file_path)
                || (choice == DirFileChoice::KeepFile && p.starts_with(path))
                || (choice == DirFileChoice::KeepDirectory && p == path && *mode != 0o040000)
        })
        .map(|(p, _, _)| p.clone())
        .collect();

    Ok(DirFilePlan {
        unaffected: IndexIdentity::capture(&index, |bytes| {
            bytes == file_path.as_os_str().as_encoded_bytes()
                || bytes == path.as_os_str().as_encoded_bytes()
                || bytes.starts_with(&prefix)
        }),
        path: path.to_path_buf(),
        choice,
        file_oid: file.id,
        file_mode: file.mode,
        dir_children,
        file_path,
        file_stage,
        child_entries,
        kept_children,
        removal_paths,
        worktree,
    })
}

/// Re-verify the plan against the current index just before executing: the file
/// side must still be unmerged, and the complete directory child list must
/// match before any savepoint, index mutation or forced checkout.
/// (TOCTOU guard — mirrors the preflight step of the `Backend::run` path.)
pub fn preflight_dir_file_resolution(
    repo: &Repository,
    plan: &DirFilePlan,
) -> Result<(), GitError> {
    let fresh = plan_dir_file_resolution(repo, &plan.path, plan.choice)?;
    if fresh.file_oid != plan.file_oid
        || fresh.file_mode != plan.file_mode
        || fresh.dir_children != plan.dir_children
        || fresh.file_path != plan.file_path
        || fresh.file_stage != plan.file_stage
        || fresh.child_entries != plan.child_entries
        || fresh.kept_children != plan.kept_children
        || fresh.worktree != plan.worktree
        || fresh.removal_paths != plan.removal_paths
        || fresh.unaffected != plan.unaffected
    {
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
pub(crate) fn execute_dir_file_resolution(
    repo: &Repository,
    repo_path: &Path,
    plan: &DirFilePlan,
) -> Result<(), GitError> {
    let result = apply_dir_file_resolution(repo, plan);
    let recovery = result
        .as_ref()
        .map(String::as_str)
        .unwrap_or("snapshot/recovery status unavailable");

    let before = StateSummary {
        head: format!("dir-file conflict {}", plan.path.display()),
        dirty: format!("choice={}; {}", plan.choice.slug(), recovery),
    };
    let op_name = format!("conflict-dir-file:{}", plan.choice.slug());

    let outcome = match &result {
        Ok(_) => OpOutcome::Success {
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
        Err(GitError::Blocked(note)) => OpOutcome::Refused {
            blockers: vec![note.message_en()],
        },
        Err(error) => OpOutcome::Failed {
            error: error.to_string(),
        },
    };
    let entry = OpLogEntry::new(op_name, repo_path.display().to_string(), before, outcome);
    if let Err(e) = append_oplog(&entry) {
        eprintln!("kagi-git: dir-file oplog write failed (non-fatal): {}", e);
    }

    result.map(|_| ())
}

/// C1 execution primitive. The application boundary owns the one durable
/// receipt, while the compatibility facade above records through its existing
/// entry point. Both routes share this exact preflight/mutation implementation.
pub(crate) fn apply_dir_file_resolution(
    repo: &Repository,
    plan: &DirFilePlan,
) -> Result<String, GitError> {
    apply_dir_file_resolution_with_progress(repo, plan, |_| Ok(()))
}

pub(crate) fn apply_dir_file_resolution_with_progress(
    repo: &Repository,
    plan: &DirFilePlan,
    mut progress: impl FnMut(kagi_domain::conflict_family::ConflictProgress) -> Result<(), GitError>,
) -> Result<String, GitError> {
    let _lock = crate::write_lock::acquire(repo)?;
    preflight_dir_file_resolution(repo, plan)?;

    let recovery = match crate::ops::create_snapshot(
        repo,
        &format!(
            "auto snapshot before conflict-dir-file:{} {}",
            plan.choice.slug(),
            plan.path.display()
        ),
    ) {
        Ok(s) => {
            progress(kagi_domain::conflict_family::ConflictProgress::RecoveryCaptured)?;
            format!("snapshot={} commit={}", s.id, s.commit)
        }
        Err(_) => return Err(crate::ops::conflict_abort::draft_preservation_failed()),
    };
    // Recovery can be slow; it does not authorize changes made while it ran.
    let transaction = ConflictIndexTransaction::acquire(repo)?;
    preflight_dir_file_resolution(repo, plan)?;
    apply_to_index(&transaction, plan)?;
    progress(kagi_domain::conflict_family::ConflictProgress::IndexWritten)?;
    reconcile_worktree(repo, plan)?;
    progress(kagi_domain::conflict_family::ConflictProgress::WorktreeWritten)?;
    verify_dir_file_resolution(repo, plan)?;
    progress(kagi_domain::conflict_family::ConflictProgress::IndexAndWorktreeWritten)?;
    Ok(recovery)
}

/// The index surgery, kept separate so [`execute_dir_file_resolution`] can log
/// both success and failure around it.
fn apply_to_index(
    transaction: &ConflictIndexTransaction,
    plan: &DirFilePlan,
) -> Result<(), GitError> {
    let mut index = transaction.read()?;
    index.conflict_remove(&plan.file_path).map_err(git_error)?;
    for child in &plan.child_entries {
        index
            .remove(&child.path, child.stage as i32)
            .map_err(git_error)?;
    }

    match plan.choice {
        DirFileChoice::KeepDirectory => {
            for child in &plan.kept_children {
                stage_identity(&mut index, child)?;
            }
        }
        DirFileChoice::KeepFile => {
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

    transaction.write(&mut index)
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
        .ok_or_else(|| GitError::Other("bare repository has no working tree".into()))?;
    for path in &plan.removal_paths {
        let abs = workdir.join(path);
        let metadata = std::fs::symlink_metadata(&abs).map_err(io_error)?;
        if metadata.is_dir() {
            // Never recursively remove contents that were not in the approved plan.
            std::fs::remove_dir(&abs).map_err(io_error)?;
        } else {
            std::fs::remove_file(&abs).map_err(io_error)?;
        }
    }
    match plan.choice {
        DirFileChoice::KeepFile => write_file_side(repo, &workdir.join(&plan.path), plan),
        DirFileChoice::KeepDirectory => checkout_paths(repo, &plan.dir_children),
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
    let content = blob.content();
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
            let target = Path::new(std::ffi::OsStr::from_bytes(content));
            // Remove any existing node first so symlink() does not fail on EEXIST.
            let _ = std::fs::remove_file(abs);
            std::os::unix::fs::symlink(target, abs).map_err(|e| {
                GitError::Other(format!(
                    "reconcile: symlink {} failed: {}",
                    abs.display(),
                    e
                ))
            })?;
            return Ok(());
        }
    }

    std::fs::write(abs, content).map_err(|e| {
        GitError::Other(format!("reconcile: write {} failed: {}", abs.display(), e))
    })?;
    #[cfg(unix)]
    if plan.file_mode == 0o100755 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(abs, std::fs::Permissions::from_mode(0o755)).map_err(io_error)?;
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
    let mut index = repo.index().map_err(git_error)?;
    index.read(true).map_err(git_error)?;
    repo.checkout_index(Some(&mut index), Some(&mut cb))
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

fn git_error(error: git2::Error) -> GitError {
    GitError::Other(error.message().to_owned())
}

fn io_error(error: std::io::Error) -> GitError {
    GitError::Other(error.to_string())
}

fn stage_identity(index: &mut git2::Index, entry: &DirFileEntry) -> Result<(), GitError> {
    index
        .add(&git2::IndexEntry {
            ctime: git2::IndexTime::new(0, 0),
            mtime: git2::IndexTime::new(0, 0),
            dev: 0,
            ino: 0,
            mode: entry.mode,
            uid: 0,
            gid: 0,
            file_size: 0,
            id: entry.oid,
            flags: 0,
            flags_extended: 0,
            path: path_to_index_bytes(&entry.path)
                .ok_or_else(|| GitError::Other("non-representable child".into()))?,
        })
        .map_err(git_error)
}

/// Capture every node without following symlinks; includes empty directories and modes.
fn capture_worktree(
    root: &Path,
    path: &Path,
    out: &mut Vec<(PathBuf, u32, git2::Oid)>,
) -> Result<(), GitError> {
    let absolute = root.join(path);
    let metadata = match std::fs::symlink_metadata(&absolute) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_error(e)),
    };
    let (mode, oid) = if metadata.is_dir() {
        (0o040000, git2::Oid::ZERO_SHA1)
    } else if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(&absolute).map_err(io_error)?;
        let bytes = path_to_index_bytes(&target)
            .ok_or_else(|| GitError::Other("non-representable symlink".into()))?;
        (
            0o120000,
            git2::Oid::hash_object(git2::ObjectType::Blob, &bytes).map_err(git_error)?,
        )
    } else if metadata.is_file() {
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 != 0 {
                0o100755
            } else {
                0o100644
            }
        };
        #[cfg(not(unix))]
        let mode = 0o100644;
        (
            mode,
            git2::Oid::hash_file(git2::ObjectType::Blob, &absolute).map_err(git_error)?,
        )
    } else {
        return Err(GitError::Other(
            "unsupported worktree node in conflict namespace".into(),
        ));
    };
    out.push((path.to_path_buf(), mode, oid));
    if metadata.is_dir() {
        let mut children = std::fs::read_dir(absolute)
            .map_err(io_error)?
            .map(|e| e.map(|e| path.join(e.file_name())))
            .collect::<Result<Vec<_>, _>>()
            .map_err(io_error)?;
        children.sort();
        for child in children {
            capture_worktree(root, &child, out)?;
        }
    }
    Ok(())
}

/// Verify the prepared namespace, not merely absence of unmerged index stages.
pub(crate) fn verify_dir_file_resolution(
    repo: &Repository,
    plan: &DirFilePlan,
) -> Result<(), GitError> {
    let mut index = repo.index().map_err(git_error)?;
    index.read(true).map_err(git_error)?;
    if IndexIdentity::capture(&index, |p| plan.affects(p)) != plan.unaffected {
        return Err(GitError::Other(
            "unaffected index changed during directory/file resolution".into(),
        ));
    }
    let root = repo
        .workdir()
        .ok_or_else(|| GitError::Other("bare repository".into()))?;
    let fail = || {
        GitError::Other(format!(
            "directory/file verification failed: {}",
            plan.path.display()
        ))
    };
    let file_path_bytes = path_to_index_bytes(&plan.file_path);
    if plan.file_path != plan.path
        && (index
            .iter()
            .any(|e| file_path_bytes.as_deref() == Some(e.path.as_slice()))
            || std::fs::symlink_metadata(root.join(&plan.file_path)).is_ok())
    {
        return Err(fail());
    }
    let mut actual = Vec::new();
    if plan.choice == DirFileChoice::KeepFile {
        capture_worktree(root, &plan.path, &mut actual)?;
    }
    let prefix = path_to_index_bytes(&plan.path).ok_or_else(fail)?;
    let count = index
        .iter()
        .filter(|e| {
            e.path == prefix
                || (e.path.starts_with(&prefix) && e.path.get(prefix.len()) == Some(&b'/'))
        })
        .count();
    let identity_matches = |path: &Path, oid, mode| {
        index
            .get_path(path, 0)
            .is_some_and(|e| e.id == oid && e.mode == mode)
            && actual
                .iter()
                .any(|(p, m, o)| p == path && *m == mode && *o == oid)
    };
    let verified = match plan.choice {
        DirFileChoice::KeepFile => {
            count == 1 && identity_matches(&plan.path, plan.file_oid, plan.file_mode)
        }
        DirFileChoice::KeepDirectory => {
            let mut expected = git2::Index::new().map_err(git_error)?;
            let mut options = git2::DiffOptions::new();
            options
                .disable_pathspec_match(true)
                .include_untracked(true)
                .recurse_untracked_dirs(true)
                .include_typechange(true);
            for child in &plan.kept_children {
                stage_identity(&mut expected, child)?;
                options.pathspec(&child.path);
            }
            count == plan.kept_children.len()
                && plan.kept_children.iter().all(|child| {
                    index
                        .get_path(&child.path, 0)
                        .is_some_and(|e| e.id == child.oid && e.mode == child.mode)
                })
                && repo
                    .diff_index_to_workdir(Some(&expected), Some(&mut options))
                    .map_err(git_error)?
                    .deltas()
                    .len()
                    == 0
        }
    };
    if !verified {
        return Err(fail());
    }
    Ok(())
}
