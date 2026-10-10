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
            Self::Ambiguous {
                revision,
                candidates,
            } => {
                write!(
                    f,
                    "revision '{revision}' is ambiguous; use a longer prefix or full SHA"
                )?;
                if !candidates.is_empty() {
                    write!(f, ". Candidates: {}", candidates.join(", "))?;
                }
                Ok(())
            }
            Self::NotFound { revision } => {
                write!(f, "revision '{revision}' not found")?;
                if !revision.is_empty()
                    && revision.len() < 4
                    && revision.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    write!(f, "; use 4+ hex characters for a SHA prefix")?;
                }
                Ok(())
            }
            Self::NotACommit { revision, oid } => {
                write!(f, "revision '{revision}' ({oid}) is not a commit")
            }
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
        ErrorCode::Ambiguous
            if revision.len() < 4 && revision.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            RevisionError::NotFound {
                revision: revision.to_string(),
            }
        }
        ErrorCode::Ambiguous => {
            let mut candidates = Vec::new();
            // Enumerate only on failure. libgit2 determines uniqueness across
            // every object type, including objects outside reachable history.
            if (4..=40).contains(&revision.len()) && revision.bytes().all(|b| b.is_ascii_hexdigit())
            {
                let mut prefix = [0_u8; 40];
                for (index, byte) in revision.bytes().enumerate() {
                    prefix[index] = match byte {
                        b'0'..=b'9' => byte - b'0',
                        b'a'..=b'f' => byte - b'a' + 10,
                        b'A'..=b'F' => byte - b'A' + 10,
                        _ => unreachable!("validated hexadecimal prefix"),
                    };
                }
                let odb = repo
                    .odb()
                    .map_err(|e| GitError::Other(e.message().to_string()))?;
                odb.foreach(|oid| {
                    let bytes = oid.as_bytes();
                    if prefix[..revision.len()]
                        .iter()
                        .enumerate()
                        .all(|(index, nibble)| {
                            let byte = bytes[index / 2];
                            *nibble
                                == if index % 2 == 0 {
                                    byte >> 4
                                } else {
                                    byte & 0x0f
                                }
                        })
                    {
                        candidates.push(oid.to_string());
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
