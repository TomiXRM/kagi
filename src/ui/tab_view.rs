//! Per-tab pure read model — [`TabViewState`], its builder [`build_tab_view`],
//! and the UI-side accessors over the session-owned store ([`KagiApp::view`],
//! [`KagiApp::view_mut`], [`KagiApp::publish_tab_view`]).
//!
//! #482 stage 2 / ADR-0183: the value itself is owned by
//! [`crate::app::Reads`], keyed by the `SessionId` of the tab that owns the
//! worktree — there is no `active_view` field and no `tab_cache`. Adding a field
//! to per-tab data still needs exactly **2 places**: the `TabViewState` struct
//! and `build_tab_view`.

use std::collections::HashMap;

use gpui::SharedString;

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
    /// Issue #286: drop every UI state keyed by commit-row index that a graph
    /// renumber (tab switch, external reload, solo toggle) would otherwise leave
    /// pointing at the wrong commit. `diff_caches` keys `changed_files` /
    /// `diffstat` / `file_content` / `*_inflight` by row index; `main_diff` /
    /// `compare_view` render the previously-selected row's diff; `commit_menu` /
    /// `inspector_file_menu` are row/file-index context menus. `selected` is NOT
    /// touched — callers re-resolve it by CommitId. (No `cx`; pure field reset.)
    pub fn invalidate_caches_for_row_renumber(&mut self) {
        self.diff_caches.clear();
        self.main_diff = None;
        self.compare_view = None;
        self.commit_menu = None;
        self.inspector_file_menu = None;
    }

    /// The read model on screen. The empty one on the Welcome screen, and while
    /// a tab's first read is still in flight (`loading_tab` is what says so).
    pub fn view(&self) -> &TabViewState {
        self.reads.get(self.active_session())
    }

    /// In-place update of the read model on screen — a status-only WIP refresh,
    /// a solo toggle, the branch-cleanup rows, the squash ghost edges. Not a new
    /// read: it mutates the owner's existing allocation rather than rebuilding
    /// it, which is why a staging keystroke no longer copies every commit row.
    pub fn view_mut(&mut self) -> &mut TabViewState {
        let session = self.active_session();
        self.reads.get_mut(session)
    }

    /// #482 stage 2: open the bootstrap tab for a launch that has no `Context`
    /// yet (CLI argument, offscreen E2E mount). Attaches the session, pushes the
    /// tab, then publishes the read it was built from — the same order every
    /// other open follows, so the read model never exists without an owner.
    pub(crate) fn open_initial_tab(
        &mut self,
        path: &std::path::Path,
        name: &str,
        is_worktree: bool,
        view: TabViewState,
    ) {
        let session = self.app_sessions.attach(path.to_path_buf());
        self.tabs.push(super::tabs::RepoTab {
            session,
            path: path.to_path_buf(),
            name: name.to_string(),
            remote: None,
            is_worktree,
            wt_color_idx: None,
        });
        self.active_tab = self.tabs.len() - 1;
        self.publish_tab_view(session, view);
    }

    /// Publish a freshly-built read model for `session` (bootstrap, remote
    /// snapshot, load-more) — supersedes anything in flight for that owner.
    pub fn publish_tab_view(&mut self, session: crate::app::SessionId, view: TabViewState) {
        self.reads.publish(session, view);
        self.on_view_published(session);
    }

    /// Publish the completion of a read that was started with
    /// [`crate::app::Reads::begin`]. `false` = superseded: nothing was written
    /// and the caller must drop the result without any display side effect.
    pub fn accept_tab_view(&mut self, key: crate::app::ReadKey, view: TabViewState) -> bool {
        if !self.reads.accept(key, view) {
            return false;
        }
        self.on_view_published(key.session());
        true
    }

    /// The UI's reaction to a *different* read model being on screen — a new one
    /// published for the active owner, or a tab switch to another owner. Not
    /// called when a background tab's read lands: that owner's data is stored,
    /// but the active tab's diff panes and caches must not be touched.
    pub(crate) fn on_view_published(&mut self, session: crate::app::SessionId) {
        if self.active_session() != Some(session) {
            return;
        }
        // T-PERF-RENDER-002: a fresh view may change branches/tags/stashes/
        // worktrees, so invalidate the sidebar-rows cache fingerprint.
        self.view_epoch = self.view_epoch.wrapping_add(1);
        // The background scans write into the read model (cleanup rows, squash
        // ghost edges) and a fresh view has neither — `build_tab_view` copies
        // `snap.cleanup_rows`, which the snapshot always leaves empty. Every
        // apply therefore erases whatever the last scan produced. Flagging it
        // here rather than at the call sites means no future apply site can
        // forget; `render` re-arms the scans on the next frame. (This method
        // has no `cx`, and one of its callers runs before a `cx` exists.)
        self.scans_stale = true;

        // Issue #286: `diff_caches` (and the commit inspector's changed-file
        // menu) are keyed by COMMIT ROW INDEX. A fresh view renumbers rows, so a
        // stale entry would show one commit's changed-file list under another
        // commit's row (and a right-click Discard/menu would hit the wrong file).
        // Centralize the invalidation here — the same reason `view_epoch` /
        // `scans_stale` live here — so no apply site can forget it. Callers still
        // re-resolve `selected` by CommitId (there is no `cx` here).
        self.invalidate_caches_for_row_renumber();

        // Tie a worktree tab's colour to its WIP-row colour: the WIP row uses
        // lane_color(rank-in-worktrees-list), so record the same rank on the tab.
        let wt_idx = self.view().worktrees.iter().position(|w| w.is_current);
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if tab.is_worktree {
                tab.wt_color_idx = wt_idx;
            }
        }
    }
}
