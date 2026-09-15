//! Mutating helpers on [`TabUiState`], split out of `tab_view.rs` (which is at
//! its LOC ceiling) so the state invariants live beside each other rather than
//! being respelled at every call site.

use super::tab_view::TabUiState;
use gpui::Entity;
use std::collections::HashSet;

/// Whether a session's retained panes may act on the repository (ADR-0197
/// 決定 3 / #722). One value, not a pair of flags, so no order of events can
/// leave "read accepted" and "conflict checked" disagreeing about the gate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaneRevalidation {
    /// Checked against the read on screen: mutations are admitted.
    #[default]
    Settled,
    /// The tab was activated and its panes still describe the read it had
    /// before it was left. Only a read **accepted after** that activation ends
    /// this — re-showing the cached read on switch does not.
    AwaitingRead,
    /// A new read was published; `render` owes the panes a pass against it.
    Queued,
    /// That pass is done, but a conflict is in progress and only a detector
    /// run against the accepted read can say it is still the same one.
    AwaitingConflict,
}

impl super::KagiApp {
    /// `session`'s current read is the one its panes must be checked against.
    pub(crate) fn queue_pane_revalidation(&mut self, session: crate::app::SessionId) {
        if let Some(ui) = self.ui.get_mut(&session) {
            ui.pane_revalidation = PaneRevalidation::Queued;
        }
    }
}

impl TabUiState {
    /// Mutations from this session's retained panes are refused.
    pub fn panes_revalidating(&self) -> bool {
        self.pane_revalidation != PaneRevalidation::Settled
    }

    /// Show `panel` as this session's commit panel. Commit *selection* and the
    /// commit panel are mutually exclusive, and the open diff belongs to the
    /// selection, so both are cleared here rather than at each call site.
    pub fn open_commit_panel(&mut self, panel: Entity<super::commit_panel::CommitPanelView>) {
        self.commit_panel = Some(panel);
        self.commit_panel_open = true;
        self.selected = None;
        self.main_diff = None;
    }

    /// Replace the Smart Commit status line.
    pub fn set_smart_commit_status(&mut self, msg: &str) {
        self.smart_commit_status = Some(msg.to_string());
    }

    /// Collapse a sidebar group, or expand it when it is already collapsed.
    pub fn toggle_branch_group(&mut self, key: &str) {
        if !self.branch_groups_collapsed.remove(key) {
            self.branch_groups_collapsed.insert(key.to_string());
        }
    }

    /// ADR-0197 決定 4: `Err(field)` names the first piece of user intent, pane
    /// or session resource this state holds that a tab nobody has touched would
    /// not. The destructuring has no `..`, so adding a field does not compile
    /// until it is classified here. Fields `_`-bound are filled by the owner's
    /// **own** reads and probes (evidence, caches, revisions, the repository
    /// session), so a fresh tab legitimately holds them; a leak there is caught
    /// by the leak matrix comparing values, not by this probe.
    pub fn is_pristine(&self) -> Result<(), &'static str> {
        let Self {
            selected,
            // Scroll handles expose their position only under gpui `test-support`;
            // the leak matrix observes both (ADR-0197 決定 4, last paragraph).
            commit_scroll_handle: _,
            commit_limit,
            graph_scroll_x,
            branch_groups_collapsed,
            cleanup_scroll: _,
            cleanup_selected,
            branch_cleanup_open,
            pr_mode,
            pr_menu,
            smart_commit_generating,
            smart_commit_status,
            view_publish_gen: _,
            cache_epoch: _,
            diff_caches: _,
            wip_diffstat: _,
            last_working_status: _,
            operation_history: _,
            history_seed_attempted: _,
            github_prs: _,
            github_prs_loaded: _,
            github_error: _,
            github_unavailable: _,
            github_prs_epoch: _,
            cleanup_gen: _,
            cleanup_scanning: _,
            cleanup_prs: _,
            cleanup_prs_stale: _,
            conflict_detected: _,
            pane_revalidation: _,
            ecosystem_cache,
            ecosystem_inflight,
            ecosystem_gen: _,
            ecosystem_mine_head,
            repo_session: _,
            terminal_session,
            conflict,
            conflict_merge_pending,
            file_history,
            file_history_head,
            ecosystem,
            editor_workspace,
            commit_panel,
            commit_panel_open,
            main_diff,
            compare_view,
        } = self;
        let default_groups = HashSet::from([super::sidebar::PR_GROUP_OTHERS.to_string()]);
        let dirty = [
            (selected.is_some(), "selected"),
            (*commit_limit != super::DEFAULT_COMMIT_LIMIT, "commit_limit"),
            (*graph_scroll_x != 0.0, "graph_scroll_x"),
            (
                *branch_groups_collapsed != default_groups,
                "branch_groups_collapsed",
            ),
            (!cleanup_selected.is_empty(), "cleanup_selected"),
            (*branch_cleanup_open, "branch_cleanup_open"),
            (pr_mode.is_some(), "pr_mode"),
            (pr_menu.is_some(), "pr_menu"),
            (*smart_commit_generating, "smart_commit_generating"),
            (smart_commit_status.is_some(), "smart_commit_status"),
            (ecosystem_cache.is_some(), "ecosystem_cache"),
            (*ecosystem_inflight, "ecosystem_inflight"),
            (ecosystem_mine_head.is_some(), "ecosystem_mine_head"),
            (terminal_session.is_some(), "terminal_session"),
            (conflict.is_some(), "conflict"),
            (*conflict_merge_pending, "conflict_merge_pending"),
            (file_history.is_some(), "file_history"),
            (file_history_head.is_some(), "file_history_head"),
            (ecosystem.is_some(), "ecosystem"),
            (editor_workspace.is_some(), "editor_workspace"),
            (commit_panel.is_some(), "commit_panel"),
            (*commit_panel_open, "commit_panel_open"),
            (main_diff.is_some(), "main_diff"),
            (compare_view.is_some(), "compare_view"),
        ];
        match dirty.into_iter().find(|(is_dirty, _)| *is_dirty) {
            Some((_, field)) => Err(field),
            None => Ok(()),
        }
    }

    /// Tick / untick one Branch Cleanup row.
    pub fn toggle_cleanup_selection(&mut self, name: String) {
        if !self.cleanup_selected.remove(&name) {
            self.cleanup_selected.insert(name);
        }
    }
}

impl super::KagiApp {
    /// PR mode of the tab on screen (ADR-0197: a workspace mode is per-session).
    pub fn pr_mode(&self) -> Option<&super::pr_mode::PrModeState> {
        self.ui().pr_mode.as_ref()
    }
    /// Foreground writer for [`Self::pr_mode`]. A background completion must
    /// write through the owner it froze at spawn, never through this.
    pub(crate) fn pr_mode_mut(&mut self) -> Option<&mut super::pr_mode::PrModeState> {
        self.ui_mut()?.pr_mode.as_mut()
    }
    /// The PR mode of `owner`, the session a background load froze at spawn.
    pub(crate) fn pr_mode_of(
        &mut self,
        owner: Option<crate::app::SessionId>,
    ) -> Option<&mut super::pr_mode::PrModeState> {
        self.ui.get_mut(&owner?)?.pr_mode.as_mut()
    }
}
