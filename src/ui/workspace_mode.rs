//! The Graph | PRs | Editor workspace-mode switcher.
//!
//! Three mutually exclusive top-level modes. Each toolbar button names the
//! mode it selects and lights up while that mode is on screen. They used to
//! be two independent toggles that each morphed into a "Graph" button; with
//! two takeovers open at once both read "Graph" and neither said which one
//! you would land in (user report).

use super::{EditorPendingIntent, KagiApp};
use gpui::Context;

use super::workspace::WorkspaceItem;

/// Which of the three top-level workspace modes is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceMode {
    Graph,
    Prs,
    Editor,
    /// A center takeover that is none of the three — File History, Analyze,
    /// Branch Cleanup. No toolbar button owns it, so none of them lights up.
    /// Without this the Graph button claimed to be the active mode while
    /// something else entirely was on screen.
    Takeover,
}

impl KagiApp {
    /// Close every center takeover that outranks `keep` in `resolve_workspace`.
    ///
    /// Each entry point used to close its own ad-hoc subset, and none of them
    /// knew about Branch Cleanup — so with the cleanup table open, Graph, PRs
    /// and Editor all did nothing visible while the toolbar lit their button up
    /// as the active mode. One list, derived from the resolver's order, is the
    /// only way these stay in step as panes are added.
    pub(crate) fn leave_takeovers(&mut self, keep: WorkspaceMode) {
        self.close_file_history();
        self.close_ecosystem_view();
        self.branch_cleanup_open = false;
        if keep != WorkspaceMode::Prs {
            // PR mode outranks Editor, so Editor has to displace it too.
            self.pr_mode = None;
        }
    }

    /// The mode the resolver will show. PR mode outranks Editor
    /// (`resolve_workspace`), so an editor open *behind* PR mode is not the
    /// active mode — the toolbar must agree with what is on screen.
    pub fn workspace_mode(&self) -> WorkspaceMode {
        if super::workspace::FileHistoryItem.is_open(self)
            || self.ecosystem.is_some()
            || self.branch_cleanup_open
        {
            WorkspaceMode::Takeover
        } else if self.pr_mode.is_some() {
            WorkspaceMode::Prs
        } else if self.editor_workspace.is_some() {
            WorkspaceMode::Editor
        } else {
            WorkspaceMode::Graph
        }
    }

    /// Graph: leave both takeovers. The editor closes through the dirty
    /// guard, so unsaved buffers still prompt.
    pub fn show_graph_mode(&mut self, cx: &mut Context<Self>) {
        self.leave_takeovers(WorkspaceMode::Graph);
        if let Some(ev) = self.editor_workspace.clone() {
            if ev.read(cx).any_dirty() {
                self.open_editor_dirty_guard(EditorPendingIntent::Close, cx);
            } else {
                self.close_editor_workspace();
            }
        }
        klog!("menu: editor_workspace={}", self.editor_workspace.is_some());
        klog!("mode: graph");
        cx.notify();
    }

    /// PRs: show PR mode. An open editor stays alive underneath (its unsaved
    /// work is preserved) and the Editor button brings it straight back.
    pub fn show_pr_mode(&mut self, cx: &mut Context<Self>) {
        self.leave_takeovers(WorkspaceMode::Prs);
        if self.pr_mode.is_none() {
            self.toggle_pr_mode(cx);
        }
        klog!("mode: prs");
        cx.notify();
    }

    /// Editor: reveal the existing workspace, or create one. Either way PR
    /// mode steps aside (it outranks Editor in the resolver, which is why
    /// pressing Editor from PR mode used to do nothing — user report).
    pub fn show_editor_mode(&mut self, cx: &mut Context<Self>) {
        self.leave_takeovers(WorkspaceMode::Editor);
        if self.editor_workspace.is_none() {
            self.open_editor_workspace(cx);
        }
        klog!("menu: editor_workspace={}", self.editor_workspace.is_some());
        klog!("mode: editor");
        cx.notify();
    }
}
