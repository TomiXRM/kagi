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
use kagi_domain::github::Mergeable;
use kagi_domain::github::{
    stack_order, CiState, Comment, PrAttention, PrGroup, PrReason, PullRequest, Review,
    ReviewComment, ReviewState,
};
use kagi_git::{Commit, CommitId, FileStatus, PrConflictFile};
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
    /// The base **branch's** current tip, which is what a merge would actually
    /// be against. Distinct from `base`: merging `head` into merge-base can
    /// never conflict, because merge-base is an ancestor of `head` (ADR-0145).
    pub base_tip: CommitId,
    pub head: CommitId,
    pub commits: Vec<Commit>,
    /// Files of the current selection (whole PR, or the selected commit).
    pub files: Vec<FileStatus>,
    /// `None` = whole PR; `Some(i)` = `commits[i]` only.
    pub selected_commit: Option<usize>,
    pub selected_file: Option<usize>,
    pub diff: Option<MainDiffView>,
    pub diff_scroll: ListState,
    /// Reviews + issue comments + line comments ("review chat"), fetched once
    /// per tab open.
    pub reviews: Vec<Review>,
    pub comments: Vec<Comment>,
    pub line_comments: Vec<ReviewComment>,
    /// ADR-0145: conflicts a merge would produce, computed locally on first
    /// open of the Conflicts tab. `None` = not computed yet; `Some(Ok(vec![]))`
    /// = computed and clean, which is a different thing to say than "unknown".
    pub conflicts: Option<Result<Vec<PrConflictFile>, String>>,
}

/// Which body the PR tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrView {
    Overview,
    Review,
    Diff,
    /// ADR-0145: read-only preview of what merging this PR would conflict on.
    Conflicts,
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
    /// Which body the center shows. Mode-wide, NOT per tab: switching PRs
    /// while reading reviews should keep showing reviews (user request).
    pub view: PrView,
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
            view: PrView::Overview,
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
            // PR mode is a full-height workspace (list | tabs | rail); the
            // bottom terminal panel would eat a third of it. Collapse it on
            // entry — Cmd-J still brings it back (user request).
            self.bottom_panel_open = false;
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
        let base = repo
            .merge_base(&base_tip, &head)
            .unwrap_or_else(|_| base_tip.clone());
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
            base_tip,
            head,
            commits,
            files,
            selected_commit: None,
            selected_file: None,
            diff: None,
            diff_scroll: ListState::new(0, gpui::ListAlignment::Top, px(200.)),
            // A fresh tab opens on the description — "what is this PR" first,
            // the diff once a file/commit is picked (user request).
            reviews: Vec::new(),
            comments: Vec::new(),
            line_comments: Vec::new(),
            conflicts: None,
        };
        if !tab.files.is_empty() {
            tab.selected_file = Some(0);
        }
        self.pr_tab_reload_diff(&mut tab);
        let m = self.pr_mode.get_or_insert_with(PrModeState::default);
        m.tabs.push(tab);
        m.active = Some(m.tabs.len() - 1);
        cx.notify();
        self.pr_mode_load_conversation(pr.number, cx);
    }

    /// Fetch reviews + comments for `number` in the background and drop them
    /// on the matching tab. One `gh pr view` per tab open (never per list
    /// refresh — the list ticker must stay one call).
    fn pr_mode_load_conversation(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some(repo) = self.repo_path.clone() else {
            return;
        };
        cx.spawn(async move |this, acx| {
            let repo2 = repo.clone();
            let result = acx
                .background_executor()
                .spawn(async move {
                    // Two calls: `gh pr view` for the verdicts + issue
                    // comments, `gh api` for the line comments (where Copilot
                    // / Codex put code suggestions — not exposed by --json).
                    let convo = kagi_git::github::pr_conversation(&repo2, number);
                    let lines = kagi_git::github::pr_review_comments(&repo2, number);
                    (convo, lines)
                })
                .await;
            let (Ok((reviews, comments)), lines) = result else {
                return;
            };
            let line_comments = lines.unwrap_or_default();
            let _ = this.update(acx, |app, cx| {
                if let Some(m) = app.pr_mode.as_mut() {
                    if let Some(t) = m.tabs.iter_mut().find(|t| t.pr.number == number) {
                        klog!(
                            "pr-mode: conversation #{} reviews={} comments={} line={}",
                            number,
                            reviews.len(),
                            comments.len(),
                            line_comments.len()
                        );
                        t.reviews = reviews;
                        t.comments = comments;
                        t.line_comments = line_comments;
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Switch the body between Overview (description), Review, Diff and
    /// Conflicts.
    pub fn pr_mode_show(&mut self, view: PrView, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode.as_mut() {
            m.view = view;
        }
        if view == PrView::Conflicts {
            self.pr_mode_load_conflicts(cx);
        }
        cx.notify();
    }

    /// Compute the active tab's conflict preview, once (ADR-0145).
    ///
    /// Off the UI thread: it is a full three-way merge of two trees, the same
    /// work `plan_merge_branch` does, and on a large repo that is long enough
    /// to drop frames. Cached on the tab because the answer only changes when
    /// the PR or the base does, and re-running it on every render of a tab the
    /// user is *looking at* would be the worst possible cadence.
    fn pr_mode_load_conflicts(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.pr_mode.as_ref() else {
            return;
        };
        let Some(ix) = m.active else { return };
        let Some(tab) = m.tabs.get(ix) else { return };
        if tab.conflicts.is_some() {
            return;
        }
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        // The base **tip**, not `tab.base`: that is merge-base(base, head), and
        // merging head into its own ancestor is a fast-forward, so the preview
        // would report "no conflicts" for every PR ever opened.
        let (base, head, number) = (tab.base_tip.clone(), tab.head.clone(), tab.pr.number);
        let task = cx.background_spawn(async move {
            let repo = kagi_git::Backend::open(&repo_path).map_err(|e| format!("{e}"))?;
            repo.pr_conflict_preview(&base, &head)
                .map_err(|e| format!("{e}"))
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                // The user may have closed or switched tabs while this ran;
                // find the tab by PR number rather than by the index we had.
                let Some(m) = app.pr_mode.as_mut() else {
                    return;
                };
                let Some(t) = m.tabs.iter_mut().find(|t| t.pr.number == number) else {
                    return;
                };
                klog!(
                    "pr-conflicts: #{} {}",
                    number,
                    match &result {
                        Ok(v) => format!("{} file(s)", v.len()),
                        Err(e) => format!("error: {e}"),
                    }
                );
                t.conflicts = Some(result);
                cx.notify();
            });
        })
        .detach();
    }

    /// Back to the dashboard. Deactivates the tab without closing it, so its
    /// loaded commits / files / conversation survive the round trip.
    pub fn pr_mode_home(&mut self, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode.as_mut() {
            m.active = None;
            m.focus = PrFocus::List;
        }
        klog!("pr-mode: home");
        cx.notify();
    }

    /// Fetch the PR list now rather than waiting out the 60s ticker.
    pub fn pr_mode_refresh(&mut self, cx: &mut Context<Self>) {
        klog!("pr-mode: refresh");
        self.push_toast(ToastKind::Info, Msg::PrRefreshing.t(), cx);
        self.refresh_github_prs(cx);
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

    /// Close the tab for `number`, if open (after a merge).
    pub fn pr_mode_close_tab_for(&mut self, number: u64, cx: &mut Context<Self>) {
        let ix = self
            .pr_mode
            .as_ref()
            .and_then(|m| m.tabs.iter().position(|t| t.pr.number == number));
        if let Some(ix) = ix {
            self.pr_mode_close_tab(ix, cx);
        }
    }

    /// Select a commit (`None` = whole PR): the files list and diff follow.
    pub fn pr_mode_select_commit(&mut self, sel: Option<usize>, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode.as_mut() {
            m.view = PrView::Diff;
        }
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

    /// Title click: jump to the Overview (description), or back to the Diff.
    pub fn pr_mode_toggle_description(&mut self, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode.as_mut() {
            m.view = if m.view == PrView::Overview {
                PrView::Diff
            } else {
                PrView::Overview
            };
        }
        cx.notify();
    }

    pub fn pr_mode_select_file(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode.as_mut() {
            m.view = PrView::Diff;
        }
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

/// Default PR-list width. ~1.5× the old 230 so a card's title line survives
/// truncation and line 2 (#N · reason · checks · author) fits without
/// crowding; still draggable between LEFT_MIN..LEFT_MAX.
const LEFT_W: f32 = 345.0;
const RIGHT_W: f32 = 320.0;
/// Deepest indent level drawn in the left list; beyond it depth is shown by
/// the └ marker only, so a tall stack can never push rows out of the pane.
pub(super) const MAX_INDENT_DEPTH: usize = 3;
/// Card height: two rows (18 + 15) plus breathing room. Tighter line boxes
/// than this clipped the descenders of the title against the meta row.
const CARD_H: f32 = 42.0;
/// Commit-strip height CAP (≈7 rows + header). The strip fits its content and
/// only scrolls past this — a one-commit PR gets a one-row strip rather than a
/// mostly-empty fixed block (user request).
const COMMIT_STRIP_MAX_H: f32 = 210.0;
const ROW_H: f32 = 24.0;

pub fn render_pr_mode(app: &mut KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    // The right rail is STACK + FILES *of the open PR*. On the dashboard
    // there is no open PR, so it stood there empty — drop it and give the
    // width to the home screen (user request).
    let has_tab = app.pr_mode.as_ref().is_some_and(|m| m.active.is_some());
    let left = render_pr_list(app, cx);
    let center = render_center(app, cx);
    let right = has_tab.then(|| render_right(app, cx));
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
        .when(has_tab, |el| el.child(vdivider(DividerKind::PrModeRight)))
        .children(right)
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

/// The reading-card surface for the Overview / Review boxes, and the pane
/// behind them.
///
/// Every kagi theme puts `surface` on the *chrome* side of `bg_base` — in
/// dark themes surface is lighter, in light themes darker (verified across
/// all 11). So painting the pane `surface` and the card `bg_base` gives the
/// requested relationship for free: the card is a shade DARKER than its
/// surroundings in dark themes and LIGHTER in light ones, with no blending
/// and no per-theme table.
pub(super) fn card_bg() -> u32 {
    theme().bg_base
}

pub(super) fn card_pane_bg() -> u32 {
    theme().surface
}

/// The card is defined by its border rather than a heavy fill, so the border
/// is a muted-foreground tint instead of the near-background `selected`.
pub(super) fn card_border() -> gpui::Hsla {
    let mut c: gpui::Hsla = rgb(theme().text_muted).into();
    c.a = if theme().dark { 0.45 } else { 0.55 };
    c
}

pub(super) fn ci_glyph(ci: CiState) -> (&'static str, u32) {
    match ci {
        CiState::Success => ("\u{2713}", theme().color_success),
        CiState::Failure => ("\u{2717}", theme().color_blocker),
        CiState::Pending => ("\u{25CF}", theme().color_warning),
        CiState::None => ("\u{25CB}", theme().text_muted),
    }
}

/// The Focus Queue: PRs bucketed by *what the user should do*, not by owner.
/// Everything is derived from data the list already carries (checks, review
/// decision, mergeable) — no extra API calls.
pub(super) fn focus_queue(app: &KagiApp) -> Vec<(PrAttention, Vec<(PullRequest, PrReason)>)> {
    let login = app.github_login.clone();
    let local: Vec<String> = app
        .active_view
        .branches
        .iter()
        .map(|(n, _)| n.clone())
        .collect();
    let mut buckets: Vec<(PrAttention, Vec<(PullRequest, PrReason)>)> = [
        PrAttention::NeedsYou,
        PrAttention::InProgress,
        PrAttention::Ready,
        PrAttention::Waiting,
        PrAttention::Dormant,
    ]
    .into_iter()
    .map(|a| (a, Vec::new()))
    .collect();
    for pr in &app.github_prs {
        let group = pr.group_for(login.as_deref(), &local);
        let (att, why) = pr.attention(group == PrGroup::Mine, group == PrGroup::ReviewRequested);
        if let Some(slot) = buckets.iter_mut().find(|(a, _)| *a == att) {
            slot.1.push((pr.clone(), why));
        }
    }
    // Stack order within each bucket so a chain still reads top-down.
    for (_, members) in buckets.iter_mut() {
        let prs: Vec<PullRequest> = members.iter().map(|(p, _)| p.clone()).collect();
        let reordered: Vec<(PullRequest, PrReason)> = stack_order(&prs)
            .into_iter()
            .map(|(ix, _)| members[ix].clone())
            .collect();
        *members = reordered;
    }
    buckets.retain(|(_, m)| !m.is_empty());
    buckets
}

pub fn queue_bucket_label(a: PrAttention) -> &'static str {
    match a {
        PrAttention::NeedsYou => Msg::PrQueueNeedsYou.t(),
        PrAttention::InProgress => Msg::PrQueueInProgress.t(),
        PrAttention::Ready => Msg::PrQueueReady.t(),
        PrAttention::Waiting => Msg::PrQueueWaiting.t(),
        PrAttention::Dormant => Msg::PrQueueDormant.t(),
    }
}

/// The queue's colour language: action state, not GitHub state.
pub fn attention_color(a: PrAttention) -> u32 {
    match a {
        PrAttention::NeedsYou => theme().color_blocker,
        PrAttention::InProgress => theme().color_warning,
        PrAttention::Ready => theme().color_success,
        PrAttention::Waiting => theme().color_branch,
        PrAttention::Dormant => theme().text_muted,
    }
}

pub fn reason_text(r: &PrReason) -> String {
    match r {
        PrReason::CiFailed(n) => {
            format!("{} CI {}", n, if *n == 1 { "failure" } else { "failures" })
        }
        PrReason::ChangesRequested => Msg::PrWhyChangesRequested.t().to_string(),
        PrReason::Conflicting => Msg::PrWhyConflicting.t().to_string(),
        PrReason::CiRunning => Msg::PrWhyCiRunning.t().to_string(),
        PrReason::ReadyToMerge => Msg::PrWhyReadyToMerge.t().to_string(),
        PrReason::ReviewRequested => Msg::PrWhyReviewRequested.t().to_string(),
        PrReason::AwaitingReview => Msg::PrWhyAwaitingReview.t().to_string(),
        PrReason::Draft => Msg::PrDraft.t().to_string(),
        PrReason::None => String::new(),
    }
}

/// The left list's display order — shared by the renderer and ↑/↓ stepping so
/// they can never disagree.
fn pr_list_order(app: &KagiApp) -> Vec<PullRequest> {
    focus_queue(app)
        .into_iter()
        .flat_map(|(_, m)| m.into_iter().map(|(p, _)| p))
        .collect()
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
    for (bucket, members) in focus_queue(app) {
        col = col.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .px_3()
                .pt_2()
                .pb_1()
                .child(
                    div()
                        .w(theme::scaled_px(6.))
                        .h(theme::scaled_px(6.))
                        .rounded_full()
                        .flex_shrink_0()
                        .bg(rgb(attention_color(bucket))),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(format!(
                            "{} ({})",
                            queue_bucket_label(bucket),
                            members.len()
                        ))),
                ),
        );
        for (pr, why) in members {
            let stacked = pr.is_stacked_on(&all);
            col = col.child(render_pr_card(&pr, bucket, &why, stacked, active_pr, cx));
        }
    }
    col.into_any_element()
}

// ── Center: header + view tabs + commits + body ──────────────
fn render_center(app: &mut KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let active: Option<usize> = app.pr_mode.as_ref().and_then(|m| m.active);
    // No tab strip: the left PR list already highlights the active PR and
    // switching is one click there, so a second row of #N chips was pure
    // duplication (user request). Tabs still exist as state — opening a PR
    // keeps its loaded commits/files/conversation — they just aren't drawn.

    let mut col = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .h_full();

    let Some(ix) = active else {
        // No tab: use the whole center as a PR dashboard instead of an empty
        // hint (user request) — every open PR as a table row, click to open.
        return col
            .child(super::pr_dashboard::render_dashboard(app, cx))
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
    let (view, reviews, comments, line_comments, conflicts) = {
        let m = app.pr_mode.as_ref().unwrap();
        let t = &m.tabs[ix];
        (
            m.view,
            t.reviews.clone(),
            t.comments.clone(),
            t.line_comments.clone(),
            t.conflicts.clone(),
        )
    };
    let show_description = view == PrView::Overview;
    let show_review = view == PrView::Review;

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
        // Leaving a PR: back to the dashboard, and a manual fetch so CI state
        // can be refreshed without waiting out the ticker (user request).
        .child(super::pr_dashboard::home_button(cx))
        .child(super::pr_dashboard::refresh_button(cx))
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
        // Merge — only for a mergeable, non-draft PR; the confirm modal
        // states the CI / review caveats before anything happens.
        .when(!pr.is_draft, |el| {
            let pr_merge = pr.clone();
            let merge_click =
                cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
                    // Squash is kagi's own default (its history is squash-merged);
                    // the modal names the method it will use.
                    this.open_pr_merge_modal(
                        &pr_merge,
                        kagi_git::github::MergeMethod::Squash,
                        true,
                        cx,
                    );
                });
            let ready = pr.mergeable != kagi_domain::github::Mergeable::Conflicting;
            el.child(
                div()
                    .id("pr-mode-merge")
                    .px_2()
                    .py_px()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(if ready {
                        theme().color_success
                    } else {
                        theme().selected
                    }))
                    .text_xs()
                    .text_color(rgb(if ready {
                        theme().color_success
                    } else {
                        theme().text_muted
                    }))
                    .cursor_pointer()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .hover(|s| s.bg(rgb(theme().surface)))
                    .on_click(merge_click)
                    .child(
                        gpui::svg()
                            .path("icons/git-merge.svg")
                            .flex_shrink_0()
                            .w(theme::scaled_px(12.))
                            .h(theme::scaled_px(12.))
                            .text_color(rgb(if ready {
                                theme().color_success
                            } else {
                                theme().text_muted
                            })),
                    )
                    .child(SharedString::from(Msg::PrModeMerge.t())),
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
            .max_h(theme::scaled_px(COMMIT_STRIP_MAX_H))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .bg(rgb(theme().panel))
            .border_b_1()
            .border_color(rgb(theme().selected))
            .on_mouse_down(gpui::MouseButton::Left, commits_focus_click),
        commits_focused,
    )
    // The COMMITS header IS the "whole PR" selector: a separate "All changes"
    // row said the same thing one line below it (user request). Clicking the
    // header clears the per-commit selection; it highlights while active.
    .child(
        div()
            .id("pr-mode-commits-all")
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_3()
            .py_1()
            .cursor_pointer()
            .when(selected_commit.is_none(), |el| el.bg(rgb(theme().selected)))
            .hover(|s| s.bg(rgb(theme().surface)))
            .on_click(all_click)
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(if selected_commit.is_none() {
                        theme().text_main
                    } else {
                        theme().text_muted
                    }))
                    .child(SharedString::from(format!(
                        "{} ({})",
                        Msg::PrModeCommits.t(),
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

    // Overview | Review | Diff — one place to say what the body shows, so
    // the title click is a shortcut rather than the only route.
    let convo_n = reviews.len() + comments.len() + line_comments.len();
    let tab_btn = |id: &'static str,
                   icon: &'static str,
                   label: String,
                   on: bool,
                   view: PrView,
                   cx: &mut Context<KagiApp>| {
        let click = cx.listener(move |this: &mut KagiApp, _: &gpui::ClickEvent, _w, cx| {
            this.pr_mode_show(view, cx);
        });
        // A real tab: underline on the active one, no pill background —
        // buttons read as actions, these switch what the pane shows.
        div()
            .id(id)
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_3()
            .py_1()
            .cursor_pointer()
            .border_b_2()
            .border_color(rgb(if on {
                theme().color_branch
            } else {
                theme().panel
            }))
            .text_color(rgb(if on {
                theme().text_main
            } else {
                theme().text_sub
            }))
            .hover(|s| s.text_color(rgb(theme().text_main)))
            .on_click(click)
            .child(
                gpui::svg()
                    .path(icon)
                    .flex_shrink_0()
                    .w(theme::scaled_px(13.))
                    .h(theme::scaled_px(13.))
                    .text_color(rgb(if on {
                        theme().color_branch
                    } else {
                        theme().text_sub
                    })),
            )
            .child(div().text_xs().child(SharedString::from(label)))
    };
    let views = div()
        .flex()
        .flex_row()
        .items_center()
        .flex_shrink_0()
        .px_2()
        .bg(rgb(theme().panel))
        .border_b_1()
        .border_color(rgb(theme().selected))
        .child(tab_btn(
            "pr-view-overview",
            "icons/file-text.svg",
            Msg::PrModeOverview.t().to_string(),
            show_description,
            PrView::Overview,
            cx,
        ))
        .child(tab_btn(
            "pr-view-review",
            "icons/message-square.svg",
            if convo_n > 0 {
                format!("{} ({})", Msg::PrModeReview.t(), convo_n)
            } else {
                Msg::PrModeReview.t().to_string()
            },
            show_review,
            PrView::Review,
            cx,
        ))
        .child(tab_btn(
            "pr-view-diff",
            "icons/git-compare.svg",
            "Diff".to_string(),
            view == PrView::Diff,
            PrView::Diff,
            cx,
        ))
        // ADR-0145: only when GitHub says the merge conflicts. A tab that is
        // always there but empty six times out of seven teaches people to
        // ignore it, which is the opposite of the point.
        .when(pr.mergeable == Mergeable::Conflicting, |el| {
            el.child(tab_btn(
                "pr-view-conflicts",
                "icons/git-merge.svg",
                Msg::PrModeConflicts.t().to_string(),
                view == PrView::Conflicts,
                PrView::Conflicts,
                cx,
            ))
        });

    col = col.child(header).child(views).child(strip);
    if show_review {
        return col
            .child(super::pr_conversation::render_conversation(
                &pr,
                &reviews,
                &comments,
                &line_comments,
                cx,
            ))
            .into_any_element();
    }
    if show_description {
        return col
            .child(super::pr_conversation::render_description(&pr, cx))
            .into_any_element();
    }
    if view == PrView::Conflicts {
        return col
            .child(super::pr_conflicts::render_conflicts(conflicts.as_ref()))
            .into_any_element();
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

    // Checks — the per-check list, so "CI failed" names the job. Clicking a
    // row opens that check's run page.
    if let Some(t) = active {
        let checks = t.pr.checks.clone();
        if !checks.is_empty() {
            let failed = t.pr.failed_checks();
            col = col.child(
                section_label(format!(
                    "{} ({}{})",
                    Msg::PrModeChecks.t(),
                    checks.len(),
                    if failed > 0 {
                        format!(", {} failed", failed)
                    } else {
                        String::new()
                    }
                ))
                .border_t_1()
                .border_color(rgb(theme().surface)),
            );
            let mut list = div()
                .id("pr-mode-checks")
                .flex_shrink_0()
                .max_h(theme::scaled_px(150.))
                .overflow_y_scroll()
                .flex()
                .flex_col();
            // Failures first: the actionable ones must not need scrolling.
            let mut ordered = checks.clone();
            ordered.sort_by_key(|c| match c.state {
                CiState::Failure => 0,
                CiState::Pending => 1,
                _ => 2,
            });
            for (i, c) in ordered.iter().enumerate() {
                let (g, color) = ci_glyph(c.state);
                let url = c.url.clone();
                let open =
                    cx.listener(move |_this: &mut KagiApp, _: &gpui::ClickEvent, _w, _cx| {
                        if !url.is_empty() {
                            let _ = std::process::Command::new("open").arg(&url).spawn();
                        }
                    });
                let label = if c.workflow.is_empty() || c.workflow == c.name {
                    c.name.clone()
                } else {
                    format!("{} / {}", c.workflow, c.name)
                };
                list = list.child(
                    div()
                        .id(("pr-mode-check", i))
                        .h(theme::scaled_px(ROW_H))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .text_sm()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(theme().surface)))
                        .on_click(open)
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_color(rgb(color))
                                .child(SharedString::from(g)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_color(rgb(if c.state == CiState::Failure {
                                    theme().text_main
                                } else {
                                    theme().text_sub
                                }))
                                .child(SharedString::from(label)),
                        ),
                );
            }
            col = col.child(list);
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

/// A Focus-Queue card: identity, size, and — the point — the one line that
/// says what state it is in and why. Replaces the one-line row: at a glance
/// the user should know which PR to touch next without decoding glyphs.
fn render_pr_card(
    pr: &PullRequest,
    bucket: PrAttention,
    why: &PrReason,
    stacked: bool,
    active_pr: Option<u64>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let is_active = active_pr == Some(pr.number);
    let accent = attention_color(bucket);
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

    // Two lines instead of three, and the title gets the whole first one —
    // it is the only field that identifies the PR, so #N, the state and the
    // metadata all move to line 2 or become glyphs (user request).
    //
    // Line 2 packs, left to right: the reason (already colour-coded by the
    // accent bar, so it needs no glyph of its own), then the compact facts —
    // a stack marker, the failed/total check count, a review mark, @author.
    let reason = reason_text(why);
    let checks = match (pr.checks.len(), pr.failed_checks()) {
        (0, _) => String::new(),
        (total, 0) => format!("\u{2713}{}", total),
        (total, failed) => format!("\u{2717}{}/{}", failed, total),
    };
    let review_mark = match pr.review {
        ReviewState::Approved => Some(("\u{2714}", theme().color_success)),
        ReviewState::ChangesRequested => Some(("\u{21BA}", theme().color_warning)),
        _ => None,
    };
    div()
        .id(("pr-mode-card", pr.number as usize))
        .mx_2()
        .mb_px()
        .pl_2()
        .pr_2()
        .rounded_md()
        .flex()
        .flex_col()
        // Fixed two-row height with tight line boxes: the default line-height
        // on two stacked text divs left a lot of air, which is what made the
        // cards feel tall (user report).
        .h(theme::scaled_px(CARD_H))
        .justify_center()
        .gap_px()
        .cursor_pointer()
        // The accent bar is the state; the card body stays neutral so a wall
        // of cards doesn't turn into a wall of colour.
        .border_l_2()
        .border_color(rgb(accent))
        .when(is_active, |el| el.bg(rgb(theme().selected)))
        .hover(|s| s.bg(rgb(theme().surface)))
        .on_click(click)
        .on_mouse_down(gpui::MouseButton::Right, menu)
        .when(pr.is_draft, |el| el.opacity(0.7))
        // Line 1 — the title, full width.
        .child(
            div()
                .w_full()
                .truncate()
                .text_sm()
                .line_height(theme::scaled_px(18.))
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(pr.title.clone())),
        )
        // Line 2 — everything else, one row.
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .w_full()
                .text_xs()
                .line_height(theme::scaled_px(15.))
                .text_color(rgb(theme().text_muted))
                .child(
                    div()
                        .flex_shrink_0()
                        .child(SharedString::from(format!("#{}", pr.number))),
                )
                .when(stacked, |el| {
                    el.child(div().flex_shrink_0().child(SharedString::from("\u{21B3}")))
                })
                .when(!reason.is_empty(), |el| {
                    el.child(
                        div()
                            .min_w(px(0.))
                            .truncate()
                            .text_color(rgb(accent))
                            .child(SharedString::from(reason.clone())),
                    )
                })
                .child(div().flex_1().min_w(px(0.)))
                .when(!checks.is_empty(), |el| {
                    el.child(
                        div()
                            .flex_shrink_0()
                            .text_color(rgb(if pr.failed_checks() > 0 {
                                theme().color_blocker
                            } else {
                                theme().text_muted
                            }))
                            .child(SharedString::from(checks.clone())),
                    )
                })
                .children(review_mark.map(|(g, c)| {
                    div()
                        .flex_shrink_0()
                        .text_color(rgb(c))
                        .child(SharedString::from(g))
                }))
                .child(
                    div()
                        .flex_shrink_0()
                        .max_w(theme::scaled_px(70.))
                        .truncate()
                        .child(SharedString::from(pr.author.clone())),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod stack_tests {
    use super::{reason_text, stack_for, StackRow};
    use kagi_domain::github::{CiState, Mergeable, PrReason, PullRequest, ReviewState};

    fn pr(number: u64, head: &str, base: &str) -> PullRequest {
        PullRequest {
            number,
            title: String::new(),
            head: head.into(),
            base: base.into(),
            is_draft: false,
            ci: CiState::None,
            review: ReviewState::None,
            url: String::new(),
            author: String::new(),
            reviewers: Vec::new(),
            body: String::new(),
            checks: Vec::new(),
            mergeable: Mergeable::default(),
        }
    }

    fn chain(rows: &[StackRow]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                StackRow::Pr(p) => format!("#{}", p.number),
                StackRow::Trunk(t) => format!("trunk:{t}"),
            })
            .collect()
    }

    /// Tip first, trunk last — descendants above `pr`, ancestors below.
    #[test]
    fn linear_chain_is_tip_first_trunk_last() {
        let all = vec![
            pr(1, "feat/a", "main"),
            pr(2, "feat/b", "feat/a"),
            pr(3, "feat/c", "feat/b"),
        ];
        assert_eq!(
            chain(&stack_for(&all[1], &all)),
            vec!["#3", "#2", "#1", "trunk:main"]
        );
        // Seen from the tip: no descendants, same chain.
        assert_eq!(
            chain(&stack_for(&all[2], &all)),
            vec!["#3", "#2", "#1", "trunk:main"]
        );
        // Seen from the root: two descendants, and they must come out
        // tip-first (the collected chain is reversed before the self row).
        assert_eq!(
            chain(&stack_for(&all[0], &all)),
            vec!["#3", "#2", "#1", "trunk:main"]
        );
    }

    /// A PR whose base is nobody's head: just itself over the trunk.
    #[test]
    fn pr_with_no_parent_is_itself_over_trunk() {
        let all = vec![pr(7, "feat/solo", "main")];
        assert_eq!(chain(&stack_for(&all[0], &all)), vec!["#7", "trunk:main"]);
        // Unrelated PRs in the list must not join the chain.
        let others = vec![pr(7, "feat/solo", "main"), pr(8, "feat/x", "develop")];
        assert_eq!(
            chain(&stack_for(&others[0], &others)),
            vec!["#7", "trunk:main"]
        );
    }

    /// GitHub data is untrusted: a base/head cycle must terminate on the
    /// 50-step guard rather than loop forever.
    #[test]
    fn cycle_terminates_on_the_guard() {
        let all = vec![pr(1, "a", "b"), pr(2, "b", "a")];
        let rows = stack_for(&all[0], &all);
        assert!(
            rows.len() < 128,
            "stack_for did not terminate: {} rows",
            rows.len()
        );
        assert!(rows.len() > 2);
        assert!(matches!(rows.last(), Some(StackRow::Trunk(_))));
    }

    /// The one arm of `reason_text` the compiler cannot check: the count is
    /// interpolated and the noun is pluralised.
    #[test]
    fn ci_failure_count_is_pluralised() {
        assert_eq!(reason_text(&PrReason::CiFailed(1)), "1 CI failure");
        assert_eq!(reason_text(&PrReason::CiFailed(3)), "3 CI failures");
        assert_eq!(reason_text(&PrReason::CiFailed(0)), "0 CI failures");
    }
}
