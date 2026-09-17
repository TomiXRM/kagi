//! The Graph | PRs | Issues | Editor workspace-mode switcher.
//!
//! Four mutually exclusive top-level modes. Each navigation control names the
//! mode it selects and lights up while that mode is on screen. They used to
//! be two independent toggles that each morphed into a "Graph" button; with
//! two takeovers open at once both read "Graph" and neither said which one
//! you would land in (user report).

use super::{theme, EditorPendingIntent, KagiApp};
use gpui::{div, prelude::*, rgb, Context, SharedString};

use super::workspace::WorkspaceItem;

/// Which top-level workspace mode is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceMode {
    Graph,
    Prs,
    Issues,
    Editor,
    /// A center takeover that is none of the named modes — File History,
    /// Analyze, Branch Cleanup. No mode control owns it, so none lights up.
    /// Without this the Graph button claimed to be the active mode while
    /// something else entirely was on screen.
    Takeover,
}

/// One mode cell in the sidebar's pinned navigation row. Mode selection remains
/// here with the canonical dispatchers; the sidebar only hosts the element.
fn sidebar_mode_nav_cell(
    id: &'static str,
    label: &'static str,
    active: bool,
    enabled: bool,
    cx: &mut Context<KagiApp>,
    on_click: impl Fn(&mut KagiApp, &gpui::ClickEvent, &mut gpui::Window, &mut Context<KagiApp>)
        + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .flex_1()
        .py_1()
        .flex()
        .justify_center()
        .text_xs()
        .rounded(theme::scaled_px(4.))
        .when(enabled, |el| el.cursor_pointer())
        .when(active, |el| {
            el.bg(rgb(theme::theme().surface))
                .text_color(rgb(theme::theme().color_branch))
                .font_weight(gpui::FontWeight::MEDIUM)
        })
        .when(!active, |el| el.text_color(rgb(theme::theme().text_muted)))
        .when(enabled, |el| el.on_click(cx.listener(on_click)))
        .child(SharedString::from(label))
        .into_any_element()
}

/// Graph / PRs / Issues navigation, rendered in the sidebar but owned by
/// workspace-mode dispatch so the visual highlight and resolved center takeover cannot drift.
pub(super) fn render_sidebar_mode_nav(
    mode: WorkspaceMode,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let prs_available = kagi_git::github::gh_available();
    super::e2e::measure_control(
        "sidebar-mode-nav",
        div()
            .id("sidebar-mode-nav")
            .flex_shrink_0()
            .mx_2()
            .mt_1()
            .mb_1()
            .flex()
            .gap_1()
            .child(sidebar_mode_nav_cell(
                "sidebar-mode-graph",
                "Graph",
                mode == WorkspaceMode::Graph,
                true,
                cx,
                |this, _, _window, cx| this.show_graph_mode(cx),
            ))
            .when(prs_available, |el| {
                el.child(sidebar_mode_nav_cell(
                    "sidebar-mode-prs",
                    "PRs",
                    mode == WorkspaceMode::Prs,
                    true,
                    cx,
                    |this, _, _window, cx| this.show_pr_mode(cx),
                ))
                .child(sidebar_mode_nav_cell(
                    "sidebar-mode-issues",
                    "Issues",
                    mode == WorkspaceMode::Issues,
                    true,
                    cx,
                    |this, _, _window, cx| this.show_issues_mode(cx),
                ))
            }),
    )
}

impl KagiApp {
    pub(super) fn sidebar_scroll(
        &mut self,
        event: &gpui::ScrollWheelEvent,
        _window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        use gpui::{ScrollDelta, TouchPhase};
        use kagi_domain::sidebar_swipe::SidebarSwipeResult;

        if self.active_modal.is_some() {
            self.sidebar.swipe.cancel();
            return;
        }
        match event.touch_phase {
            TouchPhase::Started => self.sidebar.swipe.start(),
            TouchPhase::Cancelled => {
                self.sidebar.swipe.cancel();
                return;
            }
            TouchPhase::Moved | TouchPhase::Ended => {}
        }
        let (x, y) = match event.delta {
            ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
            ScrollDelta::Lines(p) => (p.x * 24.0, p.y * 24.0),
        };
        // Trackpad deltas are physical input distances; UI zoom must not change
        // how far the user's fingers have to travel to commit the same swipe.
        self.sidebar.swipe.move_by(x, y);
        if event.touch_phase != TouchPhase::Ended {
            return;
        }

        let mode = self.workspace_mode();
        let result = self.sidebar.swipe.finish();
        match (mode, result) {
            (WorkspaceMode::Graph, SidebarSwipeResult::Next)
                if kagi_git::github::gh_available() =>
            {
                self.show_pr_mode(cx);
            }
            (WorkspaceMode::Prs, SidebarSwipeResult::Previous) => self.show_graph_mode(cx),
            (WorkspaceMode::Prs, SidebarSwipeResult::Next) if kagi_git::github::gh_available() => {
                self.show_issues_mode(cx);
            }
            (WorkspaceMode::Issues, SidebarSwipeResult::Previous) => self.show_pr_mode(cx),
            _ => {}
        }
    }

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
        self.with_ui(|ui| ui.branch_cleanup_open = false);
        if keep != WorkspaceMode::Prs {
            // PR mode outranks Editor, so Editor has to displace it too.
            self.with_ui(|ui| ui.pr_mode = None);
        }
        if keep != WorkspaceMode::Issues {
            // Invalidate completions as well as hiding the mode: a request that
            // lands after Graph was selected must not reopen the workspace.
            self.with_ui(|ui| {
                ui.github_issues_gen = ui.github_issues_gen.wrapping_add(1);
                ui.github_issue_detail_gen = ui.github_issue_detail_gen.wrapping_add(1);
                ui.github_issues_loading = false;
                ui.github_issues_loaded = false;
                ui.github_issues_error = None;
                ui.github_issue_detail_loading = None;
                ui.github_issue_detail_error = None;
                ui.selected_github_issue = None;
            });
        }
    }

    /// The mode the resolver will show. PR and Issues mode both outrank Editor
    /// (`resolve_workspace`), so an editor open behind either takeover is not
    /// the active mode — the toolbar must agree with what is on screen.
    pub fn workspace_mode(&self) -> WorkspaceMode {
        if super::workspace::FileHistoryItem.is_open(self)
            || self.ui().ecosystem.is_some()
            || self.ui().branch_cleanup_open
        {
            WorkspaceMode::Takeover
        } else if self.pr_mode().is_some() {
            WorkspaceMode::Prs
        } else if self.issues_mode_open() {
            WorkspaceMode::Issues
        } else if self.ui().editor_workspace.is_some() {
            WorkspaceMode::Editor
        } else {
            WorkspaceMode::Graph
        }
    }

    /// Graph: leave both takeovers. The editor closes through the dirty
    /// guard, so unsaved buffers still prompt.
    pub fn show_graph_mode(&mut self, cx: &mut Context<Self>) {
        self.sidebar.swipe.cancel();
        self.leave_takeovers(WorkspaceMode::Graph);
        if let Some(ev) = self.ui().editor_workspace.clone() {
            if ev.read(cx).any_dirty() {
                self.open_editor_dirty_guard(EditorPendingIntent::Close, cx);
            } else {
                self.close_editor_workspace();
            }
        }
        klog!(
            "menu: editor_workspace={}",
            self.ui().editor_workspace.is_some()
        );
        klog!("mode: graph");
        cx.notify();
    }

    /// PRs: show PR mode. An open editor stays alive underneath (its unsaved
    /// work is preserved) and the Editor button brings it straight back.
    pub fn show_pr_mode(&mut self, cx: &mut Context<Self>) {
        self.sidebar.swipe.cancel();
        self.leave_takeovers(WorkspaceMode::Prs);
        if self.pr_mode().is_none() {
            self.toggle_pr_mode(cx);
        }
        klog!("mode: prs");
        cx.notify();
    }

    /// Issues: open the session-owned read-only list and refresh it once.
    pub fn show_issues_mode(&mut self, cx: &mut Context<Self>) {
        self.sidebar.swipe.cancel();
        self.leave_takeovers(WorkspaceMode::Issues);
        if !self.issues_mode_open() {
            self.refresh_github_issues(cx);
        }
        klog!("mode: issues");
        cx.notify();
    }

    pub(super) fn issues_mode_open(&self) -> bool {
        let ui = self.ui();
        ui.github_issues_loading || ui.github_issues_loaded || ui.github_issues_error.is_some()
    }

    /// Editor: reveal the existing workspace, or create one. Either way PR
    /// mode steps aside (it outranks Editor in the resolver, which is why
    /// pressing Editor from PR mode used to do nothing — user report).
    pub fn show_editor_mode(&mut self, cx: &mut Context<Self>) {
        self.sidebar.swipe.cancel();
        self.leave_takeovers(WorkspaceMode::Editor);
        if self.ui().editor_workspace.is_none() {
            self.open_editor_workspace(cx);
        }
        klog!(
            "menu: editor_workspace={}",
            self.ui().editor_workspace.is_some()
        );
        klog!("mode: editor");
        cx.notify();
    }
}
