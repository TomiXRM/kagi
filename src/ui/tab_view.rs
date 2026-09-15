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

/// Wall-clock now in Unix epoch seconds (right edge of the Activity windows).
fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl KagiApp {
    /// Make retained repository-derived state non-authoritative before an
    /// activation read (ADR-0197 決定 3 / #722 P2). "Not authoritative" means
    /// **stop**, not **destroy**: recomputable caches are dropped, but every
    /// pane entity stays alive, so undo/redo, the selected file/hunk and
    /// scroll all survive a tab round trip. What the panes lose is the right
    /// to *act*: `pane_revalidation` refuses their mutations until the read
    /// lands. `revalidate_retained_panes` then compares the new observation —
    /// unchanged panes are simply re-enabled, changed ones are rebuilt there
    /// and (for conflict) by the re-armed detection below.
    pub(crate) fn begin_session_revalidation(&mut self, session: crate::app::SessionId) {
        if let Some(ui) = self.ui.get_mut(&session) {
            ui.cache_epoch = ui.cache_epoch.wrapping_add(1);
            ui.diff_caches.clear();
            ui.wip_diffstat = None;
            ui.last_working_status = None;
            ui.pane_revalidation = super::tab_ui_state_ops::PaneRevalidation::AwaitingRead;
            ui.conflict_merge_pending = false;
            // Re-arm detection: its outcome decides whether the retained
            // conflict pane is updated in place or replaced.
            ui.conflict_detected = false;
            // PR mode is dropped rather than re-checked. A `PrTab` is a snapshot
            // of the refs *and* of GitHub (reviews, merge status, conflict
            // preview) taken when it was opened, and nothing refreshes it while
            // the tab is away; rebuilding it on activation needs the PR list
            // that the activation is still fetching, races the loads started
            // before departure, and loses the user's position anyway. Carrying
            // PR mode across a switch is out of scope for #643 Wave 4 S6 — the
            // tab reopens the PR. The *ownership* stays session-scoped, so B
            // never sees A's PRs.
            ui.pr_mode = None;
        }
    }

    /// Drop row-indexed caches for `session`.
    pub fn invalidate_caches_for_row_renumber(&mut self, session: crate::app::SessionId) {
        if let Some(ui) = self.ui.get_mut(&session) {
            ui.cache_epoch = ui.cache_epoch.wrapping_add(1);
            ui.diff_caches.clear();
        }
        if self.active_session() != Some(session) {
            return;
        }
        // Main Diff / Compare are not dropped here: a published read queues
        // `revalidate_retained_panes`, which re-anchors them (#722).
        self.commit_menu = None;
        self.inspector_file_menu = None;
    }

    /// The read model on screen. The empty one on the Welcome screen, and while
    /// a tab's first read is still in flight ([`KagiApp::loading_tab`] is what
    /// says so).
    pub fn view(&self) -> &TabViewState {
        self.reads.get(self.active_session())
    }

    /// A merge whose conflicts are all resolved and which is waiting for its
    /// commit (ADR-0068's commit-panel route).
    ///
    /// Derived from the session's operation observation, never stored. As a
    /// `merge_commit_ready` flag it was set by conflict detection and cleared
    /// by the next one, so anything that dropped the conflict view — the
    /// `MergeResolvedReady` branch itself — decided the answer. #704: the
    /// repository is what knows, and it says so from the first accepted read.
    pub fn merge_commit_ready(&self) -> bool {
        self.view().operation.as_ref().is_some_and(|op| {
            op.kind()
                == kagi_domain::conflict_family::ConflictOperationKind::Repository(
                    kagi_domain::plan_note::InProgressOp::Merge,
                )
                && op.unmerged() == 0
        })
    }

    /// Read the active session's presentation state. Welcome rendering receives
    /// an immutable default; no writer can put a resource into that value.
    pub fn ui(&self) -> &TabUiState {
        self.active_session()
            .and_then(|session| self.ui.get(&session))
            .unwrap_or(&self.ui_default)
    }

    /// The active session's writer, or `None` when no session owns the screen —
    /// the Welcome screen, a failed repository open, or the gap between the last
    /// tab closing and the next opening. Resource-bearing state must never use a
    /// detached sink, so mutation is *rejected* by returning `None` rather than
    /// crashing (ADR-0197 決定 2): every caller writes only inside `if let Some`.
    /// Background completion must instead use its frozen owner with
    /// `ui.get_mut`, never this foreground accessor.
    pub fn ui_mut(&mut self) -> Option<&mut TabUiState> {
        let session = self.active_session()?;
        self.ui.get_mut(&session)
    }

    /// Apply `f` to the active session's state, or do nothing when no session
    /// owns the screen. The one-line form of [`KagiApp::ui_mut`]'s `Option` for
    /// a plain field write whose only no-owner behaviour is "don't write"
    /// (#722 P1). Anything that must also *act* on the no-owner case keeps the
    /// explicit `match` / `let else` on `ui_mut` instead.
    /// `R` is discarded, so a one-expression writer whose call returns a value
    /// (`HashMap::insert`) still needs no block.
    pub(crate) fn with_ui<R>(&mut self, f: impl FnOnce(&mut TabUiState) -> R) {
        if let Some(ui) = self.ui_mut() {
            f(ui);
        }
    }

    /// What "Branch from here" acts on: the selected commit, or HEAD when
    /// nothing is selected. The header button and the `branch.new` command had
    /// the same chain spelled out twice; they now ask the session once.
    pub fn selected_or_head_commit(&self) -> Option<CommitId> {
        let details = &self.view().details;
        self.ui()
            .selected
            .and_then(|row| details.get(row))
            .or_else(|| details.first())
            .map(|detail| CommitId(detail.full_sha.to_string()))
    }

    /// Open a display slot for `path` and give it its UI state — the attach half
    /// of ADR-0197 決定 2's single lifecycle seam.
    ///
    /// `Sessions::attach` unifies aliases, so it can hand back a session a tab
    /// already holds. `entry` is what keeps that case from clearing the existing
    /// tab's selection: an attach that resolves to a live session initializes
    /// nothing.
    pub fn attach_session(&mut self, path: std::path::PathBuf) -> crate::app::SessionId {
        let session = self.app_sessions.attach(path);
        self.ui.entry(session).or_default();
        session
    }

    /// Same tab slot, fresh incarnation (a remote re-snapshot). The old
    /// incarnation expires exactly as it would on close — including its UI
    /// state, which is why this goes through [`KagiApp::release_session`]
    /// instead of forgetting the read on its own.
    pub(crate) fn reattach_session(
        &mut self,
        session: crate::app::SessionId,
        path: std::path::PathBuf,
    ) -> crate::app::SessionId {
        self.release_session(session);
        let next = self.app_sessions.reattach(session, path);
        self.ui.entry(next).or_default();
        next
    }

    /// End a session and everything that owner retained.
    ///
    /// #482 stage 1 drops the conflict/follow-up payloads and the plan slot if
    /// this session owned one, while in-flight executions keep running
    /// (ADR-0175); stage 2 adds the read model, which has the same lifetime.
    /// #643 Wave 4 makes this the sole destruction boundary for the UI entry
    /// and all repository-bound resources it owns. Removing the read or UI
    /// anywhere else would violate `dom(ui) = attached sessions`.
    pub(crate) fn release_session(&mut self, session: crate::app::SessionId) {
        self.app_sessions.detach(session);
        self.reads.forget(session);
        self.ui.remove(&session);
        self.pending_pull_confirm.remove(&session);
        if let Some(flight) = &mut self.fetch_in_flight {
            flight.waiters.retain(|waiter| *waiter != session);
        }
    }

    /// `Some(label)` while the tab on screen is waiting for its **first** read —
    /// the `Loading <name>…` placeholder the main pane shows instead of an empty
    /// graph.
    ///
    /// Derived from the owner's request slot, never stored (#482 stage 2 review,
    /// item 2). As a field it was set by the tab switch and cleared by the load
    /// that switch started, so a reload during that first load — a perfectly
    /// legal Cmd+R — refused the first read and then had nothing that cleared
    /// the placeholder: it stayed forever. Whichever read settles, success or
    /// failure, empties the slot, so the placeholder cannot outlive the request
    /// that put it there.
    pub fn loading_tab(&self) -> Option<SharedString> {
        let session = self.active_session()?;
        let waiting = self.reads.is_loading(session) && !self.reads.has_read(session);
        let name = &self.tabs.get(self.active_tab)?.name;
        waiting.then(|| SharedString::from(super::i18n::loading_fmt(name)))
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
        let session = self.attach_session(path.to_path_buf());
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

    /// The commit `session` has selected, taken **before** a new read replaces
    /// its rows.
    ///
    /// A row index is not stable across a rebuild, so a landing read has to
    /// carry the selection by `CommitId` or it silently re-points at whichever
    /// commit inherited that index. ADR-0197 決定 3: a retained value is not
    /// authoritative against a read it has not been revalidated by — and the
    /// owner of the read is not necessarily the tab on screen, so this is keyed
    /// by `session` rather than reached through [`KagiApp::ui`].
    fn selected_commit(&self, session: crate::app::SessionId) -> Option<CommitId> {
        let row = self.ui.get(&session)?.selected?;
        let detail = self.reads.get(Some(session)).details.get(row)?;
        Some(CommitId(detail.full_sha.to_string()))
    }

    /// Put `anchor` back on the read that just landed for `session`: the same
    /// commit's new row index, or no selection at all when that commit is gone
    /// from the graph.
    ///
    /// Runs for the **owner**, active or not. A background tab's revalidate
    /// renumbers its rows exactly as an active tab's does, and leaving its
    /// `selected` on the old index is how returning to it would show a
    /// different commit selected — and dispatch checkout / cherry-pick / revert
    /// at that one.
    fn reanchor_selection(&mut self, session: crate::app::SessionId, anchor: Option<CommitId>) {
        let row = anchor.and_then(|id| {
            self.reads
                .get(Some(session))
                .commit_row_index
                .get(&id)
                .copied()
        });
        if let Some(state) = self.ui.get_mut(&session) {
            state.selected = row;
        }
    }

    /// Amend the owner's read model **without** superseding a read in flight —
    /// commit-graph paging, which refines what is on screen rather than
    /// observing the repository afresh. A pending full reload still lands and
    /// still does its conflict re-detection and status baseline update.
    pub fn amend_tab_view(&mut self, session: crate::app::SessionId, view: TabViewState) {
        let anchor = self.selected_commit(session);
        self.reads.amend(session, view);
        if let Some(ui) = self.ui.get_mut(&session) {
            ui.view_publish_gen = ui.view_publish_gen.wrapping_add(1);
            ui.pane_revalidation = super::tab_ui_state_ops::PaneRevalidation::Queued;
        }
        self.reanchor_selection(session, anchor);
        self.on_view_published(session);
    }

    /// Publish a freshly-built read model for `session` (bootstrap, remote
    /// snapshot) — supersedes anything in flight for that owner.
    pub fn publish_tab_view(&mut self, session: crate::app::SessionId, view: TabViewState) {
        let anchor = self.selected_commit(session);
        self.reads.publish(session, view);
        if let Some(ui) = self.ui.get_mut(&session) {
            ui.view_publish_gen = ui.view_publish_gen.wrapping_add(1);
            ui.pane_revalidation = super::tab_ui_state_ops::PaneRevalidation::Queued;
        }
        self.reanchor_selection(session, anchor);
        self.on_view_published(session);
    }

    /// Publish the completion of a read that was started with
    /// [`crate::app::Reads::begin`]. `false` = superseded: nothing was written
    /// and the caller must drop the result without any display side effect.
    pub fn accept_tab_view(&mut self, key: crate::app::ReadKey, view: TabViewState) -> bool {
        let anchor = self.selected_commit(key.session());
        if !self.reads.accept(key, view) {
            return false;
        }
        if let Some(ui) = self.ui.get_mut(&key.session()) {
            ui.view_publish_gen = ui.view_publish_gen.wrapping_add(1);
            ui.pane_revalidation = super::tab_ui_state_ops::PaneRevalidation::Queued;
        }
        self.reanchor_selection(key.session(), anchor);
        self.on_view_published(key.session());
        true
    }

    /// The UI's reaction to a *different* read model being on screen — a new one
    /// published for the active owner, or a tab switch to another owner. Not
    /// called when a background tab's read lands: that owner's data is stored,
    /// but the active tab's diff panes and caches must not be touched.
    pub(crate) fn on_view_published(&mut self, session: crate::app::SessionId) {
        // #704: the in-progress operation is part of the read, so its owner
        // learns it the moment the read lands — for a background tab too, and
        // before any conflict editor exists. Admission used to wait for
        // `apply_conflict_detect` to observe it, which is why a repository
        // opened mid-merge had no abort until something built a `ConflictView`
        // that a resolved merge never gets. (`observe_conflict` declines while
        // a write of its own is in flight, so this cannot race one.)
        let observed = self
            .reads
            .get(Some(session))
            .operation
            .as_ref()
            .map(|operation| operation.observation.clone());
        self.app_sessions.observe_conflict(session, observed);
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
        self.invalidate_caches_for_row_renumber(session);

        // Tie a worktree tab's colour to its WIP-row colour: the WIP row uses
        // lane_color(rank-in-worktrees-list), so record the same rank on the tab.
        let wt_idx = self.view().worktrees.iter().position(|w| w.is_current);
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            if tab.is_worktree {
                tab.wt_color_idx = wt_idx;
            }
        }
    }

    /// The read model on screen changed owner (a tab switch). Same UI reaction
    /// as publishing a new read for the active owner (row-index caches, sidebar
    /// fingerprint, background scans) — see [`KagiApp::on_view_published`] — but
    /// a switch re-activates an *existing* read whose commit rows are not
    /// renumbered, so the retained main diff / compare pane stay valid and must
    /// survive that method's row-renumber sweep (ADR-0197 決定 3). Their derived
    /// caches are revalidated separately by `begin_session_revalidation` plus
    /// the activation full read.
    pub(crate) fn on_view_switched(&mut self) {
        let Some(session) = self.active_session() else {
            return;
        };
        let main_diff = self.ui().main_diff.clone();
        let compare_view = self.ui().compare_view.clone();
        self.on_view_published(session);
        if let Some(ui) = self.ui.get_mut(&session) {
            ui.main_diff = main_diff;
            ui.compare_view = compare_view;
        }
    }
}
