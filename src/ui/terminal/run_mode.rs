//! The terminal shells Kagi is running, and what they decide:
//! - `nonconcurrent` worktree run mode (#859, ADR-0213): which running shell,
//!   if any, stops a new terminal in this worktree. The decision is
//!   `kagi_domain::worktree_run_mode::blocking_worktree`.
//! - removing a worktree (#867): a running shell in it blocks the plan, and
//!   processes an exited one left behind warn. The decision is
//!   `kagi_domain::worktree_remove_shells::remove_notes`.
//!
//! Only shells Kagi started count. Processes started outside Kagi are not seen.

use std::path::{Path, PathBuf};

use super::KagiTerminalSession;
use crate::app::SessionId;
use crate::ui::KagiApp;
use kagi_domain::worktree_remove_shells::KagiShell;
use kagi_domain::worktree_run_mode::{blocking_worktree, LiveShell, RunMode};

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl KagiApp {
    /// Every tab's terminal whose shell Kagi has not observed exit (#772).
    fn live_terminals(&self) -> impl Iterator<Item = (SessionId, &KagiTerminalSession)> {
        self.ui
            .iter()
            .filter_map(|(session, ui)| Some((*session, ui.terminal_session.as_ref()?)))
            .filter(|(_, terminal)| {
                terminal
                    .shell
                    .as_ref()
                    .is_some_and(|shell| shell.exit.is_none())
            })
    }

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
            .live_terminals()
            .filter(|(session, _)| *session != owner)
            .filter_map(|(_, terminal)| {
                Some(LiveShell {
                    repository: kagi_git::worktree_ports::repository_of(&terminal.repo_path)?,
                    worktree: canonical(&terminal.repo_path),
                })
            })
            .collect();
        blocking_worktree(mode, &repository, &canonical(repo_path), &live).map(Path::to_path_buf)
    }

    /// #867: remember a shell a terminal just started, past its tab's life.
    pub(super) fn record_started_shell(&mut self, worktree: &Path, pid: Option<u32>) {
        if let Some(pid) = pid {
            self.started_shells.push((canonical(worktree), pid));
        }
    }

    /// #867: the shells Kagi started, each marked live while a tab's terminal
    /// still runs it. A live shell with no PID still blocks.
    pub(crate) fn kagi_shells(&self) -> Vec<KagiShell> {
        let live: Vec<(PathBuf, Option<u32>)> = self
            .live_terminals()
            .map(|(_, terminal)| {
                let pid = terminal.shell.as_ref().and_then(|shell| shell.pid);
                (canonical(&terminal.repo_path), pid)
            })
            .collect();
        let exited = self
            .started_shells
            .iter()
            .filter(|(_, pid)| !live.iter().any(|(_, live)| *live == Some(*pid)))
            .map(|(worktree, pid)| KagiShell {
                worktree: worktree.clone(),
                session: Some(*pid),
                live: false,
            });
        live.iter()
            .map(|(worktree, pid)| KagiShell {
                worktree: worktree.clone(),
                session: *pid,
                live: true,
            })
            .chain(exited)
            .collect()
    }
}
