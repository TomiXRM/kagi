//! Stage / Unstage one hunk from the Commit Panel's diff (#842, Refs #357).
//!
//! Rides the same write path as the per-file buttons (`do_stage_file`): the
//! pane-mutation gate, the stage write lease, and #490's failure delivery
//! (oplog + footer + notice) with the same `stage` / `unstage` op names. The
//! backend re-reads the diff and refuses a hunk whose header no longer exists
//! (`HunkChanged`). Either way the open diff is re-read afterwards, so what
//! the pane shows is what the index now holds.

use super::staging_failure::StageAction;
use crate::ui::main_diff_pane::MainDiffRead;
use crate::ui::*;
use kagi_domain::diff::HunkRange;
use std::path::PathBuf;

impl KagiApp {
    /// Stage (`staged == false`, from the unstaged diff) or unstage
    /// (`staged == true`, from the staged diff) the hunk of `path` whose
    /// header is `range`.
    pub fn stage_hunk_from_diff(
        &mut self,
        owner: crate::app::SessionId,
        path: PathBuf,
        range: HunkRange,
        staged: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.pane_mutation_admitted(owner) {
            return;
        }
        // #476: the PANEL's repository, never the tab's.
        let Some(repo_path) = self.commit_panel_repo_path(cx) else {
            return;
        };
        let action = if staged {
            StageAction::Unstage
        } else {
            StageAction::Stage
        };
        let paths = [path.clone()];
        let Some(lease) = self.reserve_stage_write(action, &repo_path, &paths, cx) else {
            return;
        };
        let result = lease.run(|| {
            self.with_staging_repo(&repo_path, |repo| {
                if staged {
                    repo.unstage_hunk(&path, range)
                } else {
                    repo.stage_hunk(&path, range)
                }
            })
        });
        self.refresh_write_busy();
        let header = format!(
            "@@ -{},{} +{},{} @@",
            range.old.0, range.old.1, range.new.0, range.new.1
        );
        match result.and_then(|r| r) {
            Ok(()) => klog!(
                "{} hunk: {} {}",
                if staged { "unstaged" } else { "staged" },
                path.display(),
                header
            ),
            Err(e) => {
                klog!("hunk staging error: {}", e);
                self.stage_failure(action, &repo_path, &paths, &e, cx);
            }
        }
        if let Some(entity) = self.ui().commit_panel.clone() {
            entity.update(cx, |v, _| {
                v.state.reload_status(&repo_path);
                klog!(
                    "commit-panel: unstaged={} staged={}",
                    v.state.unstaged.len(),
                    v.state.staged.len()
                );
            });
        }
        self.start_wip_diffstat_scan(cx);
        self.refresh_worktree_wip_row(&repo_path);
        // The pane keeps showing this side of the file, re-read; it closes
        // when this side has nothing left.
        let read = MainDiffRead::Wip {
            staged,
            refresh: true,
        };
        self.read_main_diff(repo_path, path, read, cx);
        cx.notify();
    }
}
