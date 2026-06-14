//! Commit checklist rules — W14-CHECK (T-COMMIT-004 / 005 / 006)
//!
//! The pure staged-content rule logic now lives in `kagi_domain::checklist`
//! (ADR-0072). This module is the git2-backed bridge that reads staged index
//! BLOBs from a repository, applies the pure rules, and re-exports the pure
//! entry points so existing `crate::git::checklist::*` paths resolve during the
//! migration.

use git2::Repository;

use super::{status::WorkingTreeStatus, GitError};

pub use kagi_domain::checklist::*;

/// Environment variable name for overriding the large-binary threshold.
const LARGE_BLOB_ENV: &str = "KAGI_LARGE_BLOB_BYTES";

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
) -> Result<(Vec<String>, Vec<String>), GitError> {
    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    let large_threshold = large_blob_threshold();

    let index = repo
        .index()
        .map_err(|e| GitError::Other(format!("repo.index() failed: {}", e.message())))?;

    for file in &status.staged {
        let path = file.path.as_path();
        warnings.extend(evaluate_staged_path(path));

        // Look up the staged index entry → BLOB. A deletion or a gitlink has
        // no readable BLOB; skip content rules for those.
        let entry = match index.get_path(path, 0) {
            Some(e) => e,
            None => continue,
        };
        if entry.id.is_zero() {
            continue;
        }
        let blob = match repo.find_blob(entry.id) {
            Ok(b) => b,
            Err(_) => continue, // not a blob (e.g. submodule) — skip content rules
        };
        let content = blob.content();
        let is_binary = blob_is_binary(&blob, content);
        let (file_blockers, file_warnings) =
            evaluate_staged_blob_content(path, content, is_binary, large_threshold);

        blockers.extend(file_blockers);
        warnings.extend(file_warnings);
    }

    Ok((blockers, warnings))
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
}
