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
                if let Some(entity) = self.ui().commit_panel.clone() {
                    entity.update(cx, |v, _| v.state.reload_status(&repo_path));
                }
                self.refresh_wip_diffstat();
                self.refresh_worktree_wip_row(&repo_path);
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
                if let Some(entity) = self.ui().commit_panel.clone() {
                    entity.update(cx, |v, _| v.state.reload_status(&repo_path));
                }
                self.refresh_wip_diffstat();
                self.refresh_worktree_wip_row(&repo_path);
            }
            Err(e) => {
                self.stage_failure(StageAction::UnstageAll, &repo_path, &paths, &e, cx);
            }
        }
    }
}
