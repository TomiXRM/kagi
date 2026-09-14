//! Mutating helpers on [`TabUiState`], split out of `tab_view.rs` (which is at
//! its LOC ceiling) so the state invariants live beside each other rather than
//! being respelled at every call site.

use super::tab_view::TabUiState;
use gpui::Entity;

impl TabUiState {
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

    /// Tick / untick one Branch Cleanup row.
    pub fn toggle_cleanup_selection(&mut self, name: String) {
        if !self.cleanup_selected.remove(&name) {
            self.cleanup_selected.insert(name);
        }
    }
}
