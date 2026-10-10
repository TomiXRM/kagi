//! Direct revision reads, independent of the graph's display budget.
use crate::{Commit, GitError};
use git2::{ErrorCode, Repository};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevisionError {
    Ambiguous {
        revision: String,
        candidates: Vec<String>,
    },
    NotFound {
        revision: String,
    },
    NotACommit {
        revision: String,
        oid: String,
    },
}

impl std::fmt::Display for RevisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ambiguous { revision, candidates } => write!(f, "revision '{revision}' is ambiguous; use a longer prefix or full SHA. Candidates: {}", candidates.join(", ")),
            Self::NotFound { revision } => write!(f, "revision '{revision}' not found"),
            Self::NotACommit { revision, oid } => write!(f, "revision '{revision}' ({oid}) is not a commit"),
        }
    }
}

pub(crate) fn resolve_commit(repo: &Repository, revision: &str) -> Result<Commit, GitError> {
    let object = match repo.revparse_single(revision) {
        Ok(object) => object,
        Err(error) => return Err(resolution_error(repo, revision, error)?),
    };
    let commit = object.peel_to_commit().map_err(|error| {
        if matches!(error.code(), ErrorCode::InvalidSpec | ErrorCode::Peel) {
            GitError::Revision(RevisionError::NotACommit {
                revision: revision.to_string(),
                oid: object.id().to_string(),
            })
        } else {
            GitError::Other(error.message().to_string())
        }
    })?;
    Ok(crate::log::commit_from_raw(&commit))
}

fn resolution_error(
    repo: &Repository,
    revision: &str,
    error: git2::Error,
) -> Result<GitError, GitError> {
    let typed = match error.code() {
        ErrorCode::Ambiguous => {
            let mut candidates = Vec::new();
            // Enumerate only on failure. libgit2 determines uniqueness across
            // every object type, including objects outside reachable history.
            if revision.len() >= 4 && revision.bytes().all(|b| b.is_ascii_hexdigit()) {
                let prefix = revision.to_ascii_lowercase();
                let odb = repo
                    .odb()
                    .map_err(|e| GitError::Other(e.message().to_string()))?;
                odb.foreach(|oid| {
                    let sha = oid.to_string();
                    if sha.starts_with(&prefix) {
                        candidates.push(sha);
                    }
                    true
                })
                .map_err(|e| GitError::Other(e.message().to_string()))?;
                candidates.sort_unstable();
                candidates.dedup();
            }
            RevisionError::Ambiguous {
                revision: revision.to_string(),
                candidates,
            }
        }
        ErrorCode::NotFound | ErrorCode::InvalidSpec => RevisionError::NotFound {
            revision: revision.to_string(),
        },
        _ => return Ok(GitError::Other(error.message().to_string())),
    };
    Ok(GitError::Revision(typed))
}
