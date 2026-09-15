//! Building a PR tab from the session's current read, and re-building the
//! retained ones when the tab comes back (#724 review / ADR-0197 決定 3).
//!
//! A `PrTab` is a **snapshot**: the base and head tips it resolved, the commits
//! and files between them, and what GitHub said about the PR at that moment.
//! S6 keeps PR mode across a tab switch, so that snapshot can be arbitrarily old
//! by the time the tab is on screen again — the branches may have been fetched
//! forward, the PR reviewed, its merge status changed. Retained state is not
//! authoritative until the activation's read says so, which is what the pane
//! revalidation pass exists for.

use super::pr_mode::{PrTab, PrView};
use super::KagiApp;
use gpui::{px, Context, ListState};
use kagi_domain::github::PullRequest;

/// How many commits a PR tab lists (the PR view is not a graph).
const COMMIT_LIMIT: usize = 500;

impl KagiApp {
    /// A fresh tab for `pr`, resolved against **this session's** read and
    /// repository. `None` when either branch is missing from the read's remote
    /// branches (not fetched yet) or the session has no open repository.
    pub(crate) fn pr_tab_snapshot(&self, pr: &PullRequest) -> Option<PrTab> {
        let tip = |name: &str| {
            self.view()
                .remote_branches
                .iter()
                .find(|rb| rb.name == name)
                .map(|rb| rb.target.clone())
        };
        let (base_tip, head) = (tip(&pr.base)?, tip(&pr.head)?);
        let repo = self.ui().repo_session.as_ref()?.backend();
        let base = repo
            .merge_base(&base_tip, &head)
            .unwrap_or_else(|_| base_tip.clone());
        let commits = repo
            .commits_between(&base, &head, COMMIT_LIMIT)
            .unwrap_or_default();
        let files = repo.compare_commits(&base, &head).unwrap_or_default();
        let mut tab = PrTab {
            pr: pr.clone(),
            base,
            base_tip,
            head,
            selected_commit: None,
            // A fresh tab opens on the description — "what is this PR" first,
            // the diff once a file/commit is picked (user request).
            selected_file: (!files.is_empty()).then_some(0),
            commits,
            files,
            diff: None,
            diff_scroll: ListState::new(0, gpui::ListAlignment::Top, px(200.)),
            reviews: Vec::new(),
            comments: Vec::new(),
            line_comments: Vec::new(),
            conversation_loaded: false,
            conflicts: None,
            conflict_selected: None,
            conflict_scroll: ListState::new(0, gpui::ListAlignment::Top, px(200.)),
            conflict_preview: None,
            conflict_at: 0,
            merge_status: None,
            merge_status_loaded: false,
        };
        self.pr_tab_reload_diff(&mut tab);
        Some(tab)
    }

    /// ADR-0197 決定 3: rebuild the retained PR tabs against the read this
    /// activation accepted, then fetch their conversation / merge status again.
    ///
    /// Which PRs are open, which one is active and which body is shown are
    /// presentation intent and survive; everything derived from the refs or
    /// from GitHub is recomputed, because nothing refreshed it while the tab was
    /// away — not the activation's full read, not the PR list ticker. A tab
    /// whose branches are no longer in the read (the PR was merged and its head
    /// deleted, say) cannot be rebuilt and is closed rather than left showing a
    /// snapshot of refs that are gone.
    ///
    /// The conflict preview is part of that: it is re-derived from the rebuilt
    /// tab while this owner is on screen, which is also how a list or marker
    /// text that landed for a background owner (and could not start its own
    /// follow-up there) gets completed.
    pub(crate) fn revalidate_pr_mode(&mut self, cx: &mut Context<Self>) {
        let Some(mode) = self.pr_mode() else { return };
        if mode.tabs.is_empty() {
            return;
        }
        let active_number = mode
            .active
            .and_then(|i| mode.tabs.get(i))
            .map(|t| t.pr.number);
        let conflicts_view = mode.view == PrView::Conflicts;
        // The PR list refreshed for this activation is the fresher description
        // of each PR; a tab whose PR is no longer listed keeps the one it has.
        let prs: Vec<PullRequest> = mode
            .tabs
            .iter()
            .map(|tab| {
                self.ui()
                    .github_prs
                    .iter()
                    .find(|pr| pr.number == tab.pr.number)
                    .unwrap_or(&tab.pr)
                    .clone()
            })
            .collect();

        let tabs: Vec<PrTab> = prs
            .iter()
            .filter_map(|pr| self.pr_tab_snapshot(pr))
            .collect();
        let numbers: Vec<u64> = tabs.iter().map(|t| t.pr.number).collect();
        klog!(
            "pr-mode: revalidate tabs={} kept={}",
            prs.len(),
            numbers.len()
        );
        let Some(mode) = self.pr_mode_mut() else {
            return;
        };
        mode.active = active_number
            .and_then(|number| numbers.iter().position(|n| *n == number))
            .or_else(|| (!numbers.is_empty()).then_some(0));
        mode.tabs = tabs;

        for number in numbers {
            self.pr_mode_load_conversation(number, cx);
        }
        if conflicts_view {
            self.pr_mode_load_conflicts(cx);
        }
    }
}
