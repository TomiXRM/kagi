//! Commit-panel "Stage all" / "Unstage all", split out of `commit.rs` so the
//! per-action owner guard does not grow that file past its LOC ceiling.
//!
//! Deferred from a leased panel listener (`spawn_in` → next tick): the owner
//! session is frozen at the button press and re-checked here, so a tab switch
//! before the task runs cannot stage/unstage against the panel now on screen
//! (ADR-0197 決定 5 / #722 P1-c).

use super::staging_failure::StageAction;
use crate::ui::*;

impl KagiApp {
    /// Stage every non-conflicted unstaged file (T-UI-002: Stage all).
    pub fn do_stage_all(&mut self, owner: crate::app::SessionId, cx: &mut Context<Self>) {
        if !self.pane_mutation_admitted(owner) {
            return;
        }
        // #476: stage into the PANEL's repository — a linked worktree's, when
        // the panel shows one — never the tab's.
        let repo_path = match self.commit_panel_repo_path(cx) {
            Some(p) => p,
            None => return,
        };
        let paths: Vec<std::path::PathBuf> = match self.ui().commit_panel.as_ref() {
            Some(e) => {
                let p = &e.read(cx).state;
                p.unstaged
                    .iter()
                    .filter(|f| !p.is_conflicted(&f.path))
                    .map(|f| f.path.clone())
                    .collect()
            }
            None => return,
        };
        if paths.is_empty() {
            return;
        }
        let Some(lease) = self.reserve_stage_write(StageAction::StageAll, &repo_path, &paths, cx)
        else {
            return;
        };
        let result =
            lease.run(|| self.with_staging_repo(&repo_path, |repo| repo.stage_files(&paths)));
        self.refresh_write_busy();
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                self.stage_failure(StageAction::StageAll, &repo_path, &paths, &e, cx);
                return;
            }
        };
        match result {
            Ok(n) => {
                klog!("staged-all: {} file(s)", n);
                self.refresh_batch_staging(&repo_path, cx);
            }
            Err(e) => {
                self.stage_failure(StageAction::StageAll, &repo_path, &paths, &e, cx);
            }
        }
    }

    /// Unstage every staged file (T-UI-002: Unstage all).
    pub fn do_unstage_all(&mut self, owner: crate::app::SessionId, cx: &mut Context<Self>) {
        if !self.pane_mutation_admitted(owner) {
            return;
        }
        // #476: unstage in the PANEL's repository, never the tab's.
        let repo_path = match self.commit_panel_repo_path(cx) {
            Some(p) => p,
            None => return,
        };
        let paths: Vec<std::path::PathBuf> = match self.ui().commit_panel.as_ref() {
            Some(e) => e
                .read(cx)
                .state
                .staged
                .iter()
                .map(|f| f.path.clone())
                .collect(),
            None => return,
        };
        if paths.is_empty() {
            return;
        }
        let Some(lease) = self.reserve_stage_write(StageAction::UnstageAll, &repo_path, &paths, cx)
        else {
            return;
        };
        let result =
            lease.run(|| self.with_staging_repo(&repo_path, |repo| repo.unstage_files(&paths)));
        self.refresh_write_busy();
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                self.stage_failure(StageAction::UnstageAll, &repo_path, &paths, &e, cx);
                return;
            }
        };
        match result {
            Ok(n) => {
                klog!("unstaged-all: {} file(s)", n);
                self.refresh_batch_staging(&repo_path, cx);
            }
            Err(e) => {
                self.stage_failure(StageAction::UnstageAll, &repo_path, &paths, &e, cx);
            }
        }
    }

    /// Publish the write's read once; local feedback must not wait for the
    /// filesystem watcher's debounce or repeat the panel's two diffstat reads.
    fn refresh_batch_staging(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let backend = match kagi_git::Backend::open(path) {
            Ok(backend) => backend,
            Err(error) => {
                klog!("commit_panel: repo open error: {}", error);
                return;
            }
        };
        let (status, digest) = match backend.commit_panel_status() {
            Ok(status) => status,
            Err(error) => {
                klog!("commit_panel: working_tree_status error: {}", error);
                return;
            }
        };
        let local = self.repo_path.as_deref() == Some(path);
        let baseline = local.then(|| status.clone());
        let wip = kagi_domain::refs::WorktreeWip {
            staged: status.staged.len(),
            unstaged: status.unstaged.len(),
            untracked: status.untracked.len(),
        };
        let diffstat = self.ui().commit_panel.clone().map(|panel| {
            panel.update(cx, |panel, cx| {
                panel.apply_status(&backend, status, digest);
                let mut diffstat = WipDiffStat::default();
                for stat in panel
                    .state
                    .staged_stats
                    .iter()
                    .chain(&panel.state.unstaged_stats)
                {
                    diffstat.additions += stat.additions;
                    diffstat.deletions += stat.deletions;
                }
                cx.notify();
                diffstat
            })
        });
        if let Some(status) = baseline {
            let view = self.view_mut();
            view.status_summary.is_dirty = status.is_dirty();
            view.status_summary.staged = status.staged.len();
            view.status_summary.unstaged = status.unstaged.len();
            view.status_summary.untracked = status.untracked.len();
            view.status_summary.conflict_count = status.conflicted.len();
            view.is_dirty = status.is_dirty();
            if let Some(ui) = self.ui_mut() {
                ui.last_working_status = Some(status);
                ui.wip_diffstat = diffstat;
                ui.wip_diffstat_request = ui.wip_diffstat_request.wrapping_add(1);
            }
        } else {
            for worktree in &mut self.view_mut().worktrees {
                if crate::ui::worktree_wip::canon(worktree.path.clone()) == path {
                    worktree.wip = Some(wip);
                }
            }
        }
        cx.notify();
    }
}
