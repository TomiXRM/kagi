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

use gpui::{div, prelude::*, px, rgb, Context, ListState, SharedString};
use kagi_domain::github::{stack_order, CiState, PrGroup, PullRequest, ReviewState};
use kagi_git::{Commit, CommitId, FileStatus};
use kagi_ui_core::file_tree::status_badge;

use super::diff_view::{build_main_diff_view, MainDiffView};
use super::i18n::Msg;
use super::render_helpers::render_diff_list;
use super::theme::{self, theme};
use super::types::ToastKind;
use super::{CompareTarget, KagiApp, MainDiffSource};

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
}

#[derive(Default)]
pub struct PrModeState {
    pub tabs: Vec<PrTab>,
    pub active: Option<usize>,
}

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

    pub fn pr_mode_select_file(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(mut tab) = self.pr_mode_take_active() else {
            return;
        };
        if ix < tab.files.len() {
            tab.selected_file = Some(ix);
            self.pr_tab_reload_diff(&mut tab);
        }
        self.pr_mode_put_active(tab);
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

const LEFT_W: f32 = 240.0;
const RIGHT_W: f32 = 260.0;
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
        .child(vdivider())
        .child(center)
        .child(vdivider())
        .child(right)
        .into_any_element()
}

fn vdivider() -> gpui::Div {
    div()
        .w(theme::scaled_px(1.))
        .flex_shrink_0()
        .h_full()
        .bg(rgb(theme().surface))
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

// ── Left: PR list, grouped, stack-ordered ────────────────────
fn render_pr_list(app: &KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
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
    let mut col = div()
        .id("pr-mode-list")
        .w(theme::scaled_px(LEFT_W))
        .flex_shrink_0()
        .h_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .bg(rgb(theme().sidebar));
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
                    .pl(theme::scaled_px(12. + depth as f32 * 14.))
                    .pr_3()
                    .text_sm()
                    .cursor_pointer()
                    .when(is_active, |el| el.bg(rgb(theme().selected)))
                    .hover(|s| s.bg(rgb(theme().surface)))
                    .on_click(click)
                    .on_mouse_down(gpui::MouseButton::Right, menu)
                    .when(pr.is_draft, |el| el.opacity(0.55))
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
        return col
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::PrModeSelectHint.t())),
            )
            .into_any_element();
    };
    // Snapshot what the renderers need from the active tab.
    let (pr, commits, selected_commit, diff, scroll) = {
        let m = app.pr_mode.as_ref().unwrap();
        let t = &m.tabs[ix];
        (
            t.pr.clone(),
            t.commits.clone(),
            t.selected_commit,
            t.diff.clone(),
            t.diff_scroll.clone(),
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
    let mut strip = div()
        .id("pr-mode-commits")
        .flex_shrink_0()
        .max_h(theme::scaled_px(180.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .border_b_1()
        .border_color(rgb(theme().surface))
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
    let mut col = div()
        .w(theme::scaled_px(RIGHT_W))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().panel));

    // Stack: walk down through bases and up through children of the active PR.
    let stack: Vec<StackRow> = active
        .map(|t| stack_for(&t.pr, &app.github_prs))
        .unwrap_or_default();
    col = col.child(section_label(Msg::PrModeStack.t().to_string()));
    if stack.is_empty() {
        col = col.child(
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
                col = col.child(
                    div()
                        .id(("pr-mode-stack", pr.number as usize))
                        .h(theme::scaled_px(ROW_H))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .text_sm()
                        .cursor_pointer()
                        .when(is_active, |el| el.bg(rgb(theme().selected)))
                        .hover(|s| s.bg(rgb(theme().surface)))
                        .on_click(click)
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_color(rgb(theme().color_branch))
                                .child(SharedString::from("\u{25CF}")),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(format!("#{} {}", pr.number, pr.head))),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_color(rgb(c))
                                .child(SharedString::from(g)),
                        )
                        .when(is_active, |el| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(rgb(theme().text_muted))
                                    .child(SharedString::from("\u{2190}")),
                            )
                        }),
                );
            }
            StackRow::Trunk(name) => {
                col = col.child(
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

    // Files
    let (files, selected_file) = active
        .map(|t| (t.files.clone(), t.selected_file))
        .unwrap_or_default();
    col = col.child(
        section_label(format!("{} ({})", Msg::PrModeFiles.t(), files.len()))
            .border_t_1()
            .border_color(rgb(theme().surface)),
    );
    let mut list = div()
        .id("pr-mode-files")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
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
