//! W6-TABSPEED: per-tab pure view data — [`TabViewState`], its builder
//! [`build_tab_view`] and the main-thread apply ([`KagiApp::apply_tab_view`]).
//! (ADR-0121 Phase A: behaviour-preserving relocation out of `mod.rs`; keeping
//! the struct and its builder in one file makes the ADR-0075 P2 "2 places"
//! rule — `TabViewState` field + `build_tab_view` — a single-file edit.)

use std::collections::HashMap;

use gpui::SharedString;

use kagi_git::{CommitId, Head, RemoteBranch, RepoSnapshot, Stash, Tag, UpstreamInfo, Worktree};

use super::commit_list::{self, CommitRow};
use super::detail_panel::{build_commit_details, CommitDetail};
use super::{BranchSolo, KagiApp, StatusBarSummary, ToolbarState};

/// W6-TABSPEED: snapshot-derived **pure data** for one repository tab.
///
/// This is the entire set of per-repo display fields that
/// [`KagiApp::from_snapshot`] computes from a [`RepoSnapshot`].  It contains
/// only owned, `Send` data (`SharedString`, `Vec`, `HashMap`, plain values) —
/// no `Entity`, `FocusHandle`, or `UniformListScrollHandle` — so it can be
/// built on a background thread (`cx.background_spawn`) and cached across tabs
/// (`tab_cache`).  [`build_tab_view`] is the pure, `Send` builder;
/// [`KagiApp::apply_tab_view`] does the main-thread assignment only.
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
    pub branch_solo: Option<BranchSolo>,
    /// Commit-activity aggregation for the bottom-panel "Activity" chart.
    pub activity: kagi_domain::activity::ActivityData,
    /// Branch Cleanup table rows (ADR-0128) — classified merged/stale branch
    /// candidates, straight from the snapshot; drives the sidebar badge and
    /// the cleanup pane.
    pub cleanup_rows: Vec<kagi_domain::branch_cleanup::BranchCleanupRow>,
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

    let (rows, stash_graph_rows, stash_graph_lanes) =
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

    // ADR-0128: Branch Cleanup contract line — class counts for the headless
    // harness (full = safely deletable, squash? = [gone] heuristic, warn =
    // merged-then-grown, stale = old-tip rows regardless of class).
    {
        use kagi_domain::branch_cleanup::MergedBranchStatus as S;
        let full = snap
            .cleanup_rows
            .iter()
            .filter(|r| r.status == S::FullyMerged)
            .count();
        let squash = snap
            .cleanup_rows
            .iter()
            .filter(|r| r.status == S::SquashMergedLikely)
            .count();
        let warn = snap
            .cleanup_rows
            .iter()
            .filter(|r| matches!(r.status, S::MergedThenGrown { .. }))
            .count();
        let stale = snap.cleanup_rows.iter().filter(|r| r.stale).count();
        klog!(
            "merged-branches: {} full, {} squash?, {} warn, {} stale",
            full,
            squash,
            warn,
            stale
        );
    }

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
        branch_solo: None,
        activity: kagi_domain::activity::aggregate(&snap.commits, now_unix_secs()),
        cleanup_rows: snap.cleanup_rows.clone(),
        head_oid: match &snap.head {
            Head::Attached { target, .. } | Head::Detached { target } => Some(target.clone()),
            Head::Unborn { .. } => None,
        },
    }
}

/// Wall-clock now in Unix epoch seconds (right edge of the Activity windows).
fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl KagiApp {
    /// W6-TABSPEED: assign a [`TabViewState`] into `self` (main thread, no I/O).
    ///
    /// This is pure field assignment — the snapshot read + `build_tab_view`
    /// happens elsewhere (inline in `reload`, or on a background thread for
    /// async tab switches).  It deliberately does *not* touch transient UI
    /// state (selection / modals / panels); callers reset those as needed.
    pub fn apply_tab_view(&mut self, view: TabViewState) {
        // ADR-0075 P2: the active tab's view data is a single `TabViewState`, so
        // applying a freshly-built (or cached) view is one move — there is no
        // field-by-field copy to keep in sync when `TabViewState` gains a field.
        self.active_view = view;
        // T-PERF-RENDER-002: a fresh view may change branches/tags/stashes/
        // worktrees, so invalidate the sidebar-rows cache fingerprint.
        self.view_epoch = self.view_epoch.wrapping_add(1);

        // Tie a worktree tab's colour to its WIP-row colour: the WIP row uses
        // lane_color(rank-in-worktrees-list), so record the same rank on the tab.
        let wt_idx = self.active_view.worktrees.iter().position(|w| w.is_current);
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if tab.is_worktree {
                tab.wt_color_idx = wt_idx;
            }
        }
    }
}
