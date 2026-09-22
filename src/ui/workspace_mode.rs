//! The Graph | PRs | Issues | Editor workspace-mode switcher.
//!
//! Four mutually exclusive top-level modes. Each navigation control names the
//! mode it selects and lights up while that mode is on screen. They used to
//! be two independent toggles that each morphed into a "Graph" button; with
//! two takeovers open at once both read "Graph" and neither said which one
//! you would land in (user report).

use super::{theme, EditorPendingIntent, KagiApp};
use gpui::{div, prelude::*, px, relative, rgb, Context, SharedString};

use super::i18n::Msg;
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

/// Shared PR/Issue navigator section chrome: disclosure state, label, count,
/// padding and hover behavior stay identical across both GitHub pages.
pub(super) fn sidebar_section_header(
    id: (&'static str, usize),
    label: &'static str,
    (count, has_more): (usize, bool),
    open: bool,
    emphasize_count: bool,
    cx: &mut Context<KagiApp>,
    on_click: impl Fn(&mut KagiApp, &gpui::ClickEvent, &mut gpui::Window, &mut Context<KagiApp>)
        + 'static,
) -> gpui::AnyElement {
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px_3()
        .pt_2()
        .pb_1()
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme::theme().surface)))
        .on_click(cx.listener(on_click))
        .child(
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme::theme().text_muted))
                .child(SharedString::from(if open {
                    "\u{25BE}"
                } else {
                    "\u{25B8}"
                })),
        )
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme::theme().text_muted))
                .child(SharedString::from(label)),
        )
        .child(div().flex_1().min_w(px(0.)))
        .child(
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(if count > 0 && emphasize_count {
                    theme::theme().color_branch
                } else {
                    theme::theme().text_muted
                }))
                .child(SharedString::from(if has_more && count > 0 {
                    format!("({count}+)")
                } else {
                    count.to_string()
                })),
        )
        .into_any_element()
}

/// Shared selectable row shell for GitHub sidebar lists. Feature renderers
/// provide only their content and handlers; spacing and selection colors are
/// intentionally owned here once.
pub(super) fn sidebar_list_row(active: bool) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .overflow_hidden()
        .cursor_pointer()
        .border_b_1()
        .border_color(rgb(theme::theme().surface))
        .when(active, |el| el.bg(rgb(theme::theme().selected)))
        .hover(|s| s.bg(rgb(theme::theme().surface)))
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
                Msg::WorkspaceGraph.t(),
                mode == WorkspaceMode::Graph,
                true,
                cx,
                |this, _, _window, cx| this.show_graph_mode(cx),
            ))
            .when(prs_available, |el| {
                el.child(sidebar_mode_nav_cell(
                    "sidebar-mode-prs",
                    Msg::WorkspacePrs.t(),
                    mode == WorkspaceMode::Prs,
                    true,
                    cx,
                    |this, _, _window, cx| this.show_pr_mode(cx),
                ))
                .child(sidebar_mode_nav_cell(
                    "sidebar-mode-issues",
                    Msg::WorkspaceIssues.t(),
                    mode == WorkspaceMode::Issues,
                    true,
                    cx,
                    |this, _, _window, cx| this.show_issues_mode(cx),
                ))
            }),
    )
}

/// The ordered sidebar pages one gesture can move between (ADR-0199).
///
/// Editor and the unnamed takeovers are deliberately absent: they are entered
/// explicitly, and a gesture inside them must not navigate. Without `gh` there
/// is a single page, so every gesture snaps back.
pub(super) fn nav_pages() -> &'static [WorkspaceMode] {
    if kagi_git::github::gh_available() {
        &[
            WorkspaceMode::Graph,
            WorkspaceMode::Prs,
            WorkspaceMode::Issues,
        ]
    } else {
        &[WorkspaceMode::Graph]
    }
}

fn page_index(mode: WorkspaceMode) -> Option<usize> {
    nav_pages().iter().position(|m| *m == mode)
}

/// One animation step of the settle. A fixed step keeps the spring identical
/// under a dropped frame and under the test clock.
const SETTLE_FRAME: std::time::Duration = std::time::Duration::from_millis(16);
const SETTLE_DT: f32 = 0.016;

/// One sidebar page, translated to `left` inside the clipped viewport.
///
/// Absolutely positioned on purpose: a margin would shrink the page's content
/// box and reflow the list as it slides, and the gesture must translate the
/// page, not re-lay it out.
fn sidebar_page(left: f32) -> gpui::Div {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(left))
        .w_full()
        .flex()
        .flex_col()
        .bg(rgb(theme::theme().sidebar))
}

/// The content of one sidebar page.
///
/// The single place a page's body is built, so the page a gesture is heading
/// for looks exactly like the page it lands on. Every arm is a pure read of
/// already-loaded state: none of them starts a fetch (that is what the
/// `show_*_mode` dispatchers do). `Editor` and the unnamed takeovers keep the
/// navigator, as they did before pages existed.
fn page_content(app: &KagiApp, mode: WorkspaceMode, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    match mode {
        WorkspaceMode::Prs => super::pr_nav::render_pr_list(app, cx),
        WorkspaceMode::Issues => super::issues_mode::render_issue_list(app, cx),
        _ => super::sidebar::render_sidebar(app, cx),
    }
}

/// Whether `mode`'s page can be drawn from state that is already loaded.
///
/// The Graph page is local Git data, flattened into `sidebar.rows` every frame
/// regardless of which page is on screen, so it is always ready. The GitHub
/// pages are only ready once their list has arrived; showing an empty PR or
/// Issue page mid-gesture would claim the neighbour has nothing in it.
fn page_is_cached(app: &KagiApp, mode: WorkspaceMode) -> bool {
    match mode {
        WorkspaceMode::Prs => !app.ui().github_prs.is_empty(),
        WorkspaceMode::Issues => !app.ui().github_issues.is_empty(),
        WorkspaceMode::Graph => true,
        WorkspaceMode::Editor | WorkspaceMode::Takeover => false,
    }
}

/// What an adjacent page shows before its list has ever loaded: enough shape to
/// read as a page sliding in, with nothing invented in it.
fn adjacent_page_placeholder() -> gpui::AnyElement {
    super::e2e::measure_control(
        "sidebar-adjacent-page-shell",
        div()
            .id("sidebar-adjacent-page-shell")
            .flex_1()
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_3()
            .children([1.0, 0.76, 0.92, 0.61, 0.84].into_iter().map(|w| {
                div()
                    .h(theme::scaled_px(14.))
                    .w(relative(w))
                    .rounded(theme::scaled_px(3.))
                    .bg(rgb(theme::theme().surface))
            })),
    )
}

/// The one sidebar renderer (ADR-0199).
///
/// A stationary, fixed-width shell hosts the pinned page navigator and a
/// clipped viewport. Inside the viewport the current page slides by the
/// gesture's resisted offset and the adjacent page follows it in from the
/// side being uncovered. Nothing here is visible to the main pane: the caller
/// passes only its own page content, and the offset never leaves this module.
///
/// The navigator stays outside the viewport so it remains clickable and
/// anchored, and so the shell keeps receiving wheel phases after the
/// translated page has moved out from under the pointer — `Ended` must always
/// arrive, or a gesture could never be released.
pub(super) fn render_sidebar_pages(
    app: &KagiApp,
    mode: WorkspaceMode,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let offset = app.sidebar.swipe.offset();
    let width = f32::from(theme::scaled_px(app.sidebar.width));
    let mut viewport = div()
        .relative()
        .flex_1()
        .min_h(px(0.))
        .overflow_hidden()
        .child(sidebar_page(offset).child(page_content(app, mode, cx)));
    if offset != 0.0 {
        // The neighbour follows from the side the current page is uncovering.
        let neighbour_left = offset - width * offset.signum();
        let neighbour = page_index(mode)
            .and_then(|origin| {
                let target = if offset < 0.0 {
                    origin.checked_add(1)?
                } else {
                    origin.checked_sub(1)?
                };
                nav_pages().get(target).copied()
            })
            .filter(|target| page_is_cached(app, *target));
        // `measure_control` anchors its own relative wrapper, so it goes
        // *inside* the positioned page — never around it.
        viewport = viewport.child(
            sidebar_page(neighbour_left).child(super::e2e::measure_control(
                "sidebar-adjacent-page",
                match neighbour {
                    // Already-loaded evidence: show the real page the gesture is
                    // heading for. Rendering reads cached state only — it never
                    // starts a fetch, which is what opening the mode does.
                    Some(target) => page_content(app, target, cx),
                    None => adjacent_page_placeholder(),
                },
            )),
        );
    }
    if app.sidebar.swipe.owns_wheel() {
        // One occluding layer takes the wheel for as long as the gesture owns
        // it. `Hitbox::should_handle_scroll` is false for everything a
        // `BlockMouse` hitbox covers (gpui `window.rs` hit test), so the list
        // under the pointer cannot consume the gesture's `y` delta — a
        // horizontal swipe has no vertical component. It sits inside the
        // viewport, below the pinned navigator, and carries the same listener
        // so the gesture keeps being fed while it is blocking.
        viewport = viewport.child(super::e2e::measure_control(
            "sidebar-gesture-shield",
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .on_scroll_wheel(cx.listener(KagiApp::sidebar_scroll)),
        ));
    }

    div()
        // `sidebar.width` is the unscaled, persisted width; scale at render so
        // it tracks zoom uniformly with the text. The resize/drag math in
        // `render_divider` interprets cursor deltas in the same scaled space.
        .w(theme::scaled_px(app.sidebar.width))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme::theme().sidebar))
        .on_scroll_wheel(cx.listener(KagiApp::sidebar_scroll))
        .child(render_sidebar_mode_nav(mode, cx))
        .child(viewport)
        .into_any_element()
}

impl KagiApp {
    /// Trackpad wheel phases drive the gesture; only the sidebar moves.
    ///
    /// `Started` fixes the origin page for the whole gesture, `Moved`
    /// accumulates raw distance, `Ended` decides and hands over to the settle.
    /// macOS reports momentum scroll as `Moved` with no preceding `Started`
    /// (`gpui_macos::events`), which the idle state machine ignores.
    pub(super) fn sidebar_scroll(
        &mut self,
        event: &gpui::ScrollWheelEvent,
        _window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        use gpui::{ScrollDelta, TouchPhase};

        if self.active_modal.is_some() {
            self.abandon_sidebar_gesture(cx);
            return;
        }
        match event.touch_phase {
            TouchPhase::Started => {
                let Some(origin) = page_index(self.workspace_mode()) else {
                    return;
                };
                // The commit boundary is a fraction of the sidebar the user can
                // see, so it is measured in the same rendered pixels the
                // viewport is laid out in.
                let width = f32::from(theme::scaled_px(self.sidebar.width));
                self.sidebar.swipe.start(origin, nav_pages().len(), width);
            }
            TouchPhase::Cancelled => {
                self.abandon_sidebar_gesture(cx);
                return;
            }
            TouchPhase::Moved | TouchPhase::Ended => {}
        }
        let (x, y) = match event.delta {
            ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
            ScrollDelta::Lines(p) => (p.x * 24.0, p.y * 24.0),
        };
        let before = self.sidebar.swipe.offset();
        self.sidebar.swipe.move_by(x, y);
        if event.touch_phase == TouchPhase::Ended {
            self.release_sidebar_gesture(cx);
        } else if self.sidebar.swipe.offset() != before {
            cx.notify();
        }
    }

    /// Fingers lifted: spring the sidebar from where it is to its snap target.
    /// The page the gesture chose is applied only when that settle finishes.
    fn release_sidebar_gesture(&mut self, cx: &mut Context<Self>) {
        use kagi_domain::sidebar_swipe::Settle;

        self.sidebar.swipe.release();
        self.sidebar.settle_gen = self.sidebar.settle_gen.wrapping_add(1);
        if !self.sidebar.swipe.is_settling() {
            // Nothing was dragged (the gesture was a list scroll), so there is
            // no animation to run and no frame loop to spawn.
            return;
        }
        // Reduce motion (ADR-0173): no animation, so the navigation is
        // immediate — but it still runs through the same settle terminal.
        if theme::reduce_motion() {
            if let Settle::Finished(page) = self.sidebar.swipe.complete() {
                self.finish_sidebar_settle(page, cx);
            }
            return;
        }
        let gen = self.sidebar.settle_gen;
        cx.notify();
        cx.spawn(async move |this, acx| loop {
            acx.background_executor().timer(SETTLE_FRAME).await;
            let running = this.update(acx, |app, cx| {
                if app.sidebar.settle_gen != gen {
                    return false;
                }
                match app.sidebar.swipe.tick(SETTLE_DT) {
                    Settle::Running => {
                        cx.notify();
                        true
                    }
                    Settle::Finished(page) => {
                        app.finish_sidebar_settle(page, cx);
                        false
                    }
                    Settle::Idle => false,
                }
            });
            if !matches!(running, Ok(true)) {
                return;
            }
        })
        .detach();
    }

    /// The settle rested: only now does the logical navigation happen. The
    /// state machine has already cleared the pending page and normalised the
    /// offset to 0, so switching the mode re-bases the sidebar on the new
    /// active page and moves the main pane with it — in that order.
    fn finish_sidebar_settle(&mut self, page: Option<usize>, cx: &mut Context<Self>) {
        match page.and_then(|i| nav_pages().get(i).copied()) {
            Some(WorkspaceMode::Graph) => self.show_graph_mode(cx),
            Some(WorkspaceMode::Prs) => self.show_pr_mode(cx),
            Some(WorkspaceMode::Issues) => self.show_issues_mode(cx),
            _ => cx.notify(),
        }
    }

    /// Drop the gesture (and any settle in flight) without navigating.
    fn abandon_sidebar_gesture(&mut self, cx: &mut Context<Self>) {
        if self.sidebar.swipe.is_idle() {
            return;
        }
        self.sidebar.settle_gen = self.sidebar.settle_gen.wrapping_add(1);
        self.sidebar.swipe.cancel();
        cx.notify();
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
            self.with_ui(|ui| ui.leave_pr_mode());
        }
        if keep != WorkspaceMode::Issues {
            // Invalidate completions as well as hiding the mode: a request that
            // lands after Graph was selected must not reopen the workspace.
            self.with_ui(|ui| {
                ui.github_issues_gen = ui.github_issues_gen.wrapping_add(1);
                ui.github_issue_detail_gen = ui.github_issue_detail_gen.wrapping_add(1);
                ui.github_issues_loading = false;
                ui.github_issues_loading_more = false;
                ui.github_issues_cursor = None;
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
