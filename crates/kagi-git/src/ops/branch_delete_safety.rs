//! Checked-out branch protection shared by delete planning and execution.
use super::*;

/// Open the main repository as well as every linked worktree, including when
/// the caller itself is linked. An unreadable registration fails closed.
pub(super) fn repositories(repo: &Repository) -> Result<Vec<Repository>, GitError> {
    let mut repositories = vec![Repository::open(repo.commondir()).map_err(error)?];
    for name in repo.worktrees().map_err(error)?.iter() {
        let name = name
            .map_err(error)?
            .ok_or_else(|| GitError::Other("non-UTF-8 worktree name".into()))?;
        repositories.push(
            Repository::open_from_worktree(&repo.find_worktree(name).map_err(error)?)
                .map_err(error)?,
        );
    }
    repositories.sort_by_key(|repo| repo.path().to_path_buf());
    repositories.dedup_by(|a, b| a.path() == b.path());
    Ok(repositories)
}

pub(super) fn checked_out_at(
    repositories: &[Repository],
    branch: &str,
) -> Result<Option<PathBuf>, GitError> {
    let deleting = format!("refs/heads/{branch}");
    for repo in repositories {
        // Read the symbolic HEAD itself, including unborn HEADs and aliases.
        if depends_on(repo, repo.find_reference("HEAD").map_err(error)?, &deleting)? {
            return Ok(Some(repo.workdir().unwrap_or(repo.path()).to_path_buf()));
        }
    }
    Ok(None)
}

/// A symbolic alias of the deleted ref is not an independent reachability root.
/// Follow each hop (resolve() alone loses the dependency information).
pub(super) fn depends_on<'repo>(
    repo: &'repo Repository,
    mut reference: git2::Reference<'repo>,
    deleting: &str,
) -> Result<bool, GitError> {
    let mut visited = std::collections::HashSet::new();
    loop {
        if reference.name().ok() == Some(deleting) {
            return Ok(true);
        }
        let Some(target) = reference
            .symbolic_target()
            .map_err(error)?
            .map(str::to_owned)
        else {
            return Ok(false);
        };
        if target == deleting {
            return Ok(true);
        }
        if !visited.insert(target.clone()) {
            return Err(GitError::Other("cyclic symbolic reference".into()));
        }
        reference = match repo.find_reference(&target) {
            Ok(reference) => reference,
            Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(false),
            Err(cause) => return Err(error(cause)),
        };
    }
}

fn error(error: git2::Error) -> GitError {
    GitError::Other(error.to_string())
}

/// Missing reflogs are already clean. Other failures must remain visible.
pub(super) fn remove_reflog(
    repo: &Repository,
    name: &str,
) -> Result<kagi_domain::operation::DeleteBranchProgress, GitError> {
    let existed = repo.reference_has_log(name).map_err(error)?;
    let reflog_removed = match delete_reflog(repo, name) {
        Ok(()) => existed,
        Err(cause) if cause.code() == git2::ErrorCode::NotFound => false,
        Err(cause) => return Err(error(cause)),
    };
    Ok(kagi_domain::operation::DeleteBranchProgress { reflog_removed })
}

fn delete_reflog(repo: &Repository, name: &str) -> Result<(), git2::Error> {
    #[cfg(test)]
    if matches!(
        FAULT.with(|fault| fault.get()),
        Some(DeleteFault::ReflogFailure)
    ) {
        FAULT.with(|fault| fault.take());
        return Err(git2::Error::from_str(
            "injected non-NotFound reflog failure",
        ));
    }
    repo.reflog_delete(name)
}

pub(super) fn commit(transaction: git2::Transaction<'_>) -> Result<(), GitError> {
    #[cfg(test)]
    if matches!(
        FAULT.with(|fault| fault.take()),
        Some(DeleteFault::CommitFailure)
    ) {
        return Err(GitError::Other(
            "injected branch ref transaction failure".into(),
        ));
    }
    transaction.commit().map_err(error)
}

// Same test-only, thread-local, one-shot seam as backend::absorb. Absent from
// production builds; no environment variable or public mutation bypass.
#[cfg(test)]
thread_local! {
    static FAULT: std::cell::Cell<Option<DeleteFault>> = const { std::cell::Cell::new(None) };
}
#[cfg(test)]
#[derive(Clone, Copy)]
enum DeleteFault {
    CommitFailure,
    ReflogFailure,
}

#[cfg(test)]
#[path = "branch_delete_release_tests.rs"]
mod release_tests;
