//! `nonconcurrent` worktree run mode (#859, ADR-0213): which running shell, if
//! any, stops a new terminal in this worktree. The decision is
//! `kagi_domain::worktree_run_mode::blocking_worktree`; this gathers its inputs
//! — the setting, the live shells of every tab, and the repository each
//! worktree belongs to.

use std::path::{Path, PathBuf};

use crate::app::SessionId;
use crate::ui::KagiApp;
use kagi_domain::worktree_run_mode::{blocking_worktree, LiveShell, RunMode};

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl KagiApp {
    /// The worktree whose running terminal shell keeps `owner`'s worktree at
    /// `repo_path` from starting one. Always `None` unless `worktree_run_mode`
    /// is `nonconcurrent`. A shell counts while Kagi has not observed it exit;
    /// processes started outside Kagi are not seen (ADR-0213).
    pub(super) fn nonconcurrent_blocker(
        &self,
        owner: SessionId,
        repo_path: &Path,
    ) -> Option<PathBuf> {
        let mode = crate::ui::settings::Settings::load().worktree_run_mode();
        if mode == RunMode::Concurrent {
            return None;
        }
        let repository = kagi_git::worktree_ports::repository_of(repo_path)?;
        let live: Vec<LiveShell> = self
            .ui
            .iter()
            .filter(|(session, _)| **session != owner)
            .filter_map(|(_, ui)| ui.terminal_session.as_ref())
            .filter(|terminal| {
                terminal
                    .shell
                    .as_ref()
                    .is_some_and(|shell| shell.exit.is_none())
            })
            .filter_map(|terminal| {
                Some(LiveShell {
                    repository: kagi_git::worktree_ports::repository_of(&terminal.repo_path)?,
                    worktree: canonical(&terminal.repo_path),
                })
            })
            .collect();
        blocking_worktree(mode, &repository, &canonical(repo_path), &live).map(Path::to_path_buf)
    }
}
