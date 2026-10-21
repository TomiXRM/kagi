//! Commit checklist rules — W14-CHECK (T-COMMIT-004 / 005 / 006)
//!
//! The pure staged-content rule logic now lives in `kagi_domain::checklist`
//! (ADR-0072). This module is the git2-backed bridge that reads staged index
//! BLOBs from a repository, applies the pure rules, and re-exports the pure
//! entry points so existing `crate::checklist::*` paths resolve during the
//! migration.

use std::collections::HashSet;
use std::path::PathBuf;

use git2::Repository;

use super::{status::WorkingTreeStatus, GitError};
use crate::ops::PlanNote;

pub use kagi_domain::checklist::*;

/// Environment variable name for overriding the large-binary threshold.
const LARGE_BLOB_ENV: &str = "KAGI_LARGE_BLOB_BYTES";

#[cfg(test)]
thread_local! {
    // Full-checklist calls must never enter the panel's secret-only read.
    static SCAN_WORK: std::cell::Cell<(usize, usize)> = const {
        std::cell::Cell::new((0, 0))
    };
}

/// Run the staged-content checklist rules (ADR-0043 rules 4/5/6) over the
/// repository's **index** and return `(blockers, warnings)`.
///
/// Only paths present in `status.staged` are inspected — these are the files
/// that will actually be committed.  Each is read from the object database as a
/// BLOB (the index/staged content, never the working tree) and scanned.
///
/// This function performs the git2 BLOB lookup only. The byte/path rule logic
/// is delegated to `kagi_domain::checklist`.
///
/// # Returns
///
/// `(blockers, warnings)`:
/// - `blockers` — rule 4 (conflict marker).  Override **not** allowed.
/// - `warnings` — rule 5 (secret/.env) and rule 6 (large binary).  Override
///   allowed (ADR-0039).
///
/// # Errors
///
/// Returns [`GitError::Other`] if the index cannot be read.  A staged path
/// whose BLOB cannot be resolved (e.g. a submodule gitlink, which has no BLOB)
/// is skipped silently rather than failing the whole checklist.
pub fn checklist(
    repo: &Repository,
    status: &WorkingTreeStatus,
) -> Result<(Vec<PlanNote>, Vec<PlanNote>), GitError> {
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();
    let large_threshold = large_blob_threshold();
    let index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;

    for file in &status.staged {
        let path = file.path.as_path();
        warnings.extend(
            evaluate_staged_path(path)
                .into_iter()
                .map(PlanNote::Checklist),
        );
        // Deletions and gitlinks have no readable index BLOB.
        let Some(entry) = index.get_path(path, 0) else {
            continue;
        };
        if entry.id.is_zero() {
            continue;
        }
        let Ok(blob) = repo.find_blob(entry.id) else {
            continue;
        };
        let content = blob.content();
        let is_binary = blob_is_binary(&blob, content);
        #[cfg(test)]
        SCAN_WORK.with(|work| {
            let (full_checks, bytes) = work.get();
            work.set((full_checks + 1, bytes + content.len().min(1024 * 1024)));
        });
        let (file_blockers, file_warnings) =
            evaluate_staged_blob_content(path, content, is_binary, large_threshold);
        blockers.extend(file_blockers.into_iter().map(PlanNote::Checklist));
        warnings.extend(file_warnings.into_iter().map(PlanNote::Checklist));
    }
    Ok((blockers, warnings))
}

/// Collect rule-5 identities directly from the staged status paths, not from
/// plan notes: their display strings can replace invalid UTF-8 bytes.
pub(crate) fn staged_secret_paths(
    repo: &Repository,
    status: &WorkingTreeStatus,
) -> Result<HashSet<PathBuf>, GitError> {
    let index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;
    let mut paths = HashSet::new();
    for file in &status.staged {
        let path = file.path.as_path();
        if path_is_secret_name(path) {
            paths.insert(path.to_path_buf());
            continue;
        }
        let Some(entry) = index.get_path(path, 0) else {
            continue;
        };
        if entry.id.is_zero() {
            continue;
        }
        let Ok(blob) = repo.find_blob(entry.id) else {
            continue;
        };
        let content = blob.content();
        let prefix = &content[..content.len().min(SECRET_SCAN_BYTES)];
        #[cfg(test)]
        SCAN_WORK.with(|work| {
            let (full_checks, bytes) = work.get();
            work.set((full_checks, bytes + prefix.len()));
        });
        // Rule 5 only: no marker scan, large-binary rule or display-note
        // allocation. Both predicates are shared with the full commit plan.
        if !blob_is_binary(&blob, prefix) && content_has_secret(prefix) {
            paths.insert(path.to_path_buf());
        }
    }
    Ok(paths)
}

/// Resolve the large-binary byte threshold from `KAGI_LARGE_BLOB_BYTES`,
/// falling back to [`DEFAULT_LARGE_BLOB_BYTES`].  An unparseable value falls
/// back to the default.
fn large_blob_threshold() -> u64 {
    match std::env::var(LARGE_BLOB_ENV) {
        Ok(v) => v.trim().parse::<u64>().unwrap_or(DEFAULT_LARGE_BLOB_BYTES),
        Err(_) => DEFAULT_LARGE_BLOB_BYTES,
    }
}

/// Decide whether a staged BLOB is binary.
///
/// Uses `git2::Blob::is_binary()` (libgit2's heuristic) and, as a fallback that
/// matches git's own NUL heuristic, delegates the pure content probe to
/// `kagi_domain::checklist`.
fn blob_is_binary(blob: &git2::Blob<'_>, content: &[u8]) -> bool {
    blob.is_binary() || content_looks_binary(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_env_default() {
        // Default when unset.
        std::env::remove_var(LARGE_BLOB_ENV);
        assert_eq!(large_blob_threshold(), DEFAULT_LARGE_BLOB_BYTES);
    }

    #[test]
    fn panel_secret_scan_bounds_work_for_thousand_staged_files() {
        use kagi_domain::status::{ChangeKind, FileStatus};

        const FILES: usize = 1000;
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let mut content = vec![b'a'; 1024 * 1024];
        content[..4].copy_from_slice(b"ghp_");
        let oid = repo.blob(&content).unwrap();
        let mut index = repo.index().unwrap();
        let mut status = WorkingTreeStatus::default();
        for number in 0..FILES {
            let path = format!("ordinary/file-{number}.txt");
            index
                .add(&git2::IndexEntry {
                    ctime: git2::IndexTime::new(0, 0),
                    mtime: git2::IndexTime::new(0, 0),
                    dev: 0,
                    ino: 0,
                    mode: 0o100644,
                    uid: 0,
                    gid: 0,
                    file_size: content.len() as u32,
                    id: oid,
                    flags: 0,
                    flags_extended: 0,
                    path: path.as_bytes().to_vec(),
                })
                .unwrap();
            status.staged.push(FileStatus {
                path: path.into(),
                change: ChangeKind::Added,
            });
        }
        index.write().unwrap();
        SCAN_WORK.with(|work| work.set((0, 0)));
        let backend = crate::backend::Backend::open(dir.path()).unwrap();
        let found = backend.staged_secret_paths(&status).unwrap();
        assert_eq!(found.len(), FILES, "all prefixes contain a secret");
        let (full_checks, bytes) = SCAN_WORK.with(std::cell::Cell::get);
        assert_eq!(full_checks, 0, "panel reads must not scan conflict markers");
        assert_eq!(
            bytes,
            FILES * 8 * 1024,
            "panel content classification must receive only 8 KiB per staged blob"
        );
    }

    /// Unlike the Linux filesystem fixture, raw index entries exercise invalid
    /// UTF-8 identities on macOS too, without creating forbidden filenames.
    #[cfg(unix)]
    #[test]
    fn raw_index_secret_paths_preserve_non_utf8_name_and_content_identities() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        use kagi_domain::status::{ChangeKind, FileStatus};

        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let mut index = repo.index().unwrap();
        let mut status = WorkingTreeStatus::default();
        let entries: &[(&[u8], &[u8])] = &[
            (b"private-\xff/.env", b"EXAMPLE=ordinary\n"),
            (
                b"private-\xff/generated.lock",
                b"-----BEGIN PRIVATE KEY-----\nfixture\n",
            ),
            (
                b"private-\xef\xbf\xbd/generated.lock",
                b"ordinary staged content\n",
            ),
        ];
        for &(raw_path, content) in entries {
            index
                .add(&git2::IndexEntry {
                    ctime: git2::IndexTime::new(0, 0),
                    mtime: git2::IndexTime::new(0, 0),
                    dev: 0,
                    ino: 0,
                    mode: 0o100644,
                    uid: 0,
                    gid: 0,
                    file_size: content.len() as u32,
                    id: repo.blob(content).unwrap(),
                    flags: 0,
                    flags_extended: 0,
                    path: raw_path.to_vec(),
                })
                .unwrap();
            status.staged.push(FileStatus {
                path: PathBuf::from(OsStr::from_bytes(raw_path)),
                change: ChangeKind::Added,
            });
        }
        index.write().unwrap();

        let backend = crate::backend::Backend::open(dir.path()).unwrap();
        let found = backend.staged_secret_paths(&status).unwrap();
        let expected: HashSet<_> = status.staged[..2]
            .iter()
            .map(|file| file.path.clone())
            .collect();
        assert_eq!(
            found, expected,
            "both rule-5 warning kinds must retain original staged path bytes"
        );
        assert!(
            !found.contains(&status.staged[2].path),
            "lossy display twins must not acquire another file's warning"
        );
    }
}
