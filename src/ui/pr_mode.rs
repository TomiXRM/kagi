//! PR mode (GitHub Phase 1c) — a workspace mode centred on pull requests,
//! the way Editor mode is centred on files.
//!
//! ```text
//! ┌ PR list ─┐┌ █#239 │ #238 │ ✕ ────────────────┐┌ STACK ────────┐
//! │ Mine     ││ #239 title  ✓ CI  approved  [GitHub↗]││  ● #239 ←     │
//! │  #239    ││ commits (3)                         ││  ● #238       │
//! │  #238    ││  ● e4a4059 feat: pane…               ││  ● main       │
//! │ Review   ││  ● 4b72ed4 feat: sidebar…            │├ FILES (12) ───┤
//! │ Others ▸ ││─ diff ──────────────────────────────││  M pr_pane.rs  │
//! │          ││ + fn render_pr_pane(…                ││  A github.rs   │
//! └──────────┘└──────────────────────────────────────┘└───────────────┘
//! ```
//!
//! Everything is read-only and built from the fetched `origin/*` tips: a PR
//! tab holds `merge-base(base, head)..head` (GitHub's three-dot view), its
//! commits, its changed files, and the diff of the selected file — for the
//! whole PR, or for one selected commit. Nothing is checked out. Inputs
//! (creating / editing a PR) deliberately go to GitHub's own UI.

use gpui::{div, prelude::*, px, relative, rgb, Context, ListState, SharedString};
use kagi_domain::github::{stack_order, CiState, PrGroup, PullRequest, ReviewState};
use kagi_git::{Commit, CommitId, FileStatus};
use kagi_ui_core::file_tree::status_badge;

use super::diff_view::{build_main_diff_view, MainDiffView};
use super::i18n::Msg;
use super::render_helpers::render_diff_list;
use super::theme::{self, theme};
use super::types::ToastKind;
use super::{CompareTarget, DividerDrag, DividerGhost, DividerKind, KagiApp, MainDiffSource};

/// One open PR tab.
pub struct PrTab {
    pub pr: PullRequest,
    /// merge-base(base, head) — the diff base for the whole-PR view.
    pub base: CommitId,
    pub head: CommitId,
    pub commits: Vec<Commit>,
    /// Files of the current selection (whole PR, or the selected commit).
    pub files: Vec<FileStatus>,
    /// `None` = whole PR; `Some(i)` = `commits[i]` only.
    pub selected_commit: Option<usize>,
    pub selected_file: Option<usize>,
    pub diff: Option<MainDiffView>,
    pub diff_scroll: ListState,
    /// Show the PR description (markdown) in the diff area instead of the
    /// selected file's diff. On by default for a fresh tab; the title toggles
    /// it; selecting a file or commit switches to the diff.
    pub show_description: bool,
}

/// Which list the arrow keys drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrFocus {
    #[default]
    List,
    Commits,
    Files,
    Stack,
}

pub struct PrModeState {
    pub tabs: Vec<PrTab>,
    pub active: Option<usize>,
    /// Keyboard focus target for ↑/↓; ←/→ cycle it. Set by clicking a pane.
    pub focus: PrFocus,
    /// Left / right column widths (unscaled px), dragged via the dividers.
    pub left_w: f32,
    pub right_w: f32,
}

impl Default for PrModeState {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
            focus: PrFocus::List,
            left_w: LEFT_W,
            right_w: RIGHT_W,
        }
    }
}

pub const LEFT_MIN: f32 = 160.0;
pub const LEFT_MAX: f32 = 480.0;
pub const RIGHT_MIN: f32 = 220.0;
pub const RIGHT_MAX: f32 = 640.0;

const COMMIT_LIMIT: usize = 500;

impl KagiApp {
    pub fn toggle_pr_mode(&mut self, cx: &mut Context<Self>) {
        if self.pr_mode.is_some() {
            self.pr_mode = None;
            klog!("pr-mode: closed");
        } else {
            if self.repo_path.is_none() {
                return;
            }
            // Modes are exclusive (the resolver would let a takeover win, but
            // leaving one open would just hide this one).
            self.close_file_history();
            self.close_ecosystem_view();
            self.pr_mode = Some(PrModeState::default());
            klog!("pr-mode: opened");
        }
        cx.notify();
    }

    /// Open (or activate) a tab for `pr` and load its content.
    pub fn pr_mode_open(&mut self, pr: &PullRequest, cx: &mut Context<Self>) {
        if self.pr_mode.is_none() {
            self.toggle_pr_mode(cx);
        }
        if let Some(ix) = self
            .pr_mode
            .as_ref()
            .and_then(|m| m.tabs.iter().position(|t| t.pr.number == pr.number))
        {
            if let Some(m) = self.pr_mode.as_mut() {
                m.active = Some(ix);
            }
            cx.notify();
            return;
        }
        let tip = |name: &str| {
            self.active_view
                .remote_branches
                .iter()
                .find(|rb| rb.name == name)
                .map(|rb| rb.target.clone())
        };
        let (Some(base_tip), Some(head)) = (tip(&pr.base), tip(&pr.head)) else {
            self.push_toast(
                ToastKind::Info,
                SharedString::from(format!(
                    "{}: {} / {}",
                    Msg::PrBranchNotFetched.t(),
                    pr.base,
                    pr.head
                )),
                cx,
            );
            return;
        };
        let Some(session) = self.repo_session.as_ref() else {
            return;
        };
        let repo = session.backend();
        let base = repo.merge_base(&base_tip, &head).unwrap_or(base_tip);
        let commits = repo
            .commits_between(&base, &head, COMMIT_LIMIT)
            .unwrap_or_default();
        let files = repo.compare_commits(&base, &head).unwrap_or_default();
        klog!(
            "pr-mode: open #{} commits={} files={}",
            pr.number,
            commits.len(),
            files.len()
        );
        let mut tab = PrTab {
            pr: pr.clone(),
            base,
            head,
            commits,
            files,
            selected_commit: None,
            selected_file: None,
            diff: None,
            diff_scroll: ListState::new(0, gpui::ListAlignment::Top, px(200.)),
            // A fresh tab opens on the description — "what is this PR" first,
            // the diff once a file/commit is picked (user request).
            show_description: true,
        };
        if !tab.files.is_empty() {
            tab.selected_file = Some(0);
        }
        self.pr_tab_reload_diff(&mut tab);
        let m = self.pr_mode.get_or_insert_with(PrModeState::default);
        m.tabs.push(tab);
        m.active = Some(m.tabs.len() - 1);
        cx.notify();
    }

    pub fn pr_mode_close_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode.as_mut() {
            if ix < m.tabs.len() {
                m.tabs.remove(ix);
                m.active = if m.tabs.is_empty() {
                    None
                } else {
                    Some(ix.min(m.tabs.len() - 1))
                };
            }
        }
        cx.notify();
    }

    pub fn pr_mode_activate(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode.as_mut() {
            if ix < m.tabs.len() {
                m.active = Some(ix);
            }
        }
        cx.notify();
    }

    /// Select a commit (`None` = whole PR): the files list and diff follow.
    pub fn pr_mode_select_commit(&mut self, sel: Option<usize>, cx: &mut Context<Self>) {
        let Some(mut tab) = self.pr_mode_take_active() else {
            return;
        };
        tab.selected_commit = sel;
        tab.show_description = false;
        tab.files = match self.repo_session.as_ref().map(|s| s.backend()) {
            Some(repo) => match sel.and_then(|i| tab.commits.get(i)) {
                Some(c) => repo.commit_changed_files(&c.id).unwrap_or_default(),
                None => repo
                    .compare_commits(&tab.base, &tab.head)
                    .unwrap_or_default(),
            },
            None => Vec::new(),
        };
        tab.selected_file = (!tab.files.is_empty()).then_some(0);
        self.pr_tab_reload_diff(&mut tab);
        self.pr_mode_put_active(tab);
        cx.notify();
    }

    /// Title click: show the PR description in the diff area (toggle).
    pub fn pr_mode_toggle_description(&mut self, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode.as_mut() {
            if let Some(t) = m.active.and_then(|i| m.tabs.get_mut(i)) {
                t.show_description = !t.show_description;
            }
        }
        cx.notify();
    }

    pub fn pr_mode_select_file(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(mut tab) = self.pr_mode_take_active() else {
            return;
        };
        if ix < tab.files.len() {
            tab.selected_file = Some(ix);
            tab.show_description = false;
            self.pr_tab_reload_diff(&mut tab);
        }
        self.pr_mode_put_active(tab);
        cx.notify();
    }

    /// ←/→: cycle the focused pane (List → Commits → Files → Stack).
    pub fn pr_mode_cycle_focus(&mut self, delta: i32, cx: &mut Context<Self>) {
        const ORDER: [PrFocus; 4] = [
            PrFocus::List,
            PrFocus::Commits,
            PrFocus::Files,
            PrFocus::Stack,
        ];
        if let Some(m) = self.pr_mode.as_mut() {
            let i = ORDER.iter().position(|f| *f == m.focus).unwrap_or(0) as i32;
            let n = ORDER.len() as i32;
            m.focus = ORDER[((i + delta) % n + n) as usize % ORDER.len()];
        }
        cx.notify();
    }

    /// ↑/↓ inside the focused pane.
    pub fn pr_mode_step(&mut self, delta: i32, cx: &mut Context<Self>) {
        let Some(m) = self.pr_mode.as_ref() else {
            return;
        };
        match m.focus {
            PrFocus::List => {
                // Step through the left list in its display order; open the
                // neighbour of the active PR (or the first one).
                let order = pr_list_order(self);
                if order.is_empty() {
                    return;
                }
                let cur = m
                    .active
                    .and_then(|i| m.tabs.get(i))
                    .and_then(|t| order.iter().position(|p| p.number == t.pr.number));
                let next = match cur {
                    Some(i) => (i as i32 + delta).clamp(0, order.len() as i32 - 1) as usize,
                    None => 0,
                };
                let pr = order[next].clone();
                self.pr_mode_open(&pr, cx);
            }
            PrFocus::Commits => {
                let Some(t) = m.active.and_then(|i| m.tabs.get(i)) else {
                    return;
                };
                // Row 0 = "All changes", rows 1..=n = commits.
                let n = t.commits.len() as i32;
                let cur = t.selected_commit.map(|i| i as i32 + 1).unwrap_or(0);
                let next = (cur + delta).clamp(0, n);
                let sel = if next == 0 {
                    None
                } else {
                    Some((next - 1) as usize)
                };
                if sel != t.selected_commit {
                    self.pr_mode_select_commit(sel, cx);
                }
            }
            PrFocus::Files => {
                let Some(t) = m.active.and_then(|i| m.tabs.get(i)) else {
                    return;
                };
                if t.files.is_empty() {
                    return;
                }
                let cur = t.selected_file.unwrap_or(0) as i32;
                let next = (cur + delta).clamp(0, t.files.len() as i32 - 1) as usize;
                self.pr_mode_select_file(next, cx);
            }
            PrFocus::Stack => {
                let Some(t) = m.active.and_then(|i| m.tabs.get(i)) else {
                    return;
                };
                let prs: Vec<PullRequest> = stack_for(&t.pr, &self.github_prs)
                    .into_iter()
                    .filter_map(|r| match r {
                        StackRow::Pr(p) => Some(p),
                        StackRow::Trunk(_) => None,
                    })
                    .collect();
                let Some(cur) = prs.iter().position(|p| p.number == t.pr.number) else {
                    return;
                };
                let next = (cur as i32 + delta).clamp(0, prs.len() as i32 - 1) as usize;
                if next != cur {
                    let pr = prs[next].clone();
                    self.pr_mode_open(&pr, cx);
                    // Keep the focus on the stack after the tab switch.
                    if let Some(m) = self.pr_mode.as_mut() {
                        m.focus = PrFocus::Stack;
                    }
                }
            }
        }
    }

    fn pr_mode_focus(&mut self, f: PrFocus, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode.as_mut() {
            m.focus = f;
        }
        cx.notify();
    }

    // Take/put the active tab so `pr_tab_reload_diff` can borrow `self`
    // (the repo session) without fighting the `pr_mode` borrow.
    fn pr_mode_take_active(&mut self) -> Option<PrTab> {
        let m = self.pr_mode.as_mut()?;
        let ix = m.active?;
        (ix < m.tabs.len()).then(|| m.tabs.remove(ix))
    }
    fn pr_mode_put_active(&mut self, tab: PrTab) {
        if let Some(m) = self.pr_mode.as_mut() {
            let ix = m.active.unwrap_or(0).min(m.tabs.len());
            m.tabs.insert(ix, tab);
            m.active = Some(ix);
        }
    }

    fn pr_tab_reload_diff(&self, tab: &mut PrTab) {
        let Some(session) = self.repo_session.as_ref() else {
            return;
        };
        let repo = session.backend();
        let Some(fi) = tab.selected_file else {
            tab.diff = None;
            return;
        };
        let Some(file) = tab.files.get(fi) else {
            tab.diff = None;
            return;
        };
        let path = file.path.clone();
        let (result, source) = match tab.selected_commit.and_then(|i| tab.commits.get(i)) {
            Some(c) => {
                let parent = c.parents.first().cloned().unwrap_or_else(|| c.id.clone());
                (
                    repo.commit_file_diff(&c.id, &path),
                    MainDiffSource::Compare {
                        base: parent,
                        target: CompareTarget::Commit(c.id.clone()),
                        file_index: fi,
                    },
                )
            }
            None => (
                repo.compare_file_diff(&tab.base, &tab.head, &path),
                MainDiffSource::Compare {
                    base: tab.base.clone(),
                    target: CompareTarget::Commit(tab.head.clone()),
                    file_index: fi,
                },
            ),
        };
        tab.diff = match result {
            Ok(fd) => Some(build_main_diff_view(&fd, &path, fi, source)),
            Err(e) => {
                klog!("pr-mode: diff error: {}", e);
                None
            }
        };
    }
}

// ────────────────────────────────────────────────────────────
// Rendering — three columns inside one center-takeover element
// ────────────────────────────────────────────────────────────

const LEFT_W: f32 = 230.0;
const RIGHT_W: f32 = 320.0;
/// Deepest indent level drawn in the left list; beyond it depth is shown by
/// the └ marker only, so a tall stack can never push rows out of the pane.
const MAX_INDENT_DEPTH: usize = 3;
/// Fixed commit-strip height (≈8 rows) so it neither collapses for short PRs
/// nor swallows the diff for long ones; it scrolls internally.
const COMMIT_STRIP_H: f32 = 210.0;
const ROW_H: f32 = 24.0;

pub fn render_pr_mode(app: &mut KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let left = render_pr_list(app, cx);
    let center = render_center(app, cx);
    let right = render_right(app, cx);
    div()
        .flex()
        .flex_row()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .h_full()
        .bg(rgb(theme().bg_base))
        .child(left)
        .child(vdivider(DividerKind::PrModeLeft))
        .child(center)
        .child(vdivider(DividerKind::PrModeRight))
        .child(right)
        .into_any_element()
}

fn vdivider(kind: DividerKind) -> gpui::Stateful<gpui::Div> {
    let id = match kind {
        DividerKind::PrModeLeft => "pr-mode-div-left",
        _ => "pr-mode-div-right",
    };
    div()
        .id(id)
        .w(theme::scaled_px(4.))
        .flex_shrink_0()
        .h_full()
        .bg(rgb(theme().surface))
        .hover(|s| s.bg(rgb(theme().color_branch)).cursor_col_resize())
        .cursor_col_resize()
        .on_drag(DividerDrag { kind }, |_drag, _position, _window, cx| {
            cx.new(|_| DividerGhost)
        })
}

fn section_label(text: String) -> gpui::Div {
    div()
        .px_3()
        .py_1()
        .text_xs()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(theme().text_muted))
        .child(SharedString::from(text))
}

fn ci_glyph(ci: CiState) -> (&'static str, u32) {
    match ci {
        CiState::Success => ("\u{2713}", theme().color_success),
        CiState::Failure => ("\u{2717}", theme().color_blocker),
        CiState::Pending => ("\u{25CF}", theme().color_warning),
        CiState::None => ("\u{25CB}", theme().text_muted),
    }
}

/// The left list's display order (groups, each stack-ordered) — shared by
/// the renderer and ↑/↓ stepping so they can never disagree.
fn pr_list_order(app: &KagiApp) -> Vec<PullRequest> {
    let login = app.github_login.clone();
    let local: Vec<String> = app
        .active_view
        .branches
        .iter()
        .map(|(n, _)| n.clone())
        .collect();
    let mut out = Vec::new();
    for group in [PrGroup::Mine, PrGroup::ReviewRequested, PrGroup::Others] {
        let members: Vec<PullRequest> = app
            .github_prs
            .iter()
            .filter(|p| p.group_for(login.as_deref(), &local) == group)
            .cloned()
            .collect();
        for (ix, _) in stack_order(&members) {
            out.push(members[ix].clone());
        }
    }
    out
}

/// A pane's focus cue: a 2px accent top border when it owns the arrow keys.
fn focus_border<E: gpui::Styled>(el: E, focused: bool) -> E {
    el.border_t_2().border_color(rgb(if focused {
        theme().color_branch
    } else {
        theme().panel
    }))
}

// ── Left: PR list, grouped, stack-ordered ────────────────────
fn render_pr_list(app: &KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let focused = app.pr_mode.as_ref().map(|m| m.focus) == Some(PrFocus::List);
    let left_w = app.pr_mode.as_ref().map(|m| m.left_w).unwrap_or(LEFT_W);
    let focus_click = cx.listener(|this: &mut KagiApp, _: &gpui::MouseDownEvent, _w, cx| {
        this.pr_mode_focus(PrFocus::List, cx);
    });
    let login = app.github_login.clone();
    let local: Vec<String> = app
        .active_view
        .branches
        .iter()
        .map(|(n, _)| n.clone())
        .collect();
    let all = app.github_prs.clone();
    let active_pr = app
        .pr_mode
        .as_ref()
        .and_then(|m| m.active.and_then(|i| m.tabs.get(i)))
        .map(|t| t.pr.number);
    let mut col = focus_border(
        div()
            .id("pr-mode-list")
            .w(theme::scaled_px(left_w))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .bg(rgb(theme().sidebar))
            .on_mouse_down(gpui::MouseButton::Left, focus_click),
        focused,
    );
    // Header: title + exit
    let exit = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.toggle_pr_mode(cx);
    });
    col = col.child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .px_3()
            .py_2()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(Msg::PrPaneTitle.t())),
            )
            .child(
                div()
                    .id("pr-mode-exit")
                    .px_2()
                    .rounded_sm()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .cursor_pointer()
                    .hover(|s| {
                        s.bg(rgb(theme().surface))
                            .text_color(rgb(theme().text_main))
                    })
                    .on_click(exit)
                    .child(SharedString::from(Msg::PrModeExit.t())),
            ),
    );
    if all.is_empty() {
        col = col.child(
            div()
                .px_3()
                .py_2()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::PrPaneEmpty.t())),
        );
    }
    for (group, title) in [
        (PrGroup::Mine, Msg::PrGroupMine.t()),
        (PrGroup::ReviewRequested, Msg::PrGroupReview.t()),
        (PrGroup::Others, Msg::PrGroupOthers.t()),
    ] {
        let members: Vec<PullRequest> = all
            .iter()
            .filter(|p| p.group_for(login.as_deref(), &local) == group)
            .cloned()
            .collect();
        if members.is_empty() {
            continue;
        }
        col = col.child(section_label(format!("{} ({})", title, members.len())));
        for (ix, depth) in stack_order(&members) {
            let pr = &members[ix];
            let (g, c) = ci_glyph(pr.ci);
            let is_active = active_pr == Some(pr.number);
            let pr_click = pr.clone();
            let click = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                this.pr_mode_open(&pr_click, cx);
            });
            let pr_menu = pr.clone();
            let menu = cx.listener(
                move |this: &mut KagiApp, e: &gpui::MouseDownEvent, _w, cx| {
                    this.pr_menu = Some((pr_menu.clone(), e.position));
                    cx.stop_propagation();
                    cx.notify();
                },
            );
            col = col.child(
                div()
                    .id(("pr-mode-list-row", pr.number as usize))
                    .h(theme::scaled_px(ROW_H))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .pl(theme::scaled_px(
                        12. + depth.min(MAX_INDENT_DEPTH) as f32 * 12.,
                    ))
                    .pr_3()
                    .text_sm()
                    .cursor_pointer()
                    .when(is_active, |el| el.bg(rgb(theme().selected)))
                    .hover(|s| s.bg(rgb(theme().surface)))
                    .on_click(click)
                    .on_mouse_down(gpui::MouseButton::Right, menu)
                    .when(pr.is_draft, |el| el.opacity(0.55))
                    .when(depth > 0, |el| {
                        el.child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(rgb(theme().text_muted))
                                .child(SharedString::from(if depth > MAX_INDENT_DEPTH {
                                    format!("\u{2514}{}", depth)
                                } else {
                                    "\u{2514}".to_string()
                                })),
                        )
                    })
                    .child(
                        div()
                            .flex_shrink_0()
                            .w(theme::scaled_px(12.))
                            .text_color(rgb(c))
                            .child(SharedString::from(g)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(rgb(theme().text_main))
                            .child(SharedString::from(format!("#{} {}", pr.number, pr.title))),
                    ),
            );
        }
    }
    col.into_any_element()
}

// ── Center: tabs + header + commits + diff ───────────────────
fn render_center(app: &mut KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let (tabs, active): (Vec<(u64, bool)>, Option<usize>) = match app.pr_mode.as_ref() {
        Some(m) => (
            m.tabs
                .iter()
                .enumerate()
                .map(|(i, t)| (t.pr.number, m.active == Some(i)))
                .collect(),
            m.active,
        ),
        None => (Vec::new(), None),
    };
    // Tab bar
    let mut bar = div()
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .h(theme::scaled_px(30.))
        .px_2()
        .gap_1()
        .bg(rgb(theme().panel))
        .border_b_1()
        .border_color(rgb(theme().surface));
    for (i, (n, is_active)) in tabs.iter().enumerate() {
        let activate = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
            this.pr_mode_activate(i, cx);
        });
        let close = cx.listener(
            move |this: &mut KagiApp, e: &gpui::MouseDownEvent, _w, cx| {
                let _ = e;
                cx.stop_propagation();
                this.pr_mode_close_tab(i, cx);
            },
        );
        bar = bar.child(
            div()
                .id(("pr-tab", *n as usize))
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .rounded_sm()
                .text_sm()
                .cursor_pointer()
                .when(*is_active, |el| el.bg(rgb(theme().selected)))
                .hover(|s| s.bg(rgb(theme().surface)))
                .on_click(activate)
                .child(
                    div()
                        .text_color(rgb(theme().text_main))
                        .child(SharedString::from(format!("#{}", n))),
                )
                .child(
                    div()
                        .id(("pr-tab-close", *n as usize))
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .hover(|s| s.text_color(rgb(theme().text_main)))
                        .on_mouse_down(gpui::MouseButton::Left, close)
                        .child(SharedString::from("\u{2715}")),
                ),
        );
    }

    let mut col = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .h_full()
        .child(bar);

    let Some(ix) = active else {
        // No tab: use the whole center as a PR dashboard instead of an empty
        // hint (user request) — every open PR as a table row, click to open.
        return col.child(render_dashboard(app, cx)).into_any_element();
    };
    // Snapshot what the renderers need from the active tab.
    let (pr, commits, selected_commit, diff, scroll, show_description) = {
        let m = app.pr_mode.as_ref().unwrap();
        let t = &m.tabs[ix];
        (
            t.pr.clone(),
            t.commits.clone(),
            t.selected_commit,
            t.diff.clone(),
            t.diff_scroll.clone(),
            t.show_description,
        )
    };

    // PR header
    let (g, c) = ci_glyph(pr.ci);
    let (rv, rvc) = match pr.review {
        ReviewState::Approved => (Msg::PrReviewApproved.t(), theme().color_success),
        ReviewState::ChangesRequested => (Msg::PrReviewChanges.t(), theme().color_warning),
        ReviewState::ReviewRequired => (Msg::PrReviewRequired.t(), theme().text_sub),
        ReviewState::None => ("", theme().text_muted),
    };
    let pr_open = pr.clone();
    let open_gh = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.open_pr_in_browser(&pr_open);
        cx.notify();
    });
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .flex_shrink_0()
        .px_3()
        .py_2()
        .child(
            div()
                .text_color(rgb(theme().color_branch))
                .text_sm()
                .child(SharedString::from(format!("#{}", pr.number))),
        )
        .child({
            // Click the title → the PR description (markdown) takes the diff
            // area; click again (or pick a file / commit) to go back.
            let toggle = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                this.pr_mode_toggle_description(cx);
            });
            div()
                .id("pr-mode-title")
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_sm()
                .cursor_pointer()
                .text_color(rgb(if show_description {
                    theme().color_branch
                } else {
                    theme().text_main
                }))
                .hover(|s| s.text_color(rgb(theme().color_branch)))
                .tooltip(|w, cx| {
                    gpui_component::tooltip::Tooltip::new(Msg::PrModeShowDescription.t())
                        .build(w, cx)
                })
                .on_click(toggle)
                .child(SharedString::from(pr.title.clone()))
        })
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .child(SharedString::from(format!(
                    "{} \u{2192} {}",
                    pr.head, pr.base
                ))),
        )
        .child(div().text_color(rgb(c)).child(SharedString::from(g)))
        .when(!rv.is_empty(), |el| {
            el.child(
                div()
                    .text_xs()
                    .text_color(rgb(rvc))
                    .child(SharedString::from(rv.to_string())),
            )
        })
        .child(
            div()
                .id("pr-mode-open-gh")
                .px_2()
                .py_px()
                .rounded_sm()
                .border_1()
                .border_color(rgb(theme().selected))
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(theme().surface)))
                .on_click(open_gh)
                .child(SharedString::from("GitHub \u{2197}")),
        );

    // Commit strip
    let all_click = cx.listener(|this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.pr_mode_select_commit(None, cx);
    });
    let now = kagi_ui_core::time::now_unix_secs();
    let commits_focused = app.pr_mode.as_ref().map(|m| m.focus) == Some(PrFocus::Commits);
    let commits_focus_click =
        cx.listener(|this: &mut KagiApp, _: &gpui::MouseDownEvent, _w, cx| {
            this.pr_mode_focus(PrFocus::Commits, cx);
        });
    // The strip is its own panel (panel bg, section header) so where it ends
    // and the description / diff begins is unmistakable (user request).
    let mut strip = focus_border(
        div()
            .id("pr-mode-commits")
            .flex_shrink_0()
            .h(theme::scaled_px(COMMIT_STRIP_H))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .bg(rgb(theme().panel))
            .border_b_1()
            .border_color(rgb(theme().selected))
            .on_mouse_down(gpui::MouseButton::Left, commits_focus_click),
        commits_focused,
    )
    .child(section_label(format!(
        "{} ({})",
        Msg::PrModeCommits.t(),
        commits.len()
    )))
    .child(
        div()
            .id("pr-mode-commits-all")
            .h(theme::scaled_px(ROW_H))
            .flex()
            .flex_row()
            .items_center()
            .px_3()
            .text_xs()
            .cursor_pointer()
            .when(selected_commit.is_none(), |el| el.bg(rgb(theme().selected)))
            .hover(|s| s.bg(rgb(theme().surface)))
            .on_click(all_click)
            .child(
                div()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(format!(
                        "{} ({})",
                        Msg::PrModeAllChanges.t(),
                        commits.len()
                    ))),
            ),
    );
    for (i, cmt) in commits.iter().enumerate() {
        let click = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
            this.pr_mode_select_commit(Some(i), cx);
        });
        let sel = selected_commit == Some(i);
        strip = strip.child(
            div()
                .id(("pr-mode-commit", i))
                .h(theme::scaled_px(ROW_H))
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_3()
                .text_sm()
                .cursor_pointer()
                .when(sel, |el| el.bg(rgb(theme().selected)))
                .hover(|s| s.bg(rgb(theme().surface)))
                .on_click(click)
                // Straight-line "graph": a dot per commit, no lanes — the
                // user asked for focus on this change, not the whole repo.
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(rgb(theme().color_branch))
                        .child(SharedString::from("\u{25CF}")),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .font_family(super::MONO_FONT)
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(cmt.id.short())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_color(rgb(theme().text_main))
                        .child(SharedString::from(cmt.summary.clone())),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(rgb(theme().text_sub))
                        .child(SharedString::from(cmt.author.name.clone())),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(kagi_ui_core::time::relative_time(
                            cmt.author.time,
                            now,
                        ))),
                ),
        );
    }

    col = col.child(header).child(strip);
    if show_description {
        return col.child(render_description(&pr, cx)).into_any_element();
    }
    // Diff
    let diff_el: gpui::AnyElement = match diff {
        Some(view) => render_diff_list::<KagiApp>(view, None, None, scroll, cx).into_any_element(),
        None => div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from(Msg::PrModeNoFile.t()))
            .into_any_element(),
    };
    col.child(diff_el).into_any_element()
}

// ── Right: stack (top) + files (bottom) ──────────────────────
fn render_right(app: &KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let active = app
        .pr_mode
        .as_ref()
        .and_then(|m| m.active.and_then(|i| m.tabs.get(i)));
    let right_w = app.pr_mode.as_ref().map(|m| m.right_w).unwrap_or(RIGHT_W);
    let focus = app.pr_mode.as_ref().map(|m| m.focus);
    let mut col = div()
        .w(theme::scaled_px(right_w))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().panel));

    // Stack: walk down through bases and up through children of the active PR.
    let stack_focus_click = cx.listener(|this: &mut KagiApp, _: &gpui::MouseDownEvent, _w, cx| {
        this.pr_mode_focus(PrFocus::Stack, cx);
    });
    let mut stack_col = focus_border(
        div()
            .id("pr-mode-stack-pane")
            .flex_shrink_0()
            .max_h(relative(0.5))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .on_mouse_down(gpui::MouseButton::Left, stack_focus_click),
        focus == Some(PrFocus::Stack),
    );
    let stack: Vec<StackRow> = active
        .map(|t| stack_for(&t.pr, &app.github_prs))
        .unwrap_or_default();
    stack_col = stack_col.child(section_label(Msg::PrModeStack.t().to_string()));
    if stack.is_empty() {
        stack_col = stack_col.child(
            div()
                .px_3()
                .py_1()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::PrModeSelectHint.t())),
        );
    }
    let active_n = active.map(|t| t.pr.number);
    for row in &stack {
        match row {
            StackRow::Pr(pr) => {
                let (g, c) = ci_glyph(pr.ci);
                let is_active = active_n == Some(pr.number);
                let pr_click = pr.clone();
                let click = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                    this.pr_mode_open(&pr_click, cx);
                });
                let rv = match pr.review {
                    ReviewState::Approved => {
                        Some((Msg::PrReviewApproved.t(), theme().color_success))
                    }
                    ReviewState::ChangesRequested => {
                        Some((Msg::PrReviewChanges.t(), theme().color_warning))
                    }
                    _ => None,
                };
                // Two-line row: `#N title` on top, `head → base · author` below —
                // the stack pane is where a PR's identity should be readable,
                // so it carries more than the compact left list.
                stack_col = stack_col.child(
                    div()
                        .id(("pr-mode-stack", pr.number as usize))
                        .flex()
                        .flex_row()
                        .items_start()
                        .gap_2()
                        .px_3()
                        .py_1()
                        .cursor_pointer()
                        .when(is_active, |el| el.bg(rgb(theme().selected)))
                        .hover(|s| s.bg(rgb(theme().surface)))
                        .on_click(click)
                        .child(
                            div()
                                .flex_shrink_0()
                                .pt_px()
                                .text_sm()
                                .text_color(rgb(if is_active {
                                    theme().color_branch
                                } else {
                                    theme().text_sub
                                }))
                                .child(SharedString::from("\u{25CF}")),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .text_sm()
                                                .text_color(rgb(theme().color_branch))
                                                .child(SharedString::from(format!(
                                                    "#{}",
                                                    pr.number
                                                ))),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .truncate()
                                                .text_sm()
                                                .text_color(rgb(theme().text_main))
                                                .child(SharedString::from(pr.title.clone())),
                                        )
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .text_color(rgb(c))
                                                .child(SharedString::from(g)),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_2()
                                        .text_xs()
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .truncate()
                                                .text_color(rgb(theme().text_muted))
                                                .child(SharedString::from(format!(
                                                    "{} \u{2192} {} \u{00B7} @{}",
                                                    pr.head, pr.base, pr.author
                                                ))),
                                        )
                                        .children(rv.map(|(t, c)| {
                                            div()
                                                .flex_shrink_0()
                                                .text_color(rgb(c))
                                                .child(SharedString::from(t.to_string()))
                                        })),
                                ),
                        ),
                );
            }
            StackRow::Trunk(name) => {
                stack_col = stack_col.child(
                    div()
                        .h(theme::scaled_px(ROW_H))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .text_sm()
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_color(rgb(theme().text_muted))
                                .child(SharedString::from("\u{25CB}")),
                        )
                        .child(
                            div()
                                .text_color(rgb(theme().text_sub))
                                .child(SharedString::from(name.clone())),
                        ),
                );
            }
        }
    }

    col = col.child(stack_col);

    // Files
    let (files, selected_file) = active
        .map(|t| (t.files.clone(), t.selected_file))
        .unwrap_or_default();
    col = col.child(
        section_label(format!("{} ({})", Msg::PrModeFiles.t(), files.len()))
            .border_t_1()
            .border_color(rgb(theme().surface)),
    );
    let files_focus_click = cx.listener(|this: &mut KagiApp, _: &gpui::MouseDownEvent, _w, cx| {
        this.pr_mode_focus(PrFocus::Files, cx);
    });
    let mut list = focus_border(
        div()
            .id("pr-mode-files")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .on_mouse_down(gpui::MouseButton::Left, files_focus_click),
        focus == Some(PrFocus::Files),
    );
    for (i, f) in files.iter().enumerate() {
        let (badge, color, _) = status_badge(Some(&f.change), false);
        let click = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
            this.pr_mode_select_file(i, cx);
        });
        let sel = selected_file == Some(i);
        let name = f
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dir = f
            .path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        list = list.child(
            div()
                .id(("pr-mode-file", i))
                .h(theme::scaled_px(ROW_H))
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .px_3()
                .text_sm()
                .cursor_pointer()
                .when(sel, |el| el.bg(rgb(theme().selected)))
                .hover(|s| s.bg(rgb(theme().surface)))
                .on_click(click)
                .child(
                    div()
                        .w(theme::scaled_px(14.))
                        .flex_shrink_0()
                        .text_color(rgb(color))
                        .child(SharedString::from(badge)),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(rgb(theme().text_main))
                        .child(SharedString::from(name)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_xs()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(dir)),
                ),
        );
    }
    col.child(list).into_any_element()
}

enum StackRow {
    Pr(PullRequest),
    Trunk(String),
}

/// The chain `pr` sits in, tip first, trunk last — inferred from open PRs'
/// base links (gh stack, when the branch is in one, will replace this).
fn stack_for(pr: &PullRequest, all: &[PullRequest]) -> Vec<StackRow> {
    let mut up: Vec<PullRequest> = Vec::new();
    // Ancestors: follow base → the PR whose head is that base.
    let mut down: Vec<PullRequest> = Vec::new();
    let mut cur = pr.clone();
    let mut guard = 0;
    while let Some(parent) = all
        .iter()
        .find(|p| p.head == cur.base && p.number != cur.number)
    {
        down.push(parent.clone());
        cur = parent.clone();
        guard += 1;
        if guard > 50 {
            break;
        }
    }
    let trunk = cur.base.clone();
    // Descendants: PRs whose base is our head (first child chain only —
    // branching stacks are shown from the child's own tab).
    let mut cur = pr.clone();
    guard = 0;
    while let Some(child) = all
        .iter()
        .find(|p| p.base == cur.head && p.number != cur.number)
    {
        up.push(child.clone());
        cur = child.clone();
        guard += 1;
        if guard > 50 {
            break;
        }
    }
    let mut out: Vec<StackRow> = up.into_iter().rev().map(StackRow::Pr).collect();
    out.push(StackRow::Pr(pr.clone()));
    out.extend(down.into_iter().map(StackRow::Pr));
    out.push(StackRow::Trunk(trunk));
    out
}

// ── Center dashboard (no tab open): all PRs as a table ─────────
fn render_dashboard(app: &KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let login = app.github_login.clone();
    let local: Vec<String> = app
        .active_view
        .branches
        .iter()
        .map(|(n, _)| n.clone())
        .collect();
    let all = app.github_prs.clone();
    let col_hdr = |w: f32, text: &str| {
        div()
            .w(theme::scaled_px(w))
            .flex_shrink_0()
            .overflow_hidden()
            .text_xs()
            .text_color(rgb(theme().text_label))
            .child(SharedString::from(text.to_string()))
    };
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .flex_shrink_0()
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(col_hdr(64., "#"))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_xs()
                .text_color(rgb(theme().text_label))
                .child(SharedString::from(Msg::PrColTitle.t())),
        )
        .child(col_hdr(110., Msg::PrColAuthor.t()))
        .child(col_hdr(240., Msg::PrColBranches.t()))
        .child(col_hdr(36., "CI"))
        .child(col_hdr(80., Msg::PrColReview.t()));

    let mut list = div()
        .id("pr-mode-dashboard")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    if all.is_empty() {
        list = list.child(
            div()
                .px_3()
                .py_3()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::PrPaneEmpty.t())),
        );
    }
    for (group, title) in [
        (PrGroup::Mine, Msg::PrGroupMine.t()),
        (PrGroup::ReviewRequested, Msg::PrGroupReview.t()),
        (PrGroup::Others, Msg::PrGroupOthers.t()),
    ] {
        let members: Vec<PullRequest> = all
            .iter()
            .filter(|p| p.group_for(login.as_deref(), &local) == group)
            .cloned()
            .collect();
        if members.is_empty() {
            continue;
        }
        list = list.child(section_label(format!("{} ({})", title, members.len())));
        for (ix, depth) in stack_order(&members) {
            list = list.child(render_dashboard_row(&members[ix], depth, cx));
        }
    }
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(header)
        .child(list)
        .into_any_element()
}

fn render_dashboard_row(
    pr: &PullRequest,
    depth: usize,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let (g, c) = ci_glyph(pr.ci);
    let (rv, rvc) = match pr.review {
        ReviewState::Approved => (Msg::PrReviewApproved.t(), theme().color_success),
        ReviewState::ChangesRequested => (Msg::PrReviewChanges.t(), theme().color_warning),
        ReviewState::ReviewRequired => (Msg::PrReviewRequired.t(), theme().text_sub),
        ReviewState::None => ("", theme().text_muted),
    };
    let pr_open = pr.clone();
    let click = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
        this.pr_mode_open(&pr_open, cx);
    });
    let pr_menu = pr.clone();
    let menu = cx.listener(
        move |this: &mut KagiApp, e: &gpui::MouseDownEvent, _w, cx| {
            this.pr_menu = Some((pr_menu.clone(), e.position));
            cx.stop_propagation();
            cx.notify();
        },
    );
    let title = if pr.is_draft {
        format!("{} ({})", pr.title, Msg::PrDraft.t())
    } else {
        pr.title.clone()
    };
    let cell = |w: f32| {
        div()
            .w(theme::scaled_px(w))
            .flex_shrink_0()
            .overflow_hidden()
            .truncate()
    };
    div()
        .id(("pr-mode-dash-row", pr.number as usize))
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(theme::scaled_px(26.))
        .px_3()
        .text_sm()
        .cursor_pointer()
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(click)
        .on_mouse_down(gpui::MouseButton::Right, menu)
        .when(pr.is_draft, |el| el.opacity(0.6))
        .child(
            cell(64.)
                .text_color(rgb(theme().color_branch))
                .child(SharedString::from(format!("#{}", pr.number))),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_row()
                .items_center()
                .pl(theme::scaled_px(depth.min(MAX_INDENT_DEPTH) as f32 * 18.))
                .when(depth > 0, |el| {
                    el.child(
                        div()
                            .flex_shrink_0()
                            .mr_1()
                            .text_xs()
                            .text_color(rgb(theme().text_muted))
                            .child(SharedString::from(if depth > MAX_INDENT_DEPTH {
                                format!("\u{2514}{}", depth)
                            } else {
                                "\u{2514}".to_string()
                            })),
                    )
                })
                .child(
                    div()
                        .truncate()
                        .text_color(rgb(theme().text_main))
                        .child(SharedString::from(title)),
                ),
        )
        .child(
            cell(110.)
                .text_color(rgb(theme().text_sub))
                .child(SharedString::from(pr.author.clone())),
        )
        .child(
            cell(240.)
                .text_color(rgb(theme().text_sub))
                .child(SharedString::from(format!(
                    "{} \u{2192} {}",
                    pr.head, pr.base
                ))),
        )
        .child(cell(36.).text_color(rgb(c)).child(SharedString::from(g)))
        .child(
            cell(80.)
                .text_color(rgb(rvc))
                .child(SharedString::from(rv.to_string())),
        )
        .into_any_element()
}

/// The PR description as rendered markdown, in the diff area.
fn render_description(pr: &PullRequest, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    use gpui_component::text::{TextView, TextViewStyle};
    use gpui_component::ActiveTheme as _;
    let body = if pr.body.trim().is_empty() {
        format!("_{}_", Msg::PrModeNoDescription.t())
    } else {
        // GitHub bodies are CRLF, wrap inline code across lines and carry bot
        // HTML footers — all of which crash gpui-component's inline layouter
        // ("text argument should not contain newlines"). Normalise first.
        // Thin-space padding inside `code` spans (the renderer paints the bare
        // glyph range) — same trick as the Editor's markdown preview.
        kagi_ui_editor::markdown::pad_inline_code(
            &kagi_domain::message::sanitize_markdown_for_view(&pr.body),
        )
    };
    // Table borders: gpui-component draws them in `theme().border`, which
    // kagi maps to the near-background `selected` — invisible on the card.
    // The style refinements override the container + cell borders.
    let mut table = gpui::StyleRefinement::default();
    let mut table_cell = gpui::StyleRefinement::default();
    let mut grid: gpui::Hsla = rgb(theme().text_muted).into();
    grid.a = 0.95;
    table.border_color = Some(grid);
    table_cell.border_color = Some(grid);
    let style = TextViewStyle {
        heading_base_font_size: theme::scaled_px(17.),
        highlight_theme: cx.theme().highlight_theme.clone(),
        is_dark: cx.theme().mode.is_dark(),
        table,
        table_cell,
        ..Default::default()
    };
    let (g, c) = ci_glyph(pr.ci);
    // A reading card: rounded surface floating on the base background, a
    // measure-limited column and its own small header — visually a document,
    // not another list, so the boundary with the commit strip is obvious.
    let card_header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .pb_2()
        .mb_3()
        .border_b_1()
        .border_color(rgb(theme().selected))
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(Msg::PrModeDescription.t())),
        )
        .child(div().flex_1())
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_sub))
                .child(SharedString::from(format!("@{}", pr.author))),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(c))
                .child(SharedString::from(g)),
        )
        .when(pr.is_draft, |el| {
            el.child(
                div()
                    .px_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme().selected))
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::PrDraft.t())),
            )
        });
    div()
        .id("pr-mode-description")
        .flex_1()
        .min_h(px(0.))
        .w_full()
        .overflow_y_scroll()
        .bg(rgb(theme().bg_base))
        .p_4()
        .child(
            div()
                // Fill the pane (edge-aligned with the commit strip above); a
                // measure cap left the card visibly narrower than the strip.
                .w_full()
                .rounded_lg()
                .bg(rgb(theme().surface))
                .border_1()
                .border_color(rgb(theme().selected))
                .px_6()
                .py_5()
                .text_color(rgb(theme().text_main))
                .child(card_header)
                .child(
                    TextView::markdown(
                        ("pr-mode-description-md", pr.number as usize),
                        SharedString::from(body),
                    )
                    .style(style),
                ),
        )
        .into_any_element()
}
