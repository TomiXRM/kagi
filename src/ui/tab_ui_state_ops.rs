//! Mutating helpers on [`TabUiState`], split out of `tab_view.rs` (which is at
//! its LOC ceiling) so the state invariants live beside each other rather than
//! being respelled at every call site.

use super::tab_view::TabUiState;
use gpui::Entity;

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

    /// Tick / untick one Branch Cleanup row.
    pub fn toggle_cleanup_selection(&mut self, name: String) {
        if !self.cleanup_selected.remove(&name) {
            self.cleanup_selected.insert(name);
        }
    }
}
