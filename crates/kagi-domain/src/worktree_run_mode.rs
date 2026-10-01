//! The `nonconcurrent` worktree run mode (#859, ADR-0213).
//!
//! Some projects cannot run two worktrees side by side: one shared database, a
//! fixed callback URL. The only thing Kagi starts is a worktree's embedded
//! terminal shell, so "running" means *a live terminal shell*: in
//! `nonconcurrent` mode, one worktree of a repository at a time may have one.
//! Pure — the caller supplies the live shells and the repository identities.

use std::path::{Path, PathBuf};

/// `worktree_run_mode` in settings.json.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunMode {
    /// Every worktree may run at once (the default).
    #[default]
    Concurrent,
    /// One worktree of a repository at a time.
    Nonconcurrent,
}

impl RunMode {
    /// `"nonconcurrent"` (any case, surrounding blanks ignored) is
    /// [`RunMode::Nonconcurrent`]; anything else — unset, a typo — is the
    /// default, so a broken setting never blocks a terminal.
    pub fn parse(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("nonconcurrent") {
            Self::Nonconcurrent
        } else {
            Self::Concurrent
        }
    }
}

/// A terminal shell that is still running: its repository (the common git
/// directory every worktree of it shares) and its worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveShell {
    pub repository: PathBuf,
    pub worktree: PathBuf,
}

/// The worktree whose running shell stops a new one in `worktree` of
/// `repository`, if any. `None` in [`RunMode::Concurrent`], for a shell in the
/// same worktree, and for shells of other repositories.
pub fn blocking_worktree<'a>(
    mode: RunMode,
    repository: &Path,
    worktree: &Path,
    live: &'a [LiveShell],
) -> Option<&'a Path> {
    if mode == RunMode::Concurrent {
        return None;
    }
    live.iter()
        .find(|shell| shell.repository == repository && shell.worktree != worktree)
        .map(|shell| shell.worktree.as_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(repository: &str, worktree: &str) -> LiveShell {
        LiveShell {
            repository: repository.into(),
            worktree: worktree.into(),
        }
    }

    const REPO: &str = "/r/.git";

    #[test]
    fn parse_reads_only_nonconcurrent() {
        assert_eq!(RunMode::parse("nonconcurrent"), RunMode::Nonconcurrent);
        assert_eq!(RunMode::parse(" NonConcurrent "), RunMode::Nonconcurrent);
        for other in ["", "concurrent", "non-concurrent", "serial"] {
            assert_eq!(RunMode::parse(other), RunMode::Concurrent, "{other:?}");
        }
    }

    #[test]
    fn another_worktree_of_the_same_repository_blocks() {
        let live = [shell(REPO, "/r/a")];
        assert_eq!(
            blocking_worktree(
                RunMode::Nonconcurrent,
                Path::new(REPO),
                Path::new("/r/b"),
                &live
            ),
            Some(Path::new("/r/a"))
        );
    }

    #[test]
    fn concurrent_never_blocks() {
        let live = [shell(REPO, "/r/a")];
        assert_eq!(
            blocking_worktree(
                RunMode::Concurrent,
                Path::new(REPO),
                Path::new("/r/b"),
                &live
            ),
            None
        );
    }

    #[test]
    fn the_same_worktree_and_other_repositories_do_not_block() {
        let live = [shell(REPO, "/r/a"), shell("/other/.git", "/other")];
        assert_eq!(
            blocking_worktree(
                RunMode::Nonconcurrent,
                Path::new(REPO),
                Path::new("/r/a"),
                &live
            ),
            None,
            "a second shell in the same worktree is not a second worktree running"
        );
        assert_eq!(
            blocking_worktree(
                RunMode::Nonconcurrent,
                Path::new("/third/.git"),
                Path::new("/third"),
                &live
            ),
            None
        );
    }

    #[test]
    fn nothing_live_blocks_nothing() {
        assert_eq!(
            blocking_worktree(
                RunMode::Nonconcurrent,
                Path::new(REPO),
                Path::new("/r/b"),
                &[]
            ),
            None,
            "an exited shell is not passed in, so it cannot block"
        );
    }
}
