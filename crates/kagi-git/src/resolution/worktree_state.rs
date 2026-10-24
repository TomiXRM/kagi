//! Worktree-byte provenance, separate from the index and zdiff3 identities.
use super::*;
use sha2::{Digest, Sha256};
use std::io::Read;

pub(super) fn fingerprint(path: &Path) -> Result<String, GitError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok("missing".into()),
        Err(error) => return Err(GitError::Other(error.to_string())),
    };
    let mut hash = Sha256::new();
    let kind = if metadata.file_type().is_symlink() {
        let target =
            std::fs::read_link(path).map_err(|error| GitError::Other(error.to_string()))?;
        hash.update(target.as_os_str().as_encoded_bytes());
        "symlink"
    } else if metadata.is_file() {
        let mut file =
            std::fs::File::open(path).map_err(|error| GitError::Other(error.to_string()))?;
        let mut bytes = [0u8; 8192];
        loop {
            let count = file
                .read(&mut bytes)
                .map_err(|error| GitError::Other(error.to_string()))?;
            if count == 0 {
                break;
            }
            hash.update(&bytes[..count]);
        }
        "file"
    } else {
        // Gitlinks/directories are raw index choices, never text overwrites.
        return Ok("directory".into());
    };
    Ok(format!("{kind}:{}", hex::encode(hash.finalize())))
}

pub(crate) fn validate(
    repo: &Repository,
    path: &Path,
    expected: Option<&str>,
) -> Result<(), GitError> {
    let root = repo
        .workdir()
        .ok_or_else(|| GitError::Other("repository has no working tree".into()))?;
    if expected.is_none() || fingerprint(&root.join(path)).ok().as_deref() != expected {
        return Err(GitError::Blocked(Box::new(
            kagi_domain::plan_note::PlanNote::Conflicts(
                kagi_domain::plan_note::ConflictsNote::WorktreeChanged,
            ),
        )));
    }
    Ok(())
}

impl ResolutionBuffer {
    pub fn worktree_fingerprint(&self, path: &Path) -> Option<&str> {
        self.files.get(path)?.worktree_fingerprint.as_deref()
    }

    /// Cached watcher projection. Never used in place of the live preflight.
    pub fn worktree_changed(&self, path: &Path) -> bool {
        self.files
            .get(path)
            .is_some_and(|file| file.worktree_changed)
    }

    pub(crate) fn validate_worktree(&self, repo: &Repository, path: &Path) -> Result<(), GitError> {
        validate(repo, path, self.worktree_fingerprint(path))
    }
}
