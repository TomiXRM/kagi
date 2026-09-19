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
use kagi_domain::github::{
    stack_order, CiState, Comment, Mergeable, PrAttention, PrGroup, PrReason, PullRequest, Review,
    ReviewComment, ReviewState,
};
use kagi_domain::pr_list::{PrListFilter, PrSection, PrSort};
use kagi_git::{Commit, CommitId, FileStatus, PrConflictFile};
use kagi_ui_core::file_tree::status_badge;

use super::diff_view::{build_main_diff_view, MainDiffView};
// issue #414: `gh`-sourced PR text (title, @login, ref names, check names) is the
// most attacker-controllable text in the app — anyone can open a PR. Route every
// such display string through `safe_text` (control-byte neutralization).
use super::i18n::Msg;
use super::render_helpers::render_diff_list;
use super::render_helpers::safe_text;
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
    /// The 概要/レビュー feed's scroll. The tabs are navigation: they scroll
    /// this handle to a section instead of swapping the body (ADR-0200).
    pub feed_scroll: gpui::ScrollHandle,
    /// Reviews + issue comments + line comments ("review chat"), fetched once
    /// per tab open.
    pub reviews: Vec<Review>,
    pub comments: Vec<Comment>,
    pub line_comments: Vec<ReviewComment>,
    /// The background conversation fetch has come back (ok or not). Until
    /// then the empty lists above mean "not yet", not "none".
    pub conversation_loaded: bool,
    /// The composer's text for this PR, parked here while another PR holds the
    /// box (ADR-0200).
    pub comment_draft: String,
    /// ADR-0145: conflicts a merge would produce, computed locally on first
    /// open of the Conflicts tab. `None` = not computed yet; `Some(Ok(vec![]))`
    /// = computed and clean, which is a different thing to say than "unknown".
    pub conflicts: Option<Result<Vec<PrConflictFile>, String>>,
    /// Which conflicted file the Conflicts tab shows. Its own selection, not
    /// `selected_file`: the conflicting files are a different (usually much
    /// shorter) list than the PR's changed files, and sharing an index between
    /// them would point at the wrong row.
    pub conflict_selected: Option<usize>,
    /// Scroll state for the conflict diff, one per tab like `diff_scroll`.
    pub conflict_scroll: ListState,
    /// Which conflict within the file the jump control is on (0-based).
    pub conflict_at: usize,
    /// One loaded conflict file's rows/jumps; raw marker text is dropped after
    /// construction. Selection replaces this owner, rather than accumulating
    /// full text or derived views for every conflicting file.
    pub(crate) conflict_preview: Option<super::pr_conflicts::ConflictPreview>,
    /// #347: `mergeStateStatus` + merge-queue position + unresolved-thread
    /// count, fetched once per tab open via `gh api graphql`. `None` = not
    /// fetched yet (or non-GitHub host / old `gh` — degrade to no card).
    pub merge_status: Option<kagi_git::github::PrMergeStatus>,
    /// The merge-status fetch has come back (ok or not). Its own flag so the
    /// Overview stops loading without waiting on the review calls.
    pub merge_status_loaded: bool,
}

/// Which body the PR tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrView {
    Overview,
    Review,
    Diff,
    /// The PR's commits, full height — the mock's COMMITS tab. It used to be
    /// a strip pinned above every view, which spent 210px on a list the
    /// reader consults once per PR (ADR-0200).
    Commits,
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
}

pub struct PrModeState {
    pub tabs: Vec<PrTab>,
    pub active: Option<usize>,
    /// Keyboard focus target for ↑/↓; ←/→ cycle it. Set by clicking a pane.
    pub focus: PrFocus,
    /// Which section of the feed the next frame must scroll to: child 0 is the
    /// description, child 1 the conversation. `None` once consumed - a tab
    /// press is a jump, not a position the renderer keeps re-asserting.
    pub feed_anchor: Option<usize>,
    /// Which body the center shows. Mode-wide, NOT per tab: switching PRs
    /// while reading reviews should keep showing reviews (user request).
    pub view: PrView,
    /// Right column width (unscaled px); the left shares `SidebarState::width`.
    /// Which slice the home list shows, and in what order. Mode-wide for the
    /// same reason `view` is: it is a reading preference, not a property of a
    /// PR.
    pub filter: PrListFilter,
    pub sort: PrSort,
    /// Which navigator sections are unfolded, indexed by
    /// [`PrSection::index`]. The Inbox opens with the mode; the rest are the
    /// viewer's own lists and stay folded until asked for.
    pub sections_open: [bool; PrSection::ALL.len()],
    /// The swimlane rail's horizontal scroll, in rendered px.
    ///
    /// `None` means "follow the PR": the rail places itself so the PR's own
    /// lane is in view, which is the whole point of the pane when that lane is
    /// the fifth of five. A wheel gesture replaces it with the reader's own
    /// position, because a pane that scrolls back on its own cannot be
    /// scrolled. Opening another PR returns it to following.
    pub lane_scroll_x: Option<f32>,
}

impl Default for PrModeState {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
            focus: PrFocus::List,
            feed_anchor: None,
            view: PrView::Overview,
            filter: PrListFilter::default(),
            sort: PrSort::default(),
            sections_open: [true, false, false, false],
            lane_scroll_x: None,
        }
    }
}

const COMMIT_LIMIT: usize = 500;

impl KagiApp {
    pub fn toggle_pr_mode(&mut self, cx: &mut Context<Self>) {
        if self.pr_mode().is_some() {
            self.with_ui(|ui| ui.pr_mode = None);
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
            self.with_ui(|ui| ui.pr_mode = Some(PrModeState::default()));
            klog!("pr-mode: opened");
        }
        cx.notify();
    }

    /// Open (or activate) a tab for `pr` and load its content.
    pub fn pr_mode_open(&mut self, pr: &PullRequest, cx: &mut Context<Self>) {
        if self.pr_mode().is_none() {
            self.toggle_pr_mode(cx);
        }
        if let Some(ix) = self
            .pr_mode()
            .and_then(|m| m.tabs.iter().position(|t| t.pr.number == pr.number))
        {
            if let Some(m) = self.pr_mode_mut() {
                m.active = Some(ix);
                // Another PR, another lane: the rail goes back to following it
                // rather than staying at the position the last one needed.
                m.lane_scroll_x = None;
                reset_view_if_not_conflicting(m, pr);
            }
            cx.notify();
            return;
        }
        let tip = |name: &str| {
            self.view()
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
        let Some(session) = self.ui().repo_session.as_ref() else {
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
            conversation_loaded: false,
            comment_draft: String::new(),
            feed_scroll: gpui::ScrollHandle::new(),
            conflicts: None,
            conflict_selected: None,
            conflict_scroll: ListState::new(0, gpui::ListAlignment::Top, px(200.)),
            conflict_preview: None,
            conflict_at: 0,
            merge_status: None,
            merge_status_loaded: false,
        };
        if !tab.files.is_empty() {
            tab.selected_file = Some(0);
        }
        self.pr_tab_reload_diff(&mut tab);
        let Some(m) = self.pr_mode_mut() else { return };
        m.tabs.push(tab);
        m.active = Some(m.tabs.len() - 1);
        reset_view_if_not_conflicting(m, pr);
        cx.notify();
        self.pr_mode_load_conversation(pr.number, cx);
    }

    /// Fetch reviews + comments, and separately the merge status, for `number`
    /// in the background and drop them on the matching tab. Once per tab open
    /// (never per list refresh — the list ticker must stay one call). The two
    /// run side by side so each tab's loader ends on its own data.
    fn pr_mode_load_conversation(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some(repo) = self.repo_path.clone() else {
            return;
        };
        let repo2 = repo.clone();
        let owner = self.active_session();
        cx.spawn(async move |this, acx| {
            // #347: mergeStateStatus + merge-queue position. A failure
            // (non-GitHub host, old gh, no MQ) is not fatal — the card just
            // does not appear.
            let merge_status = acx
                .background_executor()
                .spawn(async move { kagi_git::github::pr_merge_status(&repo2, number).ok() })
                .await;
            let _ = this.update(acx, |app, cx| {
                let Some(t) = app
                    .pr_mode_of(owner)
                    .and_then(|m| m.tabs.iter_mut().find(|t| t.pr.number == number))
                else {
                    return;
                };
                if let Some(s) = &merge_status {
                    klog!(
                        "pr-mode: merge-status #{} state={:?} queued={}",
                        number,
                        s.state,
                        s.queue.is_some()
                    );
                }
                t.merge_status = merge_status;
                t.merge_status_loaded = true;
                cx.notify();
            });
        })
        .detach();
        cx.spawn(async move |this, acx| {
            let (convo, lines) = acx
                .background_executor()
                .spawn(async move {
                    // Two calls: `gh pr view` for the verdicts + issue
                    // comments, `gh api` for the line comments (where Copilot
                    // / Codex put code suggestions — not exposed by --json).
                    let convo = kagi_git::github::pr_conversation(&repo, number);
                    let lines = kagi_git::github::pr_review_comments(&repo, number);
                    (convo, lines)
                })
                .await;
            let _ = this.update(acx, |app, cx| {
                let Some(t) = app
                    .pr_mode_of(owner)
                    .and_then(|m| m.tabs.iter_mut().find(|t| t.pr.number == number))
                else {
                    return;
                };
                // Stop the loader even on failure; the pane then falls back to
                // its "no reviews" wording as before.
                t.conversation_loaded = true;
                cx.notify();
                let Ok((reviews, comments)) = convo else {
                    return;
                };
                let line_comments = lines.unwrap_or_default();
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
            });
        })
        .detach();
    }

    /// Switch the body between Overview (description), Review, Diff and
    /// Conflicts.
    pub fn pr_mode_show(&mut self, view: PrView, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode_mut() {
            m.view = view;
            // 概要 and レビュー are one page: the tab scrolls the feed to its
            // section rather than replacing what is on screen (ADR-0200).
            m.feed_anchor = match view {
                PrView::Overview => Some(0),
                PrView::Review => Some(1),
                _ => None,
            };
        }
        if view == PrView::Conflicts {
            self.pr_mode_load_conflicts(cx);
        }
        cx.notify();
    }

    /// Select which conflicted file the Conflicts tab shows.
    pub fn pr_mode_select_conflict(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode_mut() {
            if let Some(t) = m.active.and_then(|a| m.tabs.get_mut(a)) {
                if t.conflict_selected.unwrap_or(0) != ix {
                    t.conflict_preview = None;
                }
                t.conflict_selected = Some(ix);
                t.conflict_at = 0;
            }
        }
        self.pr_mode_load_conflict_text(cx);
        cx.notify();
    }

    /// Move the conflict cursor and scroll that conflict into view.
    pub fn pr_mode_jump_conflict(
        &mut self,
        at: usize,
        row: Option<usize>,
        rows: Option<std::sync::Arc<Vec<super::diff_view::DiffRow>>>,
        cx: &mut Context<Self>,
    ) {
        if let Some(m) = self.pr_mode_mut() {
            if let Some(t) = m.active.and_then(|a| m.tabs.get_mut(a)) {
                t.conflict_at = at;
                if let (Some(row), Some(rows)) = (row, rows) {
                    // Side-by-side virtualizes over *paired* rows, so a unified
                    // index addresses a different element there.
                    let split = super::theme::diff_split();
                    let (count, target) = if split {
                        let sp = super::diff_split::split_rows(&rows);
                        let t = super::diff_split::split_index_of(&sp, row).unwrap_or(row);
                        (sp.len(), t)
                    } else {
                        (rows.len(), row)
                    };
                    // Sync the count first. `render_diff_list` resets the list
                    // whenever its count disagrees, and `reset` clears
                    // `logical_scroll_top` — so scrolling before the list has
                    // been told how many rows there are is thrown away on the
                    // very next frame, which is why this did nothing.
                    if t.conflict_scroll.item_count() != count {
                        t.conflict_scroll.reset(count);
                    }
                    // `scroll_to`, not `scroll_to_reveal_item`: revealing seeks
                    // by accumulated height, and the list only measures rows it
                    // has drawn, so downward targets land short. Setting the
                    // top item is exact regardless of what has been measured —
                    // and puts the conflict header at the top, which is where
                    // it belongs.
                    t.conflict_scroll.scroll_to(gpui::ListOffset {
                        item_ix: target,
                        offset_in_item: px(0.),
                    });
                }
            }
        }
        cx.notify();
    }

    /// Fetch the marker text for the selected conflicted file, if it is not
    /// already the one held. One file at a time: see `PrTab::conflict_preview`.
    fn pr_mode_load_conflict_text(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.pr_mode() else {
            return;
        };
        let Some(tab) = m.active.and_then(|a| m.tabs.get(a)) else {
            return;
        };
        let Some(Ok(files)) = tab.conflicts.as_ref() else {
            return;
        };
        if files.is_empty() {
            return;
        }
        let ix = tab.conflict_selected.unwrap_or(0).min(files.len() - 1);
        let path = files[ix].path.clone();
        if tab.conflict_preview.as_ref().map(|preview| preview.path()) == Some(path.as_path()) {
            return;
        }
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let (base, head, number) = (tab.base_tip.clone(), tab.head.clone(), tab.pr.number);
        let bg_path = path.clone();
        let owner = self.active_session();
        let task = cx.background_spawn(async move {
            let repo = kagi_git::Backend::open(&repo_path).ok()?;
            repo.pr_conflict_text(&base, &head, &bg_path).ok().flatten()
        });
        cx.spawn(async move |this, acx| {
            let text = task.await;
            let _ = this.update(acx, |app, cx| {
                let Some(m) = app.pr_mode_of(owner) else {
                    return;
                };
                let Some(t) = m.tabs.iter_mut().find(|t| t.pr.number == number) else {
                    return;
                };
                // Land on the first conflict rather than the top of the
                // file: in a 2000-line file the interesting part is nowhere
                // near where the scroll starts, and hunting for it is the work
                // this tab exists to remove.
                let first = t.apply_conflict_text(&path, text.as_deref());
                cx.notify();
                if let Some((row, rows)) = first.filter(|_| app.active_session() == owner) {
                    app.pr_mode_jump_conflict(0, Some(row), Some(rows), cx);
                }
            });
        })
        .detach();
    }

    /// Compute the active tab's conflict preview, once (ADR-0145).
    ///
    /// Off the UI thread: it is a full three-way merge of two trees, the same
    /// work `plan_merge_branch` does, and on a large repo that is long enough
    /// to drop frames. Cached on the tab because the answer only changes when
    /// the PR or the base does, and re-running it on every render of a tab the
    /// user is *looking at* would be the worst possible cadence.
    fn pr_mode_load_conflicts(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.pr_mode() else {
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
            repo.pr_conflict_files(&base, &head)
                .map_err(|e| format!("{e}"))
        });
        let owner = self.active_session();
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                // The user may have closed or switched tabs while this ran;
                // find the tab by PR number rather than by the index we had.
                let Some(m) = app.pr_mode_of(owner) else {
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
                // The list has just arrived; pull the first file's text so the
                // tab is not left showing an empty pane beside a full list.
                if app.active_session() == owner {
                    app.pr_mode_load_conflict_text(cx);
                }
            });
        })
        .detach();
    }

    /// Back to the dashboard. Deactivates the tab without closing it, so its
    /// loaded commits / files / conversation survive the round trip.
    pub fn pr_mode_home(&mut self, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode_mut() {
            m.active = None;
            m.focus = PrFocus::List;
        }
        klog!("pr-mode: home");
        cx.notify();
    }

    /// Fetch the PR list now rather than waiting out the 60s ticker.
    pub fn pr_mode_refresh(&mut self, cx: &mut Context<Self>) {
        klog!("pr-mode: refresh");
        // In flight, so it takes the spinning sync icon rather than a glyph
        // that cannot turn (user report).
        self.push_toast(ToastKind::Sync, Msg::PrRefreshing.t(), cx);
        self.refresh_github_prs(cx);
    }

    pub fn pr_mode_close_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode_mut() {
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
            .pr_mode()
            .and_then(|m| m.tabs.iter().position(|t| t.pr.number == number));
        if let Some(ix) = ix {
            self.pr_mode_close_tab(ix, cx);
        }
    }

    /// Select a commit (`None` = whole PR): the files list and diff follow.
    pub fn pr_mode_select_commit(&mut self, sel: Option<usize>, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode_mut() {
            m.view = PrView::Diff;
        }
        let Some(mut tab) = self.pr_mode_take_active() else {
            return;
        };
        tab.selected_commit = sel;
        tab.files = match self.ui().repo_session.as_ref().map(|s| s.backend()) {
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

    /// A swimlane row was clicked: show that commit in the PR that owns it.
    ///
    /// The lane pane draws every open tab, so a row may belong to a PR that is
    /// not the active one — activating its tab first is what makes the click
    /// land on the commit the user actually pointed at, rather than on an
    /// index into someone else's range.
    pub(super) fn pr_lane_select(&mut self, pr: u64, commit: &CommitId, cx: &mut Context<Self>) {
        let Some(ix) = self
            .pr_mode()
            .and_then(|m| m.tabs.iter().position(|t| t.pr.number == pr))
        else {
            return;
        };
        if let Some(m) = self.pr_mode_mut() {
            m.active = Some(ix);
        }
        let sel = self
            .pr_mode()
            .and_then(|m| m.tabs.get(ix))
            .and_then(|t| t.commits.iter().position(|c| c.id == *commit));
        self.pr_mode_select_commit(sel, cx);
    }

    /// Scroll the swimlane rail sideways.
    ///
    /// Taking the wheel means taking it for good: the rail stops following the
    /// PR's own lane and stays where the reader put it, because a pane that
    /// scrolls back on its own cannot be scrolled. Opening another PR restores
    /// following. Vertical deltas are ignored — the rows scroll on their own.
    pub(super) fn pr_lane_scroll_by(
        &mut self,
        delta: &gpui::ScrollDelta,
        lanes: usize,
        rail: f32,
        // Where the rail is *now*, following included, so the first wheel
        // gesture continues from what is on screen instead of jumping to 0.
        current: f32,
        cx: &mut Context<Self>,
    ) {
        let dx = match delta {
            gpui::ScrollDelta::Pixels(p) => f32::from(p.x),
            // One "line" step is one lane pitch, as in the commit list.
            gpui::ScrollDelta::Lines(l) => l.x * super::graph_view::lane_w(),
        };
        if dx.abs() < 0.01 {
            return;
        }
        let max = super::pr_lane::max_scroll(lanes, rail);
        let next = (current - dx).clamp(0.0, max);
        let Some(m) = self.pr_mode_mut() else {
            return;
        };
        if (next - current).abs() > 0.1 || m.lane_scroll_x.is_none() {
            m.lane_scroll_x = Some(next);
            cx.notify();
        }
    }

    /// Title click: jump to the Overview (description), or back to the Diff.
    pub fn pr_mode_toggle_description(&mut self, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode_mut() {
            m.view = if m.view == PrView::Overview {
                PrView::Diff
            } else {
                PrView::Overview
            };
        }
        cx.notify();
    }

    pub fn pr_mode_select_file(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode_mut() {
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

    /// Fold or unfold one navigator section (ADR-0200).
    pub fn pr_mode_toggle_section(&mut self, section: PrSection, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode_mut() {
            let slot = &mut m.sections_open[section.index()];
            *slot = !*slot;
        }
        cx.notify();
    }

    /// Which slice the home list shows.
    pub fn pr_mode_set_filter(&mut self, filter: PrListFilter, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode_mut() {
            m.filter = filter;
        }
        cx.notify();
    }

    /// Cycle the home list's order — the chip is one control, not three.
    pub fn pr_mode_cycle_sort(&mut self, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode_mut() {
            m.sort = match m.sort {
                PrSort::Updated => PrSort::Created,
                PrSort::Created => PrSort::Number,
                PrSort::Number => PrSort::Updated,
            };
        }
        cx.notify();
    }

    /// ←/→: cycle the focused pane (List → Commits → Files).
    pub fn pr_mode_cycle_focus(&mut self, delta: i32, cx: &mut Context<Self>) {
        const ORDER: [PrFocus; 3] = [PrFocus::List, PrFocus::Commits, PrFocus::Files];
        if let Some(m) = self.pr_mode_mut() {
            let i = ORDER.iter().position(|f| *f == m.focus).unwrap_or(0) as i32;
            let n = ORDER.len() as i32;
            m.focus = ORDER[((i + delta) % n + n) as usize % ORDER.len()];
        }
        cx.notify();
    }

    /// ↑/↓ inside the focused pane.
    pub fn pr_mode_step(&mut self, delta: i32, cx: &mut Context<Self>) {
        let Some(m) = self.pr_mode() else {
            return;
        };
        match m.focus {
            PrFocus::List => {
                // Step through the left list in its display order; open the
                // neighbour of the active PR (or the first one).
                let order = super::pr_nav::pr_list_order(self);
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
        }
    }

    /// Lazy-create the PR comment composer and keep its text with the PR it was
    /// typed for (ADR-0200). Runs on the window-bearing render pass, because
    /// `InputState::new` needs a `&mut Window` and `render_pr_mode` has none.
    ///
    /// Switching PRs parks the text in the tab it belongs to and loads the new
    /// tab's draft, so a half-written comment is never posted to the wrong PR
    /// and never silently lost.
    /// Re-read the conversation of `number` after a write to it (ADR-0200).
    /// The load itself already freezes its owner, so a tab switch mid-flight
    /// lands the rows on the PR they belong to and nowhere else.
    pub(crate) fn pr_mode_reload_conversation(&mut self, number: u64, cx: &mut Context<Self>) {
        self.pr_mode_load_conversation(number, cx);
    }

    /// Empty the composer for `number` - the text is on the server now. Only
    /// the box currently holding that PR's text is reset, so a draft parked for
    /// another PR survives.
    pub(crate) fn clear_pr_comment_draft(&mut self, number: u64, cx: &mut Context<Self>) {
        if let Some(tab) = self
            .pr_mode_mut()
            .and_then(|m| m.tabs.iter_mut().find(|t| t.pr.number == number))
        {
            tab.comment_draft.clear();
        }
        if self.pr_comment_for == Some(number) {
            // `InputState::set_value` needs a `&mut Window`, which a completion
            // callback has none of. Dropping the entity is what the next
            // window-bearing frame rebuilds from the (now empty) draft.
            self.pr_comment_input = None;
            self.pr_comment_for = None;
        }
        cx.notify();
    }

    pub(crate) fn sync_pr_comment_input(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let open = self
            .pr_mode()
            .and_then(|m| m.active.and_then(|ix| m.tabs.get(ix)))
            .map(|t| t.pr.number);
        let Some(number) = open else {
            return;
        };
        if self.pr_comment_input.is_none() {
            let input = cx.new(|cx| {
                gpui_component::input::InputState::new(window, cx)
                    .multi_line(true)
                    .auto_grow(2, 8)
                    .placeholder(Msg::PrCommentPlaceholder.t())
            });
            self.pr_comment_input = Some(input);
        }
        let Some(input) = self.pr_comment_input.clone() else {
            return;
        };
        if self.pr_comment_for == Some(number) {
            // Same PR: the box is the truth, the tab keeps the copy.
            let text = input.read(cx).value().to_string();
            if let Some(tab) = self
                .pr_mode_mut()
                .and_then(|m| m.active.and_then(|ix| m.tabs.get_mut(ix)))
            {
                if tab.comment_draft != text {
                    tab.comment_draft = text;
                }
            }
            return;
        }
        // Another PR took the box: park the old text, load this tab's draft.
        let parked = input.read(cx).value().to_string();
        if let Some(previous) = self.pr_comment_for.take() {
            if let Some(tab) = self
                .pr_mode_mut()
                .and_then(|m| m.tabs.iter_mut().find(|t| t.pr.number == previous))
            {
                tab.comment_draft = parked;
            }
        }
        let draft = self
            .pr_mode()
            .and_then(|m| m.tabs.iter().find(|t| t.pr.number == number))
            .map(|t| t.comment_draft.clone())
            .unwrap_or_default();
        input.update(cx, |state, cx| state.set_value(draft, window, cx));
        self.pr_comment_for = Some(number);
    }

    pub(super) fn pr_mode_focus(&mut self, f: PrFocus, cx: &mut Context<Self>) {
        if let Some(m) = self.pr_mode_mut() {
            m.focus = f;
        }
        cx.notify();
    }

    // Take/put the active tab so `pr_tab_reload_diff` can borrow `self`
    // (the repo session) without fighting the `pr_mode` borrow.
    fn pr_mode_take_active(&mut self) -> Option<PrTab> {
        let m = self.pr_mode_mut()?;
        let ix = m.active?;
        (ix < m.tabs.len()).then(|| m.tabs.remove(ix))
    }
    fn pr_mode_put_active(&mut self, tab: PrTab) {
        if let Some(m) = self.pr_mode_mut() {
            let ix = m.active.unwrap_or(0).min(m.tabs.len());
            m.tabs.insert(ix, tab);
            m.active = Some(ix);
        }
    }

    fn pr_tab_reload_diff(&self, tab: &mut PrTab) {
        let Some(session) = self.ui().repo_session.as_ref() else {
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

/// The centre view is mode-wide so that switching PRs while reading reviews
/// keeps showing reviews. Conflicts is the exception: it only exists for a PR
/// that has them, so carrying it to one that does not leaves the pane on a tab
/// with no button and nothing to say.
fn reset_view_if_not_conflicting(m: &mut PrModeState, pr: &PullRequest) {
    if m.view == PrView::Conflicts && pr.mergeable != Mergeable::Conflicting {
        m.view = PrView::Overview;
    }
}

/// A centred one-line note in the PR centre pane (no file selected, nothing to
/// show yet, an error). Extracted so the Conflicts tab's three empty states
/// look like the Diff tab's rather than approximately like it.
fn pr_center_note(text: SharedString) -> gpui::AnyElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(rgb(theme().text_muted))
        .child(text)
        .into_any_element()
}

/// Commit-strip height CAP (≈7 rows + header). The strip fits its content and
/// only scrolls past this — a one-commit PR gets a one-row strip rather than a
/// mostly-empty fixed block (user request).
const COMMIT_STRIP_MAX_H: f32 = 210.0;
const ROW_H: f32 = 24.0;
pub fn render_pr_mode(app: &mut KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let has_tab = app.pr_mode().is_some_and(|m| m.active.is_some());
    // Faces for the logins on this page (ADR-0200). One attempt per login per
    // process; the pass is a set lookup once they are in hand.
    app.ensure_pr_avatars(cx);
    let left = super::e2e::measure_control(
        "pr-mode-left-pane",
        super::workspace_mode::render_sidebar_pages(
            app,
            super::workspace_mode::WorkspaceMode::Prs,
            cx,
        ),
    );
    let center = div()
        .id("pr-mode-center-pane")
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .h_full()
        .child(super::e2e::measure_control(
            "pr-mode-center-pane",
            render_center(app, cx),
        ));
    // The swimlane sits between the navigator and the body: it is about the
    // PRs, not about the file being read, and it is absent with no tab open.
    let lane = super::pr_lane::render_pr_lane(app, cx)
        .map(|pane| super::e2e::measure_control("pr-mode-lane-pane", pane));
    // No outer right pane: the checks, the facts and the file list moved
    // inside the PR body, where the mock has them (ADR-0200). Three panes
    // between the navigator and the diff left the diff a sliver.
    div()
        .id("pr-mode-layout")
        .flex()
        .flex_row()
        .flex_1()
        .min_w(px(0.))
        .min_h(px(0.))
        .h_full()
        .bg(rgb(theme().bg_base))
        .child(left)
        .child(vdivider(DividerKind::PrModeLeft))
        .children(lane)
        .when(has_tab, |el| el.child(vdivider(DividerKind::PrModeLeft)))
        .child(center)
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
/// A reading card on the PR page: one step off the page's own background, so
/// the card is legible without the page turning grey.
pub(super) fn card_bg() -> u32 {
    theme().panel
}

/// The PR page itself. It is the app's base background, like every other main
/// pane - it used to be `surface`, which read as a grey panel beside the black
/// ones around it (user report).
pub(super) fn card_pane_bg() -> u32 {
    theme().bg_base
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
    let local: Vec<String> = app.view().branches.iter().map(|(n, _)| n.clone()).collect();
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
    for pr in &app.ui().github_prs {
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

/// A pane's focus cue: a 2px accent top border when it owns the arrow keys.
pub(super) fn focus_border<E: gpui::Styled>(el: E, focused: bool) -> E {
    el.border_t_2().border_color(rgb(if focused {
        theme().color_branch
    } else {
        theme().panel
    }))
}

// ── Center: header + view tabs + commits + body ──────────────
fn render_center(app: &mut KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let active: Option<usize> = app.pr_mode().and_then(|m| m.active);
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
    let (pr, commits, selected_commit, diff, scroll, files_n) = {
        let m = app.pr_mode().unwrap();
        let t = &m.tabs[ix];
        (
            t.pr.clone(),
            t.commits.clone(),
            t.selected_commit,
            t.diff.clone(),
            t.diff_scroll.clone(),
            t.files.len(),
        )
    };
    let (
        view,
        reviews,
        comments,
        line_comments,
        conflicts,
        conflict_selected,
        conflict_scroll,
        conflict_at,
        merge_status,
        conversation_loaded,
        merge_status_loaded,
    ) = {
        let m = app.pr_mode().unwrap();
        let t = &m.tabs[ix];
        (
            m.view,
            t.reviews.clone(),
            t.comments.clone(),
            t.line_comments.clone(),
            t.conflicts.clone(),
            t.conflict_selected,
            t.conflict_scroll.clone(),
            t.conflict_at,
            t.merge_status.clone(),
            t.conversation_loaded,
            t.merge_status_loaded,
        )
    };
    let feed_scroll = app.pr_mode().unwrap().tabs[ix].feed_scroll.clone();
    // A tab press is one jump. Consume it here so a later frame - a fetch
    // landing, a resize - does not drag the reader back to the heading.
    if let Some(anchor) = app.pr_mode_mut().and_then(|m| m.feed_anchor.take()) {
        feed_scroll.scroll_to_top_of_item(anchor);
    }
    // 概要 and レビュー are two sections of one page, so both tabs draw it;
    // which chip is lit is which section the reader last jumped to.
    let show_feed = matches!(view, PrView::Overview | PrView::Review);
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
    let merge_held = app.repo_path.as_ref().is_some_and(|owner| {
        app.transport_holds
            .contains(owner, &format!("pr-merge #{}", pr.number))
    });
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
                .child(safe_text(&pr.title))
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
        .when(!pr.is_draft && !merge_held, |el| {
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
    let commits_focused = app.pr_mode().map(|m| m.focus) == Some(PrFocus::Commits);
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
                        .child(safe_text(&cmt.summary)),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(rgb(theme().text_sub))
                        .child(safe_text(&cmt.author.name)),
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
            // The mock's `FILES n`: the count is what says whether this is a
            // one-line fix or a rewrite, and it is already in the tab's data.
            match files_n {
                0 => Msg::PrModeFiles.t().to_string(),
                n => format!("{} ({})", Msg::PrModeFiles.t(), n),
            },
            view == PrView::Diff,
            PrView::Diff,
            cx,
        ))
        .child(tab_btn(
            "pr-view-commits",
            "icons/git-commit.svg",
            match commits.len() {
                0 => Msg::PrModeCommits.t().to_string(),
                n => format!("{} ({})", Msg::PrModeCommits.t(), n),
            },
            view == PrView::Commits,
            PrView::Commits,
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

    col = col.child(header).child(views);

    // The body below the tabs is the view's own content, full width.
    let mut content = div().flex_1().min_w(px(0.)).min_h(px(0.)).flex().flex_col();
    // The commits are their own tab now, at full height, instead of a strip
    // pinned above every other view (ADR-0200).
    if view == PrView::Commits {
        content = content.child(strip.flex_1().max_h(relative(1.)));
    } else if show_feed {
        // #347: the merge-status card rides at the top of the feed - the four
        // `mergeStateStatus` actions, queue position, and what is still
        // missing. The animated loading rows stay above the scroll pane:
        // `with_animation` does not tick inside one.
        if !merge_status_loaded || !conversation_loaded {
            content = content.child(super::pr_conversation::render_loading());
        }
        let merge_card = merge_status.as_ref().and_then(|status| {
            let status_view = super::pr_merge_status::view_from(status, pr.review);
            super::pr_merge_status::render(&status_view, cx)
        });
        content = content.child(super::pr_conversation::render_feed(
            app,
            super::pr_conversation::Feed {
                pr: &pr,
                reviews: &reviews,
                comments: &comments,
                line_comments: &line_comments,
                loaded: conversation_loaded,
                merge_card,
                scroll: &feed_scroll,
            },
            cx,
        ));
    } else if view == PrView::Conflicts {
        let body: gpui::AnyElement = match conflicts.as_ref() {
            None => pr_center_note(SharedString::from("\u{2026}")),
            Some(Err(e)) => pr_center_note(SharedString::from(e.clone())),
            Some(Ok(files)) if files.is_empty() => {
                pr_center_note(SharedString::from(Msg::PrConflictsNone.t()))
            }
            Some(Ok(files)) => {
                let file_index = conflict_selected.unwrap_or(0).min(files.len() - 1);
                // Loaded rows belong to the tab. A loading shell is just one
                // explanatory row; it must not borrow another file's content.
                let (dv, jumps) = app
                    .pr_mode_mut()
                    .and_then(|mode| mode.tabs.get_mut(ix))
                    .and_then(|tab| tab.conflict_preview.as_mut())
                    .filter(|preview| preview.path() == files[file_index].path)
                    .map(|preview| preview.snapshot())
                    .unwrap_or_else(|| {
                        super::pr_conflicts::conflict_diff_view(&files[file_index], None).snapshot()
                    });
                // Prev/next sits in the header's `leading` slot, beside the
                // unified/side-by-side toggle it shares a row with.
                // Shown even for a single conflict: "1/1" answers "is there
                // more of this?", which is the question the control exists for.
                let nav = (!jumps.is_empty()).then(|| {
                    super::pr_conflicts::render_jump_nav(
                        jumps.clone(),
                        dv.rows.clone(),
                        conflict_at,
                        cx,
                    )
                });
                render_diff_list::<KagiApp>(dv, nav, None, conflict_scroll, cx).into_any_element()
            }
        };
        content = content.child(body);
    } else {
        // Diff
        let diff_el: gpui::AnyElement = match diff {
            Some(dv) => render_diff_list::<KagiApp>(dv, None, None, scroll, cx).into_any_element(),
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
        content = content.child(diff_el);
    }

    // Every pixel of the body is the view's own: the files and the checks live
    // in the swimlane pane's lower third (ADR-0200), not in a column of their
    // own. A pane that exists to describe the PR must not eat the width of the
    // diff being read (user report).
    //
    // The composer is pinned under the feed rather than scrolling with it, the
    // way github.com keeps it reachable at the foot of the conversation.
    col.child(content)
        .children(
            show_feed
                .then(|| super::pr_page::render_composer(app, cx))
                .flatten(),
        )
        .into_any_element()
}

/// The per-check list, so "CI failed" names the job. Clicking a row opens
/// that check's run page. `None` when the PR reported no checks - an empty
/// section header says nothing the reader can use.
///
/// It lives in the PR body's facts rail (ADR-0200): the mock has no outer
/// right pane, and checks are a fact about the PR rather than a third pane of
/// the window.
fn render_checks(tab: &PrTab, cx: &mut Context<KagiApp>) -> Option<gpui::AnyElement> {
    let checks = tab.pr.checks.clone();
    if checks.is_empty() {
        return None;
    }
    let failed = tab.pr.failed_checks();
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
        let open = cx.listener(move |_this: &mut KagiApp, _: &gpui::ClickEvent, _w, _cx| {
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
                        .child(safe_text(&label)),
                ),
        );
    }
    Some(
        div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .child(section_label(format!(
                "{} ({}{})",
                Msg::PrModeChecks.t(),
                checks.len(),
                if failed > 0 {
                    format!(", {} failed", failed)
                } else {
                    String::new()
                }
            )))
            .child(list)
            .into_any_element(),
    )
}

/// What the swimlane pane's lower third shows about the PR on screen: the
/// files of the view when a file view is open, and otherwise the checks.
///
/// It lives in an existing pane on purpose. As a column of its own - left or
/// right of the body - it only narrowed the diff the reader came for
/// (ADR-0200, user report). The PR's *properties* are not here: they are the
/// first rows of the PR's own page, where github.com's reader looks for them.
pub(super) fn render_pr_detail(
    app: &KagiApp,
    tab: &PrTab,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let files_view = app
        .pr_mode()
        .map(|m| matches!(m.view, PrView::Diff | PrView::Conflicts))
        .unwrap_or(false);
    if files_view {
        return render_file_list(app, cx);
    }
    div()
        .id("pr-mode-facts")
        .size_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .children(render_checks(tab, cx))
        .into_any_element()
}

/// The files of the view on screen, listed in the swimlane pane's lower third.
///
/// In the Conflicts view this lists the conflicting files instead: they are a
/// different, usually much shorter set, and showing the PR's whole changed-file
/// list beside a conflict diff invites clicking a row that has nothing to do
/// with what is on screen.
fn render_file_list(app: &KagiApp, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let active = app
        .pr_mode()
        .and_then(|m| m.active.and_then(|i| m.tabs.get(i)));
    let focus = app.pr_mode().map(|m| m.focus);
    let conflicts_view = app
        .pr_mode()
        .map(|m| m.view == PrView::Conflicts)
        .unwrap_or(false);
    let (files, selected_file) = if conflicts_view {
        active
            .map(|t| {
                let files = match t.conflicts.as_ref() {
                    Some(Ok(c)) => c
                        .iter()
                        .map(|f| FileStatus {
                            path: f.path.clone(),
                            change: kagi_git::ChangeKind::Modified,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                let sel = (!files.is_empty()).then(|| t.conflict_selected.unwrap_or(0));
                (files, sel)
            })
            .unwrap_or_default()
    } else {
        active
            .map(|t| (t.files.clone(), t.selected_file))
            .unwrap_or_default()
    };
    let files_focus_click = cx.listener(|this: &mut KagiApp, _: &gpui::MouseDownEvent, _w, cx| {
        this.pr_mode_focus(PrFocus::Files, cx);
    });
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
            if conflicts_view {
                this.pr_mode_select_conflict(i, cx);
            } else {
                this.pr_mode_select_file(i, cx);
            }
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
    focus_border(
        div()
            .id("pr-mode-files-pane")
            .size_full()
            .flex()
            .flex_col()
            .on_mouse_down(gpui::MouseButton::Left, files_focus_click),
        focus == Some(PrFocus::Files),
    )
    .child(section_label(format!(
        "{} ({})",
        Msg::PrModeFiles.t(),
        files.len()
    )))
    .child(list)
    .into_any_element()
}

/// GitHub's own six-hex label colour, or the neutral border when it is absent
/// or malformed. A label's colour is how it is recognised at a glance, so it
/// is worth carrying through rather than painting every pill the same.
pub(super) fn label_color(hex: &str) -> u32 {
    u32::from_str_radix(hex.trim_start_matches('#'), 16)
        .ok()
        .filter(|_| hex.trim_start_matches('#').len() == 6)
        .unwrap_or(theme().text_muted)
}

/// REVIEWERS / ASSIGNEES / LABELS / WORKTREE — the facts about the PR itself.
#[cfg(test)]
mod view_reset_tests {
    use super::*;

    fn pr_with(mergeable: Mergeable) -> PullRequest {
        PullRequest {
            mergeable,
            ..super::pr_fixture_tests::pr(1, "h", "b")
        }
    }

    /// The Conflicts tab exists only for a PR that has conflicts, so carrying
    /// it across to one that does not leaves the pane on a tab with no button
    /// and nothing to show.
    #[test]
    fn conflicts_view_falls_back_when_the_next_pr_is_clean() {
        for m in [Mergeable::Clean, Mergeable::Unknown] {
            let mut state = PrModeState {
                view: PrView::Conflicts,
                ..Default::default()
            };
            reset_view_if_not_conflicting(&mut state, &pr_with(m));
            assert_eq!(state.view, PrView::Overview, "{m:?}");
        }
    }

    /// …and it stays put when the next PR does conflict, and never disturbs
    /// the other views, which are mode-wide on purpose.
    #[test]
    fn every_other_case_is_left_alone() {
        let mut state = PrModeState {
            view: PrView::Conflicts,
            ..Default::default()
        };
        reset_view_if_not_conflicting(&mut state, &pr_with(Mergeable::Conflicting));
        assert_eq!(state.view, PrView::Conflicts);

        for view in [PrView::Overview, PrView::Review, PrView::Diff] {
            let mut state = PrModeState {
                view,
                ..Default::default()
            };
            reset_view_if_not_conflicting(&mut state, &pr_with(Mergeable::Clean));
            assert_eq!(state.view, view, "{view:?} must be untouched");
        }
    }
}

#[cfg(test)]
mod pr_fixture_tests {
    use super::reason_text;
    use kagi_domain::github::{PrReason, PullRequest};

    pub(super) fn pr(number: u64, head: &str, base: &str) -> PullRequest {
        PullRequest {
            number,
            head: head.into(),
            base: base.into(),
            cross_repository: false,
            base_repo: "o/r".into(),
            ..Default::default()
        }
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
