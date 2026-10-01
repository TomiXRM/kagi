//! The terminal shells Kagi is running, and what they decide:
//! - `nonconcurrent` worktree run mode (#859, ADR-0213): which running shell,
//!   if any, stops a new terminal in this worktree. The decision is
//!   `kagi_domain::worktree_run_mode::blocking_worktree`.
//! - removing a worktree (#867): a running shell in it blocks the plan, and
//!   processes an exited one left behind warn. The decision is
//!   `kagi_domain::worktree_remove_shells::remove_notes`.
//!
//! Liveness comes from [`StartedShell`], which outlives the tab: closing a tab
//! drops its PTY, but the shell exits only when it handles the hangup (it may
//! ignore it), and the background wait reports that later. A shell is running
//! until that wait reports an exit; a failed wait (`ShellExit::Unknown`) is
//! not an exit (ADR-0208 決定 3).
//!
//! Only shells Kagi started count. Processes started outside Kagi are not seen.

use std::path::{Path, PathBuf};

use super::ShellExit;
use crate::app::SessionId;
use crate::ui::KagiApp;
use kagi_domain::worktree_remove_shells::KagiShell;
use kagi_domain::worktree_run_mode::{blocking_worktree, LiveShell, RunMode};

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// A shell a Kagi terminal started this run, kept past its tab's life.
#[derive(Clone, Debug)]
pub(crate) struct StartedShell {
    /// The tab session and spawn generation the background wait reports for.
    owner: SessionId,
    generation: u64,
    /// The worktree it runs in (canonical).
    worktree: PathBuf,
    /// Its PID, which is also its session id (the PTY spawn runs `setsid`).
    pid: Option<u32>,
    /// Set once the wait reports that the shell exited.
    exited: bool,
}

impl KagiApp {
    /// Shells that have not been seen to exit.
    fn running_shells(&self) -> impl Iterator<Item = &StartedShell> {
        self.started_shells.iter().filter(|shell| !shell.exited)
    }

    /// The worktree whose running terminal shell keeps the worktree at
    /// `repo_path` from starting one. Always `None` unless `worktree_run_mode`
    /// is `nonconcurrent`. A shell counts until Kagi observes it exit, also
    /// after its tab closed; processes started outside Kagi are not seen
    /// (ADR-0213).
    pub(super) fn nonconcurrent_blocker(&self, repo_path: &Path) -> Option<PathBuf> {
        let mode = crate::ui::settings::Settings::load().worktree_run_mode();
        if mode == RunMode::Concurrent {
            return None;
        }
        let repository = kagi_git::worktree_ports::repository_of(repo_path)?;
        let live: Vec<LiveShell> = self
            .running_shells()
            .filter_map(|shell| {
                Some(LiveShell {
                    repository: kagi_git::worktree_ports::repository_of(&shell.worktree)?,
                    worktree: shell.worktree.clone(),
                })
            })
            .collect();
        blocking_worktree(mode, &repository, &canonical(repo_path), &live).map(Path::to_path_buf)
    }

    /// #867: remember a shell a terminal just started.
    pub(super) fn record_started_shell(
        &mut self,
        owner: SessionId,
        generation: u64,
        worktree: &Path,
        pid: Option<u32>,
    ) {
        self.started_shells.push(StartedShell {
            owner,
            generation,
            worktree: canonical(worktree),
            pid,
            exited: false,
        });
    }

    /// The background wait reported on a shell, whether or not its tab is
    /// still open. Only a reported exit ends it.
    pub(super) fn note_shell_wait(&mut self, owner: SessionId, generation: u64, exit: &ShellExit) {
        if !matches!(exit, ShellExit::Exited { .. }) {
            return;
        }
        if let Some(shell) = self
            .started_shells
            .iter_mut()
            .find(|shell| shell.owner == owner && shell.generation == generation)
        {
            shell.exited = true;
        }
    }

    /// #867: the shells Kagi started, for the remove plan.
    pub(crate) fn kagi_shells(&self) -> Vec<KagiShell> {
        self.started_shells
            .iter()
            .map(|shell| KagiShell {
                worktree: shell.worktree.clone(),
                session: shell.pid,
                live: !shell.exited,
            })
            .collect()
    }
}
