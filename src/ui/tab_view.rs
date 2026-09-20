//! Per-tab pure read model — [`TabViewState`], its builder [`build_tab_view`],
//! and the UI-side accessors over the session-owned store ([`KagiApp::view`],
//! [`KagiApp::view_mut`], [`KagiApp::publish_tab_view`]).
//!
//! #482 stage 2 / ADR-0183: the value itself is owned by
//! [`crate::app::Reads`], keyed by the `SessionId` of the tab that owns the
//! worktree — there is no `active_view` field and no `tab_cache`. Adding a field
//! to per-tab data still needs exactly **2 places**: the `TabViewState` struct
//! and `build_tab_view`.
//!
//! Alongside it, #643 Wave 4 S1 (ADR-0197) adds the other half of "per tab":
//! [`TabUiState`], the session's **presentation intent**, with the same
//! `SessionId` key and the same accessor shape ([`KagiApp::ui`] /
//! [`KagiApp::ui_mut`]). Attach ([`KagiApp::attach_session`],
//! [`KagiApp::reattach_session`]) and detach ([`KagiApp::release_session`]) are
//! the only places either store gains or loses a key.

use std::collections::{HashMap, HashSet};

use gpui::{Entity, SharedString, UniformListScrollHandle};

use kagi_git::{CommitId, Head, RemoteBranch, RepoSnapshot, Stash, Tag, UpstreamInfo, Worktree};

use super::commit_list::{self, CommitRow};
use super::detail_panel::{build_commit_details, CommitDetail};
use super::graph_wip::WipTarget;
use super::{BranchSolo, KagiApp, StatusBarSummary, ToolbarState};

/// W6-TABSPEED: snapshot-derived **pure data** for one repository tab.
///
/// This is the entire set of per-repo display fields that
/// [`KagiApp::from_snapshot`] computes from a [`RepoSnapshot`].  It contains
/// only owned, `Send` data (`SharedString`, `Vec`, `HashMap`, plain values) —
/// no `Entity`, `FocusHandle`, or `UniformListScrollHandle` — so it can be
/// built on a background thread (`cx.background_spawn`) and held by the
/// application layer's [`crate::app::Reads`] store. `SharedString` is an
/// `Arc<str>` newtype and stays: it is what the row/detail builders already
/// produce, and `src/app` never names this type (`Reads<V>` is generic), so the
/// no-gpui-in-app rule holds structurally rather than by convention.
/// [`build_tab_view`] is the pure, `Send` builder.
#[derive(Clone, Default)]
pub struct TabViewState {
    pub header: SharedString,
    pub rows: Vec<CommitRow>,
    pub stash_graph_rows: Vec<commit_list::StashRow>,
    pub stash_graph_lanes: Vec<usize>,
    pub details: Vec<CommitDetail>,
    pub branches: Vec<(String, bool)>,
    pub stashes: Vec<Stash>,
    pub is_dirty: bool,
    pub branch_targets: HashMap<String, CommitId>,
    pub commit_row_index: HashMap<CommitId, usize>,
    pub status_summary: StatusBarSummary,
    pub toolbar_state: ToolbarState,
    pub remote_branches: Vec<RemoteBranch>,
    pub tags: Vec<Tag>,
    pub branch_upstream_info: HashMap<String, UpstreamInfo>,
    pub worktrees: Vec<Worktree>,
    /// #472: the lane each WIP row's dashed HEAD connector took, **keyed by
    /// [`WipTarget`]** — `render_body` looks up its own row's key rather than
    /// its position (#476 slice 2: a worktree that just committed drops out of
    /// the row list, and by position every row below it would shift lane and
    /// colour). `None` = no connector drawn.
    pub wip_lanes: Vec<(WipTarget, Option<usize>)>,
    pub branch_solo: Option<BranchSolo>,
    /// Commit-activity aggregation for the bottom-panel "Activity" chart.
    pub activity: kagi_domain::activity::ActivityData,
    /// Branch Cleanup table rows (ADR-0128) — classified merged/stale branch
    /// candidates, straight from the snapshot; drives the sidebar badge and
    /// the cleanup pane.
    pub cleanup_rows: Vec<kagi_domain::branch_cleanup::BranchCleanupRow>,
    /// The merge / rebase / cherry-pick / revert / stash-apply this worktree
    /// is in the middle of, straight from the snapshot (#704 / ADR-0196).
    ///
    /// The session owns this observation, so the header operation strip and
    /// the availability of Abort come from the first accepted read of the tab
    /// — not from whether a `ConflictView` entity happens to exist. It stays
    /// present after the last conflict is resolved, which is the state the
    /// user could not escape.
    pub operation: Option<kagi_domain::conflict_family::ObservedOperation>,
    /// HEAD commit OID (hex) for this snapshot, or `None` for an unborn HEAD.
    /// Used to decide whether HEAD-versioned overlays (Analyze, File History)
    /// are stale on a reload — an auto-fetch that only moves remote-tracking
    /// refs leaves this unchanged, so those overlays are kept as-is.
    pub head_oid: Option<String>,
}

/// W6-TABSPEED: build the pure [`TabViewState`] from a snapshot.
///
/// This is the exact computation (and the exact `eprintln!` log lines) that
/// used to live inline in `from_snapshot`.  It is a free function so it can be
/// called from a background thread — `RepoSnapshot` is `Send`, the result is
/// `Send`, and nothing here touches gpui state.
pub fn build_tab_view(snap: &RepoSnapshot, repo_name: &str) -> TabViewState {
    let head_label = match &snap.head {
        Head::Attached { branch, .. } => format!("branch: {branch}"),
        Head::Detached { target } => format!("detached: {}", target.get(..8).unwrap_or(target)),
        Head::Unborn { branch } => format!("unborn ({branch})"),
    };

    let status = &snap.status;
    let status_label = if status.is_dirty() {
        let parts: Vec<String> = [
            (!status.staged.is_empty()).then(|| format!("{}S", status.staged.len())),
            (!status.unstaged.is_empty()).then(|| format!("{}M", status.unstaged.len())),
            (!status.untracked.is_empty()).then(|| format!("{}?", status.untracked.len())),
            (!status.conflicted.is_empty()).then(|| format!("{}!", status.conflicted.len())),
        ]
        .into_iter()
        .flatten()
        .collect();
        format!(" [{}]", parts.join(" "))
    } else {
        " [clean]".to_string()
    };

    let header = SharedString::from(format!(
        "{repo_name}  ·  {head_label}{status_label}  ·  {} commits",
        snap.commits.len()
    ));

    let (mut rows, stash_graph_rows, stash_graph_lanes) =
        commit_list::build_commit_rows_with_stashes(snap);
    let details = build_commit_details(snap);

    // T009: log lane count derived from the first row (all rows share the same value).
    let lane_count = rows.first().map(|r| r.lane_count).unwrap_or(0);
    klog!("graph: lane_count={}", lane_count);
    klog!("commit list rows: {}", rows.len());
    // Model A+: one WIP row is drawn per dirty worktree. Report the totals so the
    // headless harness can assert multi-worktree WIP rendering.
    let dirty_worktrees = snap
        .worktrees
        .iter()
        .filter(|w| w.wip.is_some_and(|s| s.is_dirty()))
        .count();
    klog!(
        "worktrees: {} total, {} dirty",
        snap.worktrees.len(),
        dirty_worktrees
    );
    eprintln!(
        "[kagi] graph: stash rows={} lanes={:?}",
        stash_graph_rows.len(),
        stash_graph_lanes
    );

    // Build branch list: (name, is_head).
    let head_branch = match &snap.head {
        Head::Attached { branch, .. } => Some(branch.clone()),
        _ => None,
    };
    let branches: Vec<(String, bool)> = snap
        .branches
        .iter()
        .map(|b| {
            let is_head = head_branch.as_deref() == Some(&b.name);
            (b.name.clone(), is_head)
        })
        .collect();

    let is_dirty = snap.status.is_dirty();
    let stashes = snap.stashes.clone();

    // T028: build branch_targets (branch name → CommitId) from the snapshot.
    let branch_targets: HashMap<String, CommitId> = snap
        .branches
        .iter()
        .map(|b| (b.name.clone(), b.target.clone()))
        .collect();

    // T028: build commit_row_index (CommitId → row index in rows/commits).
    // snap.commits is the authoritative ordering; rows is built from it 1-to-1.
    let commit_row_index: HashMap<CommitId, usize> = snap
        .commits
        .iter()
        .enumerate()
        .map(|(i, c)| (c.id.clone(), i))
        .collect();

    // #472: post-pass over the built rows — one dashed connector per WIP row,
    // down to that worktree's HEAD. Same stage as the squash ghost connectors,
    // just synchronous: everything it needs is already in the snapshot.
    let wip_lanes = super::graph_wip::inject_wip_edges(
        &mut rows,
        &super::graph_wip::wip_targets(snap),
        &commit_row_index,
    );

    // W2-SIDEBAR: collect remote branches and tags.
    let remote_branches = snap.remote_branches.clone();
    let tags = snap.tags.clone();

    // W2-SIDEBAR: build upstream info map (branch name → UpstreamInfo).
    let branch_upstream_info: HashMap<String, UpstreamInfo> = snap
        .branches
        .iter()
        .filter_map(|b| b.upstream.as_ref().map(|u| (b.name.clone(), u.clone())))
        .collect();

    // W2-SIDEBAR: emit sidebar log line.
    eprintln!(
        "[kagi] sidebar: local={} remote={} tags={} stashes={} worktrees={} filter=\"\"",
        snap.branches.len(),
        snap.remote_branches.len(),
        snap.tags.len(),
        snap.stashes.len(),
        snap.worktrees.len()
    );

    // ADR-0128 follow-up: the `merged-branches: ...` contract line moved to
    // `KagiApp::start_branch_cleanup_scan` (src/ui/branch_cleanup.rs) — it's
    // emitted when the background scan actually completes, since `snapshot()`
    // no longer computes `cleanup_rows` synchronously (see snapshot.rs).

    // T-BP-003: build StatusBarSummary and emit the headless log.
    let mut status_summary = StatusBarSummary::from_snapshot(snap);
    // T-HT-001: fill repo_name for toolbar display.
    status_summary.repo_name = repo_name.to_string();
    status_summary.log_headless();

    // T-HT-001: derive toolbar state and emit headless log.
    let toolbar_state = status_summary.toolbar_state();
    toolbar_state.log_headless();

    TabViewState {
        header,
        rows,
        stash_graph_rows,
        stash_graph_lanes,
        details,
        branches,
        stashes,
        is_dirty,
        branch_targets,
        commit_row_index,
        status_summary,
        toolbar_state,
        remote_branches,
        tags,
        branch_upstream_info,
        worktrees: snap.worktrees.clone(),
        wip_lanes,
        branch_solo: None,
        activity: kagi_domain::activity::aggregate(&snap.commits, now_unix_secs()),
        cleanup_rows: snap.cleanup_rows.clone(),
        operation: snap.operation.clone(),
        head_oid: match &snap.head {
            Head::Attached { target, .. } | Head::Detached { target } => Some(target.clone()),
            Head::Unborn { .. } => None,
        },
    }
}

/// #643 Wave 4 S1 (ADR-0197): one session's **presentation intent** — what the
/// user pointed at in this tab, as opposed to what the repository said (that is
/// [`TabViewState`]).
///
/// Owned by [`KagiApp::ui`], keyed by `SessionId`, and reachable only through
/// [`KagiApp::ui`] / [`KagiApp::ui_mut`]. Two open tabs therefore hold two
/// independent values and a switch cannot leak one into the other — which is
/// why a tab switch has nothing to clear.
///
/// It deliberately owns **no** operation lifecycle: no lease, no reconcile
/// entry, no planning slot, no in-flight write. Closing a tab drops this value,
/// and closing a tab is not cancelling an execution (ADR-0182 / ADR-0197 決定 1).
///
/// It is kept as a second map rather than a field of [`TabViewState`] because a
/// read is published as a whole value: folding intent into it would make a
/// background read's landing drag the selection back to where it was when that
/// read started (ADR-0197 決定 2).
///
/// S1 owns selection; S2b adds disposable evidence and cache data. S3b adds
/// disposable positioning handles after classifying `UniformListScrollHandle`
/// as plain `Rc<RefCell<...>>` state with no task, subscription, entity, or
/// callback lifecycle. S4 adds recomputable read caches and the pure undo/redo
/// cursor. S5 makes this the owner of every repository-bound pane and session
/// resource (ADR-0197).
pub struct TabUiState {
    /// Currently selected commit row index (`None` = no selection).
    pub selected: Option<usize>,
    /// Main commit-list position and the graph walk limit that produced it.
    pub commit_scroll_handle: UniformListScrollHandle,
    pub commit_limit: usize,
    /// Horizontal graph viewport position.
    pub graph_scroll_x: f32,
    /// Sidebar branch/PR groups collapsed by this tab.
    pub branch_groups_collapsed: HashSet<String>,
    /// Branch Cleanup list position and checked branch names.
    pub cleanup_scroll: UniformListScrollHandle,
    pub cleanup_selected: HashSet<String>,
    /// ADR-0128 Branch Cleanup takeover and GitHub Phase 1c PR mode are this
    /// tab's workspace mode; the PR tabs they hold name this repository's PRs.
    pub branch_cleanup_open: bool,
    pub pr_mode: Option<super::pr_mode::PrModeState>,
    /// Right-click menu on one of this tab's PR rows: `Some((pr, cursor))`.
    pub pr_menu: Option<(kagi_domain::github::PullRequest, gpui::Point<gpui::Pixels>)>,
    /// Smart Commit generation status belongs to the session whose panel
    /// initiated it; background completion never writes through the active tab.
    pub smart_commit_generating: bool,
    pub smart_commit_status: Option<String>,
    /// Identifies the published model, independently of read-request revisions.
    pub view_publish_gen: u64,
    /// Invalidates async cache writers on activation and row renumbering.
    pub cache_epoch: u64,
    /// Recomputable diff data. Contains only owned collections and `Arc<FileDiff>`.
    pub diff_caches: super::diff_cache::DiffCaches,
    /// Aggregated staged + unstaged additions/deletions for the synthetic WIP row.
    pub wip_diffstat: Option<super::WipDiffStat>,
    /// Watcher baseline; absent until this activation observes the worktree.
    pub last_working_status: Option<kagi_git::WorkingTreeStatus>,
    /// Session-local undo/redo cursor. Backend plan and preflight reject moved refs.
    pub operation_history: kagi_git::OperationHistory,
    /// Reflog seeding is attempted once per attached session.
    pub history_seed_attempted: bool,
    /// Open-PR evidence belongs to this session, including failures and absence.
    pub github_prs: Vec<kagi_domain::github::PullRequest>,
    pub github_prs_loaded: bool,
    pub github_error: Option<String>,
    pub github_unavailable: bool,
    pub github_prs_epoch: u64,
    pub github_prs_gen: u64,
    pub github_prs_loading: bool,
    pub(super) pr_details: super::github_pr_detail::PrDetailController,
    /// Read-only Issues workspace data. Requests are session-owned and each
    /// generation accepts only its newest completion.
    pub github_issues: Vec<kagi_domain::github::Issue>,
    pub github_issues_loaded: bool,
    pub github_issues_loading: bool,
    pub github_issues_error: Option<String>,
    pub github_issues_gen: u64,
    pub github_issue_mentions: Vec<u64>,
    pub github_issue_tab: kagi_domain::github::IssueListTab,
    pub selected_github_issue: Option<u64>,
    pub github_issue_details: HashMap<u64, kagi_domain::github::Issue>,
    pub github_issue_detail_loading: Option<u64>,
    pub github_issue_detail_error: Option<String>,
    pub github_issue_detail_gen: u64,
    pub(super) issue_composer: super::issues_composer::IssuesComposerState,
    /// Scan revisions reject superseded completions without consulting the active tab.
    pub cleanup_gen: u64,
    pub cleanup_scanning: bool,
    pub cleanup_prs: Vec<kagi_domain::github::PullRequest>,
    pub cleanup_prs_stale: bool,
    /// Re-armed when retained conflict authority is invalidated or its read changes.
    pub conflict_detected: bool,
    /// Where the retained panes are in being re-checked against the read. Not
    /// [`PaneRevalidation::Settled`] = on screen but **not authoritative**: the
    /// entities (undo stacks, selection, scroll) stay, every mutation they
    /// offer is refused (ADR-0197 決定 3 / #722).
    pub pane_revalidation: super::tab_ui_state_ops::PaneRevalidation,
    /// Recomputable Analyze evidence; the pane entity is owned below.
    pub ecosystem_cache: Option<super::ecosystem::CachedMine>,
    pub ecosystem_inflight: bool,
    pub ecosystem_gen: u64,
    pub ecosystem_mine_head: Option<String>,
    /// Repository-bound resources. They survive activation changes and are
    /// destroyed together when `release_session` removes this owner.
    pub repo_session: Option<kagi_git::session::RepoSession>,
    pub terminal_session: Option<super::terminal::KagiTerminalSession>,
    pub conflict: Option<Entity<super::conflict_view::ConflictView>>,
    pub conflict_merge_pending: bool,
    pub file_history: Option<Entity<super::file_history::FileHistoryView>>,
    pub file_history_head: Option<String>,
    pub ecosystem: Option<Entity<super::ecosystem::EcosystemView>>,
    pub editor_workspace: Option<Entity<super::editor_workspace::EditorWorkspaceView>>,
    pub commit_panel: Option<Entity<super::commit_panel::CommitPanelView>>,
    pub commit_panel_open: bool,
    pub main_diff: Option<Entity<super::MainDiffPane>>,
    pub compare_view: Option<Entity<super::ComparePane>>,
}

impl Default for TabUiState {
    fn default() -> Self {
        Self {
            selected: None,
            commit_scroll_handle: UniformListScrollHandle::new(),
            commit_limit: super::DEFAULT_COMMIT_LIMIT,
            graph_scroll_x: 0.0,
            branch_groups_collapsed: HashSet::from([super::sidebar::PR_GROUP_OTHERS.to_string()]),
            cleanup_scroll: UniformListScrollHandle::new(),
            smart_commit_generating: false,
            smart_commit_status: None,
            cleanup_selected: HashSet::new(),
            branch_cleanup_open: false,
            pr_mode: None,
            pr_menu: None,
            view_publish_gen: 0,
            cache_epoch: 0,
            diff_caches: super::diff_cache::DiffCaches::default(),
            wip_diffstat: None,
            last_working_status: None,
            operation_history: kagi_git::OperationHistory::new(),
            history_seed_attempted: false,
            github_prs: Vec::new(),
            github_prs_loaded: false,
            github_error: None,
            github_unavailable: false,
            github_prs_epoch: 0,
            github_prs_gen: 0,
            github_prs_loading: false,
            pr_details: Default::default(),
            github_issues: Vec::new(),
            github_issues_loaded: false,
            github_issues_loading: false,
            github_issues_error: None,
            github_issues_gen: 0,
            github_issue_mentions: Vec::new(),
            github_issue_tab: Default::default(),
            selected_github_issue: None,
            github_issue_details: HashMap::new(),
            github_issue_detail_loading: None,
            github_issue_detail_error: None,
            github_issue_detail_gen: 0,
            issue_composer: Default::default(),
            cleanup_gen: 0,
            cleanup_scanning: false,
            cleanup_prs: Vec::new(),
            cleanup_prs_stale: false,
            conflict_detected: false,
            pane_revalidation: Default::default(),
            ecosystem_cache: None,
            ecosystem_inflight: false,
            ecosystem_gen: 0,
            ecosystem_mine_head: None,
            repo_session: None,
            terminal_session: None,
            conflict: None,
            conflict_merge_pending: false,
            file_history: None,
            file_history_head: None,
            ecosystem: None,
            editor_workspace: None,
            commit_panel: None,
            commit_panel_open: false,
            main_diff: None,
            compare_view: None,
        }
    }
}

impl TabUiState {
    pub(super) fn begin_github_prs_request(&mut self) -> u64 {
        self.github_prs_gen = self.github_prs_gen.wrapping_add(1);
        self.github_prs_loading = true;
        self.github_prs_gen
    }

    pub(super) fn accept_github_prs_completion(&mut self, generation: u64) -> bool {
        if generation != self.github_prs_gen {
            return false;
        }
        self.github_prs_loading = false;
        true
    }

    /// Begin a list request and return the generation frozen into its task.
    pub(super) fn begin_github_issues_request(&mut self) -> u64 {
        self.github_issues_gen = self.github_issues_gen.wrapping_add(1);
        self.github_issues_loading = true;
        self.github_issues_error = None;
        if self.issue_composer.base_repo.is_none() {
            self.issue_composer.repo_loading = true;
            self.issue_composer.repo_error = None;
        }
        self.github_issues_gen
    }

    /// Settle a list request only when no later request superseded it.
    ///
    /// Failure records its reason but deliberately retains the last successful
    /// list. `Ok(vec![])` is the only empty answer that replaces it.
    pub(super) fn finish_github_issues_request(
        &mut self,
        generation: u64,
        result: Result<kagi_domain::github::IssueListSnapshot, kagi_git::github::PrFetchError>,
    ) -> bool {
        if generation != self.github_issues_gen {
            return false;
        }
        self.github_issues_loading = false;
        match result {
            Ok(snapshot) => {
                self.github_issues = snapshot.issues;
                self.github_issue_mentions = snapshot.mentioned_numbers;
                self.issue_composer.base_repo = Some(snapshot.base_repo);
                self.issue_composer.repo_loading = false;
                self.issue_composer.repo_error = None;
                self.github_issues_loaded = true;
                self.github_issues_error = None;
            }
            Err(error) => {
                let error = error.to_string();
                self.github_issues_error = Some(error.clone());
                self.issue_composer.repo_loading = false;
                if self.issue_composer.base_repo.is_none() {
                    self.issue_composer.repo_error = Some(error);
                }
            }
        }
        true
    }

    /// Select an issue and begin a detail request for it.
    pub(super) fn begin_github_issue_detail_request(&mut self, number: u64) -> u64 {
        self.selected_github_issue = Some(number);
        self.github_issue_detail_gen = self.github_issue_detail_gen.wrapping_add(1);
        self.github_issue_detail_loading = Some(number);
        self.github_issue_detail_error = None;
        self.github_issue_detail_gen
    }

    /// Return to the Composer-only home without discarding a cached Thread or
    /// Reply draft. Advancing the generation prevents an in-flight detail read
    /// from reviving the selection after the user left it.
    pub(super) fn clear_github_issue_selection(&mut self) {
        self.github_issue_detail_gen = self.github_issue_detail_gen.wrapping_add(1);
        self.selected_github_issue = None;
        self.github_issue_detail_loading = None;
        self.github_issue_detail_error = None;
        for editor in self.issue_composer.editors.values_mut() {
            editor.focused = false;
        }
    }

    /// Settle only the newest detail request. A failure leaves any successfully
    /// cached detail intact, including the cached value for the same number.
    pub(super) fn finish_github_issue_detail_request(
        &mut self,
        generation: u64,
        number: u64,
        result: Result<kagi_domain::github::Issue, kagi_git::github::PrFetchError>,
    ) -> bool {
        if generation != self.github_issue_detail_gen
            || self.github_issue_detail_loading != Some(number)
        {
            return false;
        }
        self.github_issue_detail_loading = None;
        match result {
            Ok(issue) => {
                self.github_issue_details.insert(number, issue);
                self.github_issue_detail_error = None;
            }
            Err(error) => self.github_issue_detail_error = Some(error.to_string()),
        }
        true
    }
}

#[cfg(test)]
mod github_issue_state_tests {
    use super::*;
    use kagi_domain::github::{Issue, IssueState};
    use kagi_git::github::PrFetchError;

    fn issue(number: u64, title: &str) -> Issue {
        Issue {
            number,
            title: title.into(),
            state: IssueState::Open,
            url: format!("https://github.com/acme/widgets/issues/{number}"),
            author: "alice".into(),
            assignees: Vec::new(),
            labels: Vec::new(),
            body: String::new(),
            comments: Vec::new(),
            comment_count: 0,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn empty_success_is_loaded_but_failure_keeps_last_success() {
        let mut state = TabUiState::default();
        let first = state.begin_github_issues_request();
        let snapshot =
            |issues, mentioned_numbers, base_repo: &str| kagi_domain::github::IssueListSnapshot {
                issues,
                mentioned_numbers,
                base_repo: base_repo.into(),
            };
        assert!(state.finish_github_issues_request(
            first,
            Ok(snapshot(
                vec![issue(1, "kept")],
                vec![1],
                "github.com/a/one"
            ))
        ));

        let failed = state.begin_github_issues_request();
        assert!(state.finish_github_issues_request(
            failed,
            Err(PrFetchError::Auth("login required".into()))
        ));
        assert_eq!(state.github_issues[0].number, 1);
        assert!(state.github_issues_loaded);
        assert!(state
            .github_issues_error
            .as_deref()
            .is_some_and(|error| error.contains("login required")));
        assert_eq!(state.github_issue_mentions, vec![1]);
        assert_eq!(
            state.issue_composer.base_repo.as_deref(),
            Some("github.com/a/one"),
            "a failed refresh keeps the frozen write destination"
        );

        let empty = state.begin_github_issues_request();
        assert!(state.finish_github_issues_request(
            empty,
            Ok(snapshot(Vec::new(), vec![], "github.com/a/one"))
        ));
        assert!(state.github_issues.is_empty());
        assert!(state.github_issues_loaded);
        assert!(state.github_issues_error.is_none());
    }

    #[test]
    fn later_list_request_rejects_delayed_completion() {
        let mut state = TabUiState::default();
        let old = state.begin_github_issues_request();
        let new = state.begin_github_issues_request();
        assert!(state.finish_github_issues_request(
            new,
            Ok(kagi_domain::github::IssueListSnapshot {
                issues: vec![issue(2, "new")],
                mentioned_numbers: vec![2],
                base_repo: "github.com/a/new".into(),
            })
        ));
        assert!(!state.finish_github_issues_request(
            old,
            Ok(kagi_domain::github::IssueListSnapshot {
                issues: vec![issue(1, "stale")],
                mentioned_numbers: vec![1],
                base_repo: "github.com/a/stale".into(),
            })
        ));
        assert_eq!(state.github_issues[0].number, 2);
        assert_eq!(state.github_issue_mentions, vec![2]);
        assert_eq!(
            state.issue_composer.base_repo.as_deref(),
            Some("github.com/a/new")
        );
    }

    #[test]
    fn list_identity_wait_and_refresh_failure_preserve_restored_draft() {
        let mut state = TabUiState::default();
        state.issue_composer.editors.insert(
            None,
            crate::ui::issues_composer::IssueEditor {
                draft: kagi_domain::issue_composer::IssueDraft {
                    title: "restored".into(),
                    body: "keep me".into(),
                    revision: 3,
                },
                loaded: true,
                ..Default::default()
            },
        );
        let failed = state.begin_github_issues_request();
        assert!(state.issue_composer.repo_loading);
        assert!(state.issue_composer.base_repo.is_none());
        assert!(state
            .finish_github_issues_request(failed, Err(PrFetchError::Network("offline".into()))));
        assert_eq!(
            state.issue_composer.editors[&None].draft.body.as_str(),
            "keep me"
        );
        assert!(state.issue_composer.base_repo.is_none());

        let success = state.begin_github_issues_request();
        assert!(state.finish_github_issues_request(
            success,
            Ok(kagi_domain::github::IssueListSnapshot {
                issues: Vec::new(),
                mentioned_numbers: Vec::new(),
                base_repo: "github.com/a/repo".into(),
            })
        ));
        assert_eq!(
            state.issue_composer.editors[&None].draft.body.as_str(),
            "keep me",
            "accepting the frozen destination never rewrites a restored draft"
        );
        assert_eq!(
            state.issue_composer.base_repo.as_deref(),
            Some("github.com/a/repo")
        );
    }

    #[test]
    fn later_pr_list_request_rejects_delayed_completion() {
        let mut state = TabUiState::default();
        let old = state.begin_github_prs_request();
        let newest = state.begin_github_prs_request();
        assert!(!state.accept_github_prs_completion(old));
        assert!(state.github_prs_loading);
        assert!(state.accept_github_prs_completion(newest));
        assert!(!state.github_prs_loading);
    }

    #[test]
    fn owner_states_and_detail_generations_are_independent() {
        let mut owner_a = TabUiState::default();
        let mut owner_b = TabUiState::default();
        let a = owner_a.begin_github_issues_request();
        let b = owner_b.begin_github_issues_request();
        let result = |issue, repo: &str| kagi_domain::github::IssueListSnapshot {
            issues: vec![issue],
            mentioned_numbers: Vec::new(),
            base_repo: repo.into(),
        };
        assert!(owner_b.finish_github_issues_request(b, Ok(result(issue(20, "B"), "b/r"))));
        assert!(owner_a.finish_github_issues_request(a, Ok(result(issue(10, "A"), "a/r"))));
        assert_eq!(owner_a.github_issues[0].number, 10);
        assert_eq!(owner_b.github_issues[0].number, 20);

        let old = owner_a.begin_github_issue_detail_request(10);
        let newest = owner_a.begin_github_issue_detail_request(11);
        assert!(!owner_a.finish_github_issue_detail_request(old, 10, Ok(issue(10, "old"))));
        assert!(owner_a.finish_github_issue_detail_request(
            newest,
            11,
            Err(PrFetchError::NotFound("gone".into()))
        ));
        assert_eq!(owner_a.selected_github_issue, Some(11));
        assert!(owner_a.github_issue_details.is_empty());
        assert!(owner_a
            .github_issue_detail_error
            .as_deref()
            .is_some_and(|error| error.contains("gone")));

        let successful = owner_a.begin_github_issue_detail_request(11);
        assert!(owner_a.finish_github_issue_detail_request(
            successful,
            11,
            Ok(issue(11, "cached"))
        ));
        let failed = owner_a.begin_github_issue_detail_request(11);
        assert!(owner_a.finish_github_issue_detail_request(
            failed,
            11,
            Err(PrFetchError::Network("offline".into()))
        ));
        assert_eq!(
            owner_a
                .github_issue_details
                .get(&11)
                .map(|issue| issue.title.as_str()),
            Some("cached"),
            "detail failure keeps the last successful value"
        );
    }

    #[test]
    fn returning_home_invalidates_detail_without_dropping_cache_or_reply() {
        let mut state = TabUiState::default();
        let generation = state.begin_github_issue_detail_request(7);
        state.github_issue_details.insert(7, issue(7, "cached"));
        state
            .issue_composer
            .editors
            .insert(Some(7), Default::default());
        state.clear_github_issue_selection();
        assert_eq!(state.selected_github_issue, None);
        assert_eq!(state.github_issue_detail_loading, None);
        assert!(state.github_issue_detail_error.is_none());
        assert!(state.github_issue_details.contains_key(&7));
        assert!(state.issue_composer.editors.contains_key(&Some(7)));
        assert!(!state.finish_github_issue_detail_request(generation, 7, Ok(issue(7, "late"))));
        assert_eq!(state.selected_github_issue, None);
    }
}

/// Wall-clock now in Unix epoch seconds (right edge of the Activity windows).
fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
#[path = "tab_view_access.rs"]
mod tab_view_access;
