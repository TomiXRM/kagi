//! UI module — T008: GPUI commit list / T009: commit graph lane / T010: commit selection + detail panel / T011: changed files list / T012: file diff viewer / T013: checkout plan modal + sidebar / T023: pane resize / T-BP-002: bottom panel open/close + resize / T-BP-007: terminal
//!
//! This module lives in the binary crate (`main.rs` does `mod ui;`).
//! It must not be added to `src/lib.rs` so that domain tests stay
//! independent of GPUI.

pub mod avatar;
pub mod avatar_fetch;
pub mod assets;
pub mod commands;
pub mod commit_list;
pub mod commit_panel;
pub mod context_menu;
pub mod detail_panel;
pub mod file_tree;
pub mod graph_view;
pub mod inspector;
pub mod sidebar;
pub mod tabs;
pub mod terminal;
pub mod theme;
pub mod watcher;

use theme::theme;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{
    App, Context, Entity, FocusHandle, KeyDownEvent, KeyBinding, MouseButton, SharedString, Window,
    UniformListScrollHandle, ScrollStrategy,
    actions, div, prelude::*, px, rgb, uniform_list,
};
use gpui_component::input::{Input, InputState};
use gpui_component::tooltip::Tooltip;
use gpui_component::checkbox::Checkbox;
use gpui_component::scroll::Scrollbar;
use gpui_component::Sizable as _;

// ──────────────────────────────────────────────────────────────
// T-BP-002: Bottom Panel — action + tab enum
// ──────────────────────────────────────────────────────────────

// cmd-j toggle action for the bottom panel.
// escape to close main diff view.
actions!(kagi, [ToggleBottomPanel, CloseMainDiff, DiffPrevFile, DiffNextFile]);

/// Active tab in the bottom panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomTab {
    OperationLog,
    Terminal,
}

impl BottomTab {
    fn label(self) -> &'static str {
        match self {
            BottomTab::OperationLog => "Operation Log",
            BottomTab::Terminal => "Terminal",
        }
    }
}

// ──────────────────────────────────────────────────────────────
// T023: Pane resize — divider drag state
// ──────────────────────────────────────────────────────────────

/// Which divider is being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DividerKind {
    /// The divider between the sidebar and the commit list.
    Sidebar,
    /// The divider between the commit list and the detail/diff panel.
    Panel,
    /// T030: The divider between the badge column and the graph column.
    BadgeCol,
    /// T030: The divider between the graph column and the message column.
    GraphCol,
    /// T-BP-002: The divider at the top edge of the bottom panel.
    BottomPanel,
    /// W7-INSPECTOR2: The horizontal divider inside the inspector between the
    /// message scroll box (top) and the changed-files list (bottom).
    InspectorSplit,
}

/// Drag payload for a divider drag.  Only the divider kind is needed: widths
/// are derived from the absolute cursor position during drag-move (see the
/// drag-move listener), so no drag-start anchor has to be carried around.
#[derive(Clone, Copy, Debug)]
pub struct DividerDrag {
    pub kind: DividerKind,
}

/// Invisible ghost view rendered during a divider drag.  gpui requires a
/// `Render`-able entity as the drag ghost, so we use this zero-size placeholder.
struct DividerGhost;
impl gpui::Render for DividerGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        div()
    }
}

// Sidebar / panel width limits.
const SIDEBAR_MIN: f32 = 120.0;
const SIDEBAR_MAX: f32 = 400.0;
const PANEL_MIN: f32 = 240.0;
const PANEL_MAX: f32 = 800.0;

// Default widths (matching the pre-T023 hard-coded values).
const SIDEBAR_DEFAULT: f32 = 200.0;
const PANEL_DEFAULT: f32 = 360.0;

// T-BP-004: Operation Log ring-buffer size and initial load count.
const OP_ENTRIES_MAX: usize = 500;
const OP_ENTRIES_LOAD: usize = 100;

// T-BP-002: Bottom panel height limits and default.
const BOTTOM_PANEL_MIN_H: f32 = 80.0;
// W2-STATUS / ADR-0017: default height = 18% of the viewport (requirement: ≤20%).
// Resolved lazily on first render (the viewport size is unknown at construction);
// `BOTTOM_PANEL_H_UNSET` marks "not yet resolved".
const BOTTOM_PANEL_DEFAULT_FRAC: f32 = 0.18;
const BOTTOM_PANEL_H_UNSET: f32 = 0.0;
// Maximum fraction of the viewport height the bottom panel may occupy.
const BOTTOM_PANEL_MAX_FRAC: f32 = 0.6;
// Height of the horizontal divider handle at the top of the bottom panel.
const BOTTOM_PANEL_DIVIDER_H: f32 = 4.0;
// Height of the tab bar inside the bottom panel.
const BOTTOM_PANEL_TAB_H: f32 = 28.0;

// T030: Commit-list inner column width limits and defaults.
const BADGE_COL_MIN: f32 = 60.0;
const BADGE_COL_MAX: f32 = 400.0;
const BADGE_COL_DEFAULT: f32 = 150.0;

const GRAPH_COL_MIN: f32 = 28.0;
const GRAPH_COL_MAX: f32 = 600.0;
// Default: 8 lanes × LANE_W = 112px (matches the pre-T030 MAX_LANES=8 behaviour).
const GRAPH_COL_DEFAULT: f32 = 8.0 * graph_view::LANE_W;

// Height of the column header row above the commit list.
const COL_HEADER_H: f32 = 20.0;

// W7-INSPECTOR2: inspector message/files vertical split.
/// Default split ratio (message:files = 1:1).
const INSPECTOR_SPLIT_DEFAULT: f32 = 0.5;
/// Clamp bounds for the split ratio when dragging the divider.
const INSPECTOR_SPLIT_MIN: f32 = 0.2;
const INSPECTOR_SPLIT_MAX: f32 = 0.8;
/// Vertical offset of the inspector content area from the top of the window:
/// tab strip (30) + its 1px bottom border + header toolbar (52).
/// W10-TOOLBAR: header grew to 52px for the Finder-style icon-over-label
/// vertical buttons (was 34px). Measured `bounds` is the primary path; this
/// constant is only the startup fallback.
const INSPECTOR_TOP_OFFSET: f32 = 30.0 + 1.0 + 52.0;
/// Height of the status bar at the very bottom of the window.
const STATUS_BAR_H: f32 = 22.0;

// Width of the inner divider handles (badge|graph and graph|message).
const INNER_DIV_W: f32 = 4.0;

// ── W2-GRAPH: compact mode ────────────────────────────────────
/// Row height for normal (full) mode.
const ROW_H_FULL: f32 = graph_view::ROW_H;  // 29.0 (= 24 * 1.2)
/// Row height for compact mode.
const ROW_H_COMPACT: f32 = 22.0;  // 18.0 * 1.2 (keeps compact:full ratio)

/// Return the row height for the current compact mode setting.
#[inline]
fn row_height(compact: bool) -> f32 {
    if compact { ROW_H_COMPACT } else { ROW_H_FULL }
}

use kagi::git::{
    ChangeKind, CommitId, FileDiff, DiffLineKind, FileStatus, Head, RemoteBranch, RepoSnapshot, Stash, Tag, UpstreamInfo, Worktree,
    ops::{
        OperationPlan, StateSummary,
        execute_checkout, execute_checkout_commit, execute_create_branch,
        plan_checkout, plan_checkout_commit, plan_create_branch_with_checkout, preflight_check,
        execute_create_worktree, plan_create_worktree,
        plan_stash_push, execute_stash_push,
        plan_stash_apply, execute_stash_apply,
        plan_pull, execute_pull, PullOutcome,
        plan_undo_commit, execute_undo_commit,
        plan_stash_pop, execute_stash_pop,
        plan_push, execute_push,
        preflight_check_stash,
        plan_cherry_pick, execute_cherry_pick,
        plan_revert, execute_revert,
        plan_delete_branch, execute_delete_branch,
    },
    oplog::{OpLogEntry, OpOutcome, append_oplog, read_oplog_tail},
    stage_file, unstage_file, plan_commit, execute_commit,
};
use commit_panel::{CommitPanelState, CommitPanelFileRef, CommitPlanModal, status_badge};
use commit_list::{BadgeKind, CommitRow, build_commit_rows};
use context_menu::{CommitAction, CommitMenuState, MenuContext};
use detail_panel::{CommitDetail, build_commit_details};
use graph_view::graph_canvas;

// ──────────────────────────────────────────────────────────────
// Colours (W9-THEME / ADR-0036): sourced from the active `theme()`.
// All former hard-coded Catppuccin constants moved to `theme.rs`; UI code
// reads `theme().<field>` every frame so a theme switch needs no signature
// churn (just an atomic index update + cx.notify).
// ──────────────────────────────────────────────────────────────

// ──────────────────────────────────────────────────────────────
// T-BP-003: StatusBarSummary — data snapshot for the status bar
// ──────────────────────────────────────────────────────────────

/// Summary of repository state shown in the status bar (T-BP-003).
///
/// Derived from [`RepoSnapshot`] on each reload; the UI renders these
/// pre-computed values so no snapshot re-reading happens in render.
#[derive(Clone, Debug, Default)]
pub struct StatusBarSummary {
    /// Short branch name, `"detached HEAD"`, or `"no commits yet"`.
    pub branch: String,
    /// Whether the working tree is dirty (shows ● bullet).
    pub is_dirty: bool,
    /// Number of staged files (shown only when > 0).
    pub staged: usize,
    /// Number of unstaged files (shown only when > 0).
    pub unstaged: usize,
    /// Commits ahead of upstream (None = no upstream / detached).
    pub ahead: Option<usize>,
    /// Commits behind upstream (None = no upstream / detached).
    pub behind: Option<usize>,
    /// Whether there is no upstream configured (and not detached).
    pub no_upstream: bool,
    /// Wall-clock time (seconds since Unix epoch) of the last reload.
    pub last_refresh_secs: i64,
    /// Whether HEAD is detached (T-HT-001: used for toolbar disabled logic).
    pub is_detached: bool,
    /// Whether HEAD is unborn (no commits yet) (T-HT-001: used for toolbar disabled logic).
    pub is_unborn: bool,
    /// Number of stash entries (T-HT-001: used for Pop button disabled logic).
    pub stash_count: usize,
    /// Short repo name derived from path (T-HT-001: displayed in toolbar).
    pub repo_name: String,
    /// Whether at least one remote is configured (T-HT-004: push set-upstream flow).
    pub has_remote: bool,
    /// Upstream tracking ref name, e.g. `"origin/main"` (empty when no upstream / detached).
    /// ADR-0013: displayed in the left toolbar region as `→ upstream_name`.
    pub upstream_name: String,
    /// Number of conflicted files (W2-STATUS: shown as `!N` in red when > 0).
    pub conflict_count: usize,
}

impl StatusBarSummary {
    /// Build from a [`RepoSnapshot`] at the current wall clock time.
    pub fn from_snapshot(snap: &kagi::git::RepoSnapshot) -> Self {
        use kagi::git::Head;
        use commit_list::now_unix_secs;

        let (branch, ahead, behind, no_upstream, is_detached, is_unborn, upstream_name) = match &snap.head {
            Head::Attached { branch, .. } => {
                // Look up upstream info for this branch.
                let upstream = snap
                    .branches
                    .iter()
                    .find(|b| &b.name == branch)
                    .and_then(|b| b.upstream.as_ref());
                match upstream {
                    Some(u) => (branch.clone(), Some(u.ahead), Some(u.behind), false, false, false, u.remote_branch.clone()),
                    None => (branch.clone(), None, None, true, false, false, String::new()),
                }
            }
            Head::Detached { target } => {
                let short = target.get(..8).unwrap_or(target).to_string();
                (format!("detached HEAD ({})", short), None, None, false, true, false, String::new())
            }
            Head::Unborn { branch } => {
                (format!("no commits yet ({})", branch), None, None, false, false, true, String::new())
            }
        };

        // Derive has_remote from remote_branches (any entry means at least one remote exists).
        let has_remote = !snap.remote_branches.is_empty();

        StatusBarSummary {
            branch,
            is_dirty: snap.status.is_dirty(),
            staged: snap.status.staged.len(),
            unstaged: snap.status.unstaged.len(),
            ahead,
            behind,
            no_upstream,
            last_refresh_secs: now_unix_secs(),
            is_detached,
            is_unborn,
            stash_count: snap.stashes.len(),
            repo_name: String::new(), // filled in by caller after from_snapshot
            has_remote,
            upstream_name,
            conflict_count: snap.status.conflicted.len(),
        }
    }

    /// Emit the headless verification log line required by T-BP-003.
    ///
    /// Format: `[kagi] statusbar: <branch> ↑A ↓B staged=N unstaged=M`
    pub fn log_headless(&self) {
        let ahead = self.ahead.unwrap_or(0);
        let behind = self.behind.unwrap_or(0);
        // W2-STATUS: conflicts / stash / upstream appended (prefix kept
        // identical so older verification greps keep matching).
        eprintln!(
            "[kagi] statusbar: {} \u{2191}{} \u{2193}{} staged={} unstaged={} conflicts={} stash={} upstream={}",
            self.branch, ahead, behind, self.staged, self.unstaged,
            self.conflict_count, self.stash_count,
            if self.upstream_name.is_empty() { "-" } else { &self.upstream_name },
        );
    }

    /// Derive toolbar enabled/disabled flags from this summary.
    ///
    /// Returns `ToolbarState` for use in rendering and headless logging (T-HT-001).
    pub fn toolbar_state(&self) -> ToolbarState {
        // detached or unborn HEAD: no upstream possible, so also disables push/pull.
        let not_attached = self.is_detached || self.is_unborn;
        // no_upstream covers Attached branch with no upstream configured.
        let no_upstream = self.no_upstream || not_attached;

        let behind = self.behind.unwrap_or(0);
        let ahead = self.ahead.unwrap_or(0);

        // ADR-0013: Pull disabled if no upstream OR behind=0 (nothing to pull).
        let pull_on = !no_upstream && behind > 0;
        // Push: enabled when (upstream && ahead>0) OR (no-upstream && attached && remote exists).
        // Dirty WT is irrelevant — push never changes local state.
        let push_on = (!no_upstream && ahead > 0)
            || (self.no_upstream && !not_attached && self.has_remote);
        let stash_on = self.is_dirty; // Stash: disabled if working tree is clean
        let pop_on = self.stash_count > 0; // Pop: disabled if no stashes
        let undo_on = !not_attached && ahead > 0; // disabled if detached/unborn or ahead=0

        ToolbarState {
            pull_on,
            push_on,
            stash_on,
            pop_on,
            undo_on,
            behind,
            ahead,
        }
    }
}

// ──────────────────────────────────────────────────────────────
// T-HT-001: ToolbarState — pre-computed button enabled flags
// ──────────────────────────────────────────────────────────────

/// Pre-computed enabled/disabled flags for each toolbar button (T-HT-001).
///
/// Derived from [`StatusBarSummary`] on every reload.  The render path
/// uses these values; the headless path logs them.
#[derive(Clone, Debug, Default)]
pub struct ToolbarState {
    /// Whether the Pull button is enabled.
    pub pull_on: bool,
    /// Whether the Push button is enabled.
    pub push_on: bool,
    /// Whether the Stash button is enabled.
    pub stash_on: bool,
    /// Whether the Pop button is enabled.
    pub pop_on: bool,
    /// Whether the Undo button is enabled.
    pub undo_on: bool,
    /// Commits behind upstream (used for Pull button label ↓N). ADR-0013.
    pub behind: usize,
    /// Commits ahead of upstream (used for Push button label ↑N). ADR-0013.
    pub ahead: usize,
}

impl ToolbarState {
    /// Emit the headless toolbar log line required by T-HT-001 / ADR-0013.
    ///
    /// Format: `[kagi] toolbar: pull=on/off (behind=N) push=on/off (ahead=N) stash=on/off pop=on/off undo=on/off`
    pub fn log_headless(&self) {
        eprintln!(
            "[kagi] toolbar: pull={} (behind={}) push={} (ahead={}) stash={} pop={} undo={}",
            if self.pull_on { "on" } else { "off" },
            self.behind,
            if self.push_on { "on" } else { "off" },
            self.ahead,
            if self.stash_on { "on" } else { "off" },
            if self.pop_on { "on" } else { "off" },
            if self.undo_on { "on" } else { "off" },
        );
    }
}

// ──────────────────────────────────────────────────────────────
// W2-HEADER: unit tests for ToolbarState / ADR-0013
// ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod toolbar_tests {
    use super::*;

    /// Build a minimal `StatusBarSummary` with upstream set.
    fn make_summary(ahead: usize, behind: usize, is_dirty: bool, stash_count: usize) -> StatusBarSummary {
        StatusBarSummary {
            branch: "main".to_string(),
            is_dirty,
            staged: 0,
            unstaged: if is_dirty { 1 } else { 0 },
            ahead: Some(ahead),
            behind: Some(behind),
            no_upstream: false,
            last_refresh_secs: 0,
            is_detached: false,
            is_unborn: false,
            stash_count,
            repo_name: "repo".to_string(),
            has_remote: true,
            upstream_name: "origin/main".to_string(),
            conflict_count: 0,
        }
    }

    /// State 1: clean branch, behind=0, ahead=0 (e.g. fixture main when in sync).
    #[test]
    fn toolbar_clean_behind0() {
        let s = make_summary(0, 0, false, 0);
        let t = s.toolbar_state();
        // ADR-0013: Pull disabled when behind=0.
        assert!(!t.pull_on, "pull must be off when behind=0");
        assert!(!t.push_on, "push must be off when ahead=0");
        assert!(!t.stash_on, "stash must be off when clean");
        assert!(!t.pop_on, "pop must be off when no stash");
        assert!(!t.undo_on, "undo must be off when ahead=0");
        assert_eq!(t.behind, 0);
        assert_eq!(t.ahead, 0);
    }

    /// State 2: dirty branch, behind=1 (fixture main: behind0 but ahead=1 / dirty).
    /// Use behind=1 to verify pull=on; dirty=true to verify stash=on.
    #[test]
    fn toolbar_dirty_behind1() {
        let s = make_summary(1, 1, true, 0);
        let t = s.toolbar_state();
        assert!(t.pull_on, "pull must be on when behind=1");
        assert!(t.push_on, "push must be on when ahead=1");
        assert!(t.stash_on, "stash must be on when dirty");
        assert!(!t.pop_on, "pop must be off when no stash");
        assert!(t.undo_on, "undo must be on when ahead=1");
        assert_eq!(t.behind, 1);
        assert_eq!(t.ahead, 1);
    }

    /// State 3: detached HEAD — all git ops disabled.
    #[test]
    fn toolbar_detached() {
        let s = StatusBarSummary {
            branch: "detached HEAD (abc12345)".to_string(),
            is_dirty: false,
            staged: 0,
            unstaged: 0,
            ahead: None,
            behind: None,
            no_upstream: false,
            last_refresh_secs: 0,
            is_detached: true,
            is_unborn: false,
            stash_count: 0,
            repo_name: "repo".to_string(),
            has_remote: true,
            upstream_name: String::new(),
            conflict_count: 0,
        };
        let t = s.toolbar_state();
        assert!(!t.pull_on, "pull must be off on detached HEAD");
        assert!(!t.push_on, "push must be off on detached HEAD");
        assert!(!t.undo_on, "undo must be off on detached HEAD");
    }

    /// ADR-0013: fixture main (ahead=1, behind=0) → pull must be off.
    #[test]
    fn toolbar_fixture_main_behind0_pull_off() {
        // This mirrors the fixture repo: main is 1 ahead, 0 behind.
        let s = make_summary(1, 0, false, 0);
        let t = s.toolbar_state();
        assert!(!t.pull_on, "fixture main (behind=0) must have pull=off");
        assert!(t.push_on, "fixture main (ahead=1) must have push=on");
        assert!(t.undo_on, "fixture main (ahead=1) must have undo=on");
    }

    /// ADR-0013: feature/two (ahead=0, behind=1) → pull must be on, push must be off.
    #[test]
    fn toolbar_feature_two_behind1_pull_on() {
        // Mirrors fixture feature/two: 0 ahead, 1 behind.
        let s = make_summary(0, 1, false, 0);
        let t = s.toolbar_state();
        assert!(t.pull_on, "feature/two (behind=1) must have pull=on");
        assert!(!t.push_on, "feature/two (ahead=0) must have push=off (no upstream-new branch)");
        assert!(!t.undo_on, "feature/two (ahead=0) must have undo=off");
    }

    /// log_headless format includes (behind=N) and (ahead=N).
    #[test]
    fn toolbar_log_format_behind_ahead() {
        let s = make_summary(2, 3, true, 1);
        let t = s.toolbar_state();
        // Verify fields are correct (can't easily capture stderr; just verify struct values).
        assert_eq!(t.behind, 3);
        assert_eq!(t.ahead, 2);
        assert!(t.pull_on);
        assert!(t.push_on);
        assert!(t.stash_on);
        assert!(t.pop_on);
        assert!(t.undo_on);
    }
}

/// Format a Unix-epoch timestamp as `"HH:MM:SS"` (local wall-clock, UTC).
///
/// Reuses the same constant-time civil arithmetic as `detail_panel::format_utc`.
pub fn format_hms(epoch_secs: i64) -> String {
    const SECS_PER_DAY: i64 = 86_400;
    let time_of_day = if epoch_secs >= 0 {
        epoch_secs % SECS_PER_DAY
    } else {
        let d = (epoch_secs - (SECS_PER_DAY - 1)) / SECS_PER_DAY;
        epoch_secs - d * SECS_PER_DAY
    };
    let hour = (time_of_day / 3_600) as u32;
    let minute = ((time_of_day % 3_600) / 60) as u32;
    let second = (time_of_day % 60) as u32;
    format!("{:02}:{:02}:{:02}", hour, minute, second)
}

// ──────────────────────────────────────────────────────────────
// StatusFooter — last operation result display (T017)
// ──────────────────────────────────────────────────────────────

/// Outcome kind for the status footer bar (T017).
#[derive(Clone, Debug)]
pub enum FooterStatus {
    /// A git operation completed successfully (shown in green).
    Success(SharedString),
    /// A git operation failed (shown in red).
    Failed(SharedString),
    /// Idle state: shows repo name / branch info (no colour tint).
    Idle(SharedString),
    /// W2-STATUS: a git operation is in progress (shown in blue with ⟳).
    Busy(SharedString),
}

// ──────────────────────────────────────────────────────────────
// W3-NOTIFY: toast (snackbar) notifications
// ──────────────────────────────────────────────────────────────

/// Visual kind of a toast notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    /// Operation started / neutral info (blue, 4s).
    Info,
    /// Operation succeeded (green, 4s).
    Success,
    /// Operation failed or was refused (red, 8s).
    Error,
}

/// One snackbar entry, stacked bottom-right above the status bar.
///
/// Rendered by `render_toasts`; pruned by a 500ms ticker task that runs only
/// while toasts exist (spawned lazily from `push_toast`/`render`).
/// We deliberately do NOT use gpui-component's Root notification layer:
/// pushing into it requires `Root::update` and our render runs *inside*
/// Root's render pass, so that would re-entrantly borrow the Root entity.
#[derive(Clone, Debug)]
pub struct Toast {
    /// Unique id (for the dismiss button element id).
    pub id: u64,
    pub kind: ToastKind,
    pub message: SharedString,
    /// Creation time; used by the pruner for auto-dismiss.
    pub born: std::time::Instant,
}

impl Toast {
    /// Auto-dismiss lifetime by kind (errors stay longer).
    fn lifetime(&self) -> Duration {
        match self.kind {
            ToastKind::Error => Duration::from_secs(8),
            _ => Duration::from_secs(4),
        }
    }

    fn expired(&self) -> bool {
        self.born.elapsed() >= self.lifetime()
    }
}

/// Maximum simultaneously visible toasts (oldest dropped beyond this).
const TOASTS_MAX: usize = 4;

// ──────────────────────────────────────────────────────────────
// FileDiffView — pre-rendered diff rows for the diff panel
// ──────────────────────────────────────────────────────────────

/// A single displayable row in the diff viewer.
#[derive(Clone)]
pub enum DiffRow {
    /// A hunk header line (`@@ -a,b +c,d @@`).
    HunkHeader(SharedString),
    /// A content line (context / added / removed).
    Line {
        kind: DiffLineKind,
        /// The line content as a displayable string (with leading sigil stripped).
        text: SharedString,
        /// Old-side line number (None for Added lines).
        old_lineno: Option<u32>,
        /// New-side line number (None for Removed lines).
        new_lineno: Option<u32>,
        /// T-UI-004: Pre-computed syntax highlight spans (byte ranges + styles).
        /// Empty when the file type is unknown or highlighting failed.
        highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)>,
    },
    /// Placeholder shown for binary files.
    Binary,
}

/// Pre-computed state for the diff view panel.
#[derive(Clone)]
pub struct FileDiffView {
    /// Display name of the file (path component).
    pub file_name: SharedString,
    /// All displayable rows: hunk headers + content lines.
    pub rows: Vec<DiffRow>,
    /// Row index into the commit's changed-files list (reserved for future
    /// navigation: e.g. "previous / next file" buttons in the diff panel).
    #[allow(dead_code)]
    pub file_index: usize,
}

impl FileDiffView {
    /// Build a [`FileDiffView`] from a [`FileDiff`] result.
    pub fn from_file_diff(file_diff: &FileDiff, file_index: usize) -> Self {
        let path = file_diff
            .new_path
            .as_ref()
            .or(file_diff.old_path.as_ref());
        let file_name = SharedString::from(
            path.map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );

        let mut rows: Vec<DiffRow> = Vec::new();

        if file_diff.is_binary {
            rows.push(DiffRow::Binary);
        } else {
            for hunk in &file_diff.hunks {
                // Build hunk header string.
                let (os, oc) = hunk.old_range;
                let (ns, nc) = hunk.new_range;
                let header = SharedString::from(format!(
                    "@@ -{},{} +{},{} @@",
                    os, oc, ns, nc
                ));
                rows.push(DiffRow::HunkHeader(header));

                for line in &hunk.lines {
                    // Strip the trailing newline for display (keep content clean).
                    let raw = line.content.trim_end_matches('\n').trim_end_matches('\r');
                    // Add leading sigil for clarity.
                    let text = match line.kind {
                        DiffLineKind::Added   => SharedString::from(format!("+{}", raw)),
                        DiffLineKind::Removed => SharedString::from(format!("-{}", raw)),
                        DiffLineKind::Context => SharedString::from(format!(" {}", raw)),
                    };
                    rows.push(DiffRow::Line {
                        kind: line.kind.clone(),
                        text,
                        old_lineno: line.old_lineno,
                        new_lineno: line.new_lineno,
                        highlights: vec![],
                    });
                }
            }
        }

        FileDiffView {
            file_name,
            rows,
            file_index,
        }
    }
}

// ──────────────────────────────────────────────────────────────
// T-UI-003: MainDiffView — full-width main pane diff state
// ──────────────────────────────────────────────────────────────

/// Where the diff was opened from (used for re-load and navigation).
#[derive(Clone)]
pub enum MainDiffSource {
    /// Opened from the commit detail panel (changed-files list).
    Commit { row_index: usize, file_index: usize },
    /// Opened from the compare changed-files list.
    Compare {
        base: CommitId,
        target: CompareTarget,
        file_index: usize,
    },
    /// Opened from the Commit Panel — unstaged file.
    Unstaged { path: PathBuf },
    /// Opened from the Commit Panel — staged file.
    Staged { path: PathBuf },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompareTarget {
    Head,
    WorkingTree,
}

#[derive(Clone, Debug)]
pub struct CompareView {
    pub base: CommitId,
    pub target: CompareTarget,
    pub files: Vec<FileStatus>,
    pub title: SharedString,
}

/// State for the full-width main pane diff view (T-UI-003).
#[derive(Clone)]
pub struct MainDiffView {
    /// Display title: file path.
    pub title: SharedString,
    /// Stats string: "+N −M".
    pub stats: SharedString,
    /// All displayable rows (hunk headers + content lines).
    pub rows: Vec<DiffRow>,
    /// Where this diff was opened from (for re-load / back navigation).
    #[allow(dead_code)]
    pub source: MainDiffSource,
}

// ──────────────────────────────────────────────────────────────
// T-UI-004: Syntax highlighting for diff rows
// ──────────────────────────────────────────────────────────────

/// Map a file extension to a language name understood by `gpui_component`'s
/// `LanguageRegistry`.  Returns `None` for unknown extensions.
fn lang_for_ext(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "rs"                   => Some("rust"),
        "py"                   => Some("python"),
        "js" | "jsx"           => Some("javascript"),
        "ts"                   => Some("typescript"),
        "tsx"                  => Some("tsx"),
        "json" | "jsonc"       => Some("json"),
        "toml"                 => Some("toml"),
        "yaml" | "yml"         => Some("yaml"),
        "md" | "mdx"           => Some("markdown"),
        "sh" | "bash"          => Some("bash"),
        "c"                    => Some("c"),
        "cpp" | "cc" | "cxx"  => Some("cpp"),
        "h" | "hpp"            => Some("cpp"),
        "css" | "scss"         => Some("css"),
        "html" | "htm"         => Some("html"),
        "go"                   => Some("go"),
        "java"                 => Some("java"),
        "rb"                   => Some("ruby"),
        "zig"                  => Some("zig"),
        "sql"                  => Some("sql"),
        "swift"                => Some("swift"),
        _                      => None,
    }
}

/// T-UI-004: Apply syntax highlighting to a slice of `DiffRow`s in-place.
///
/// The file path's extension is used to select the language. If the language
/// is unknown or highlighting fails, rows are left with empty highlight spans
/// (plain-colour fallback).  Never panics.
///
/// Returns the language name that was used (or "none").
fn highlight_diff_rows(rows: &mut Vec<DiffRow>, file_path: &std::path::Path) -> &'static str {
    use gpui_component::highlighter::{SyntaxHighlighter, HighlightTheme};
    use gpui_component::Rope;

    // Determine language from extension.
    let lang = file_path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(lang_for_ext);

    let lang = match lang {
        Some(l) => l,
        None => return "none",
    };

    // Build the full source text for the "new" side of the diff by concatenating
    // all Line rows.  We use a one-pass approach:
    //   1. Collect (text_without_sigil, byte_start_in_rope) for each Line row.
    //   2. Feed the combined text to the highlighter.
    //   3. Distribute the resulting (byte_range, style) spans back to each row,
    //      offsetting by byte_start_in_rope.
    //
    // The sigil (+/-/ ) at position 0 of each `text` is kept in the display string
    // but excluded from the highlighted region — highlights start at byte 1.

    let mut line_offsets: Vec<(usize, usize)> = Vec::new(); // (row_index, rope_byte_start)
    let mut combined = String::new();

    for (i, row) in rows.iter().enumerate() {
        if let DiffRow::Line { text, .. } = row {
            let t = text.as_ref();
            let start = combined.len();
            // Skip the leading sigil ('+', '-', ' ') for parsing purposes.
            // The highlight byte ranges will be relative to `combined`, which
            // starts after the sigil.
            let content = if t.len() > 0 { &t[1..] } else { "" };
            combined.push_str(content);
            combined.push('\n');
            line_offsets.push((i, start));
        }
    }

    if combined.is_empty() {
        return lang;
    }

    // Build highlighter and parse the combined source.
    let mut highlighter = SyntaxHighlighter::new(lang);
    let rope = Rope::from_str(&combined);
    highlighter.update(None, &rope);

    // Use a syntax-highlight theme matching the active UI theme's brightness
    // (W9-THEME): dark themes → default_dark, light themes → default_light.
    let hl_theme = if theme::theme().dark {
        HighlightTheme::default_dark()
    } else {
        HighlightTheme::default_light()
    };
    let all_styles = highlighter.styles(&(0..combined.len()), &hl_theme);

    // Distribute styles back to rows.
    // For each row we know: rope_byte_start (start of content inside `combined`,
    // i.e. after the sigil) and rope_byte_end = start_of_next_row - 1 (the \n).
    for k in 0..line_offsets.len() {
        let (row_i, rope_start) = line_offsets[k];
        let rope_end = if k + 1 < line_offsets.len() {
            line_offsets[k + 1].1
        } else {
            combined.len()
        };
        // The content slice is rope_start..rope_end (excludes the trailing \n).
        let content_end = rope_end.saturating_sub(1); // strip the \n

        // Collect highlight spans that overlap [rope_start, content_end).
        let mut row_highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> = Vec::new();
        for (range, style) in &all_styles {
            let clipped_start = range.start.max(rope_start);
            let clipped_end   = range.end.min(content_end);
            if clipped_start >= clipped_end {
                continue;
            }
            // Translate back to row-local byte offsets (offset by 1 for the sigil).
            let local_start = 1 + (clipped_start - rope_start);
            let local_end   = 1 + (clipped_end   - rope_start);
            row_highlights.push((local_start..local_end, *style));
        }

        if let DiffRow::Line { highlights, .. } = &mut rows[row_i] {
            *highlights = row_highlights;
        }
    }

    lang
}

// ──────────────────────────────────────────────────────────────
// CheckoutPlanModal — state for the plan confirmation overlay (T013)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress checkout plan confirmation.
#[derive(Clone)]
pub struct CheckoutPlanModal {
    /// Branch or commit target captured when the plan was opened.
    pub target: CheckoutPlanTarget,
    /// The computed plan (title, current, predicted, warnings, blockers, recovery).
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if execute or preflight failed (replaces normal buttons).
    pub error: Option<SharedString>,
}

/// Execution target for the shared checkout plan modal.
#[derive(Clone, Debug)]
pub enum CheckoutPlanTarget {
    Branch(String),
    Commit(CommitId),
}

/// State for an in-progress pull confirmation (T-HT-003).  Same shape as
/// [`CheckoutPlanModal`] but kept separate so the confirm path can't be mixed up.
#[derive(Clone)]
pub struct PullPlanModal {
    /// The computed pull plan.
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

/// State for an in-progress undo-commit confirmation (T-HT-009).
#[derive(Clone)]
pub struct UndoPlanModal {
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
}

/// State for an in-progress stash-pop confirmation (T-HT-007).
#[derive(Clone)]
pub struct PopPlanModal {
    pub plan: std::sync::Arc<OperationPlan>,
    pub error: Option<SharedString>,
    /// Stash index the plan was built for.
    pub stash_index: usize,
}

/// State for an in-progress push confirmation (T-HT-004).  Same shape as
/// [`PullPlanModal`] but kept separate so the confirm path can't be mixed up.
#[derive(Clone)]
pub struct PushPlanModal {
    /// The computed push plan.
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// CreateBranchModal — state for the create-branch overlay (T014)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress create-branch confirmation.
///
/// The user types a branch name; the plan is regenerated live on each keystroke.
#[derive(Clone)]
pub struct CreateBranchModal {
    /// The commit at which the branch will be created.
    pub at: CommitId,
    /// First line of the start commit message, used to identify menu origin.
    pub start_title: String,
    /// Current text in the branch-name input field (synced from `input_state`).
    pub input: String,
    /// Real text-input entity (gpui-component). Created lazily on first
    /// render (needs a Window); `None` in headless paths.
    pub input_state: Option<Entity<InputState>>,
    /// Whether to check out the new branch after creating it.
    pub checkout_after: bool,
    /// Live plan (re-generated each keystroke from `input` and `at`).
    pub plan: Option<std::sync::Arc<OperationPlan>>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // legacy hand-rolled input era; kept for struct compat
pub enum WorktreeModalField {
    Branch,
    Path,
}

/// State for an in-progress create-worktree confirmation.
#[derive(Clone)]
pub struct CreateWorktreeModal {
    /// The commit used as the start point for the new branch.
    pub at: CommitId,
    /// First line of the start commit message.
    pub start_title: String,
    /// New branch name (synced from `branch_state`).
    pub branch_input: String,
    /// Real branch-name input entity (lazy; None headless).
    pub branch_state: Option<Entity<InputState>>,
    /// Target worktree path (synced from `path_state`).
    pub path_input: String,
    /// Real path input entity (lazy; None headless).
    pub path_state: Option<Entity<InputState>>,
    /// True once the user has manually edited the path.
    pub path_touched: bool,
    /// Which field receives key input (legacy hand-rolled input era; the
    /// real `InputState`s manage their own focus now).
    #[allow(dead_code)]
    pub active_field: WorktreeModalField,
    /// Live plan regenerated from branch/path/start.
    pub plan: Option<std::sync::Arc<OperationPlan>>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// StashPushModal — state for the stash push confirmation overlay (T015)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress stash push confirmation.
///
/// The user may optionally type a stash message; the live plan is regenerated
/// on each keystroke.
#[derive(Clone)]
pub struct StashPushModal {
    /// Optional stash message (empty string → None passed to stash_save2).
    /// Synced from `input_state`.
    pub input: String,
    /// Real text-input entity (lazy; None headless).
    pub input_state: Option<Entity<InputState>>,
    /// Live plan (re-generated each keystroke from `input`).
    pub plan: Option<std::sync::Arc<OperationPlan>>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// StashApplyModal — state for the stash apply confirmation overlay (T015)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress stash apply confirmation.
#[derive(Clone)]
pub struct StashApplyModal {
    /// The stash index to apply.
    pub index: usize,
    /// The computed plan.
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// CherryPickModal — state for the cherry-pick plan overlay (T016)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress cherry-pick plan confirmation.
///
/// The modal shows a preview of affected files and any blockers before
/// the user confirms execution.
#[derive(Clone)]
pub struct CherryPickModal {
    /// The commit id that will be cherry-picked.
    pub commit_id: CommitId,
    /// The computed plan (title, current, predicted, preview_files, blockers, recovery).
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// RevertModal — state for the revert plan overlay (T-CM-034)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress revert plan confirmation.
#[derive(Clone)]
pub struct RevertModal {
    /// The commit id that will be reverted.
    pub commit_id: CommitId,
    /// The computed plan.
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if execute or preflight failed.
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// DeleteBranchModal — state for the delete-branch confirmation overlay (W2-DELETE)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress delete-branch confirmation (W2-DELETE).
///
/// The modal shows blockers (unmerged / current branch) and the recovery
/// `git branch <name> <sha>` string before the user confirms.
#[derive(Clone)]
pub struct DeleteBranchModal {
    /// The local branch name to delete.
    pub branch_name: String,
    /// The computed plan.
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if preflight or execute failed.
    pub error: Option<SharedString>,
}

// ──────────────────────────────────────────────────────────────
// KagiApp — root view
// ──────────────────────────────────────────────────────────────

/// Root GPUI view.  Holds all pre-computed display data so the render
/// closure never calls `format!` on hot paths.
pub struct KagiApp {
    /// Root focus handle.  Created when the window opens and focused by
    /// default — without a focused element gpui never dispatches key events,
    /// so window-wide actions like cmd-j would silently do nothing.
    pub root_focus: Option<gpui::FocusHandle>,
    /// One-line header text: repo name + HEAD + status summary.
    pub header: SharedString,
    /// Pre-computed commit rows (built once from the snapshot).
    pub rows: Vec<CommitRow>,
    /// Pre-computed detail panel data, parallel to `rows`.
    pub details: Vec<CommitDetail>,
    /// Currently selected row index (None = no selection).
    pub selected: Option<usize>,
    /// Error or informational message shown instead of the commit list.
    pub error: Option<SharedString>,
    /// Absolute path to the repository root; used for on-demand diff fetches.
    pub repo_path: Option<PathBuf>,
    /// Cache of changed-files results keyed by row index.
    /// `None` value means the diff was attempted but failed (show unavailable).
    pub diff_cache: HashMap<usize, Option<Vec<FileStatus>>>,
    /// T-UI-003: When `Some`, the main pane shows this diff (full-width) instead
    /// of the commit graph list.  Cleared when `selected` changes or on reload.
    pub main_diff: Option<MainDiffView>,
    /// ADR-0026: read-only compare mode shown in the inspector changed-files area.
    /// Cleared on selection change or reload to avoid stale path/diff state.
    pub compare_view: Option<CompareView>,
    /// T-UI-003: Scroll handle for the "main-diff-list" uniform_list.
    pub main_diff_scroll_handle: UniformListScrollHandle,
    /// Local branch names from the snapshot, ordered by name.
    /// Used to render the sidebar.  The first element of the tuple is the
    /// branch name; the second is whether it is the current HEAD branch.
    pub branches: Vec<(String, bool)>,
    /// When `Some`, the plan confirmation modal is visible.
    pub plan_modal: Option<CheckoutPlanModal>,
    /// When `Some`, the pull plan confirmation modal is visible (T-HT-003).
    pub pull_modal: Option<PullPlanModal>,
    /// When `Some`, the undo-commit confirmation modal is visible (T-HT-009).
    pub undo_modal: Option<UndoPlanModal>,
    /// When `Some`, the stash-pop confirmation modal is visible (T-HT-007).
    pub pop_modal: Option<PopPlanModal>,
    /// When `Some`, the push plan confirmation modal is visible (T-HT-004).
    pub push_modal: Option<PushPlanModal>,
    /// When `Some`, the create-branch modal is visible.
    pub create_branch_modal: Option<CreateBranchModal>,
    /// When `Some`, the create-worktree modal is visible.
    pub create_worktree_modal: Option<CreateWorktreeModal>,
    /// Focus handle used to receive keyboard events for the create-branch modal.
    /// Allocated on demand when the modal is first opened.
    pub modal_focus: Option<FocusHandle>,
    /// Stash entries from the snapshot, ordered by index (newest = index 0).
    pub stashes: Vec<Stash>,
    /// Whether the working tree is dirty (used to show/hide the Stash button).
    pub is_dirty: bool,
    /// When `Some`, the stash push confirmation modal is visible.
    pub stash_push_modal: Option<StashPushModal>,
    /// When `Some`, the stash apply confirmation modal is visible.
    pub stash_apply_modal: Option<StashApplyModal>,
    /// Focus handle for the stash push modal text input.
    pub stash_push_focus: Option<FocusHandle>,
    /// When `Some`, the cherry-pick plan modal is visible (T016).
    pub cherry_pick_modal: Option<CherryPickModal>,
    /// When `Some`, the revert plan modal is visible (T-CM-034).
    pub revert_modal: Option<RevertModal>,
    /// Status footer message (T017): the result of the most recent operation.
    pub status_footer: FooterStatus,
    /// Current sidebar width in pixels (T023: user-resizable).
    pub sidebar_width: f32,
    /// Current detail/diff panel width in pixels (T023: user-resizable).
    pub panel_width: f32,
    /// T030: Width of the badge (branch/tag) column in pixels.
    pub badge_col_w: f32,
    /// T030: Width of the graph column in pixels.
    pub graph_col_w: f32,
    // ── T-BP-002: Bottom Panel ───────────────────────────────────
    /// Whether the bottom panel is currently open.
    pub bottom_panel_open: bool,
    /// Current height of the bottom panel in pixels (clamped 80 .. viewport*0.6).
    pub bottom_panel_height: f32,
    /// Active tab in the bottom panel.
    pub bottom_tab: BottomTab,
    // ── T025: Commit Panel ───────────────────────────────────────
    /// Whether the commit panel is currently open (WIP row selected).
    pub commit_panel_open: bool,
    /// Commit panel state (staging lists, diff, message, modal).
    pub commit_panel: Option<CommitPanelState>,
    // ── T026: gpui-component Input for commit message (IME対応) ───
    /// InputState entity for the commit message field (gpui-component IME対応).
    /// Created lazily when the commit panel is first opened (requires &mut Window).
    pub commit_input: Option<Entity<InputState>>,
    // ── T028: branch jump (scroll to commit) ─────────────────
    /// Scroll handle for the "commit-list" uniform_list.
    /// Stored in KagiApp so it persists across render frames.
    pub commit_scroll_handle: UniformListScrollHandle,
    /// Maps local branch name → the CommitId it points to.
    /// Built at snapshot time; used by jump_to_branch.
    pub branch_targets: HashMap<String, CommitId>,
    /// Maps CommitId → row index in `self.rows`.
    /// Built at snapshot time; used by jump_to_branch.
    pub commit_row_index: HashMap<CommitId, usize>,
    // ── T-BP-003: StatusBar summary ──────────────────────────────
    /// Pre-computed status bar data (branch, ahead/behind, staged, unstaged).
    /// Updated on every reload; rendered by `render_status_bar`.
    pub status_summary: StatusBarSummary,
    // ── T-HT-001: Toolbar state ──────────────────────────────────
    /// Pre-computed toolbar button enabled/disabled flags.
    /// Updated on every reload; rendered by `render_header_slot`.
    pub toolbar_state: ToolbarState,
    // ── T-BP-004: Operation Log entries ─────────────────────────
    /// In-memory operation log ring-buffer (max 500, newest at index 0).
    pub op_entries: VecDeque<OpLogEntry>,
    /// Scroll handle for the Operation Log uniform_list.
    pub oplog_scroll_handle: UniformListScrollHandle,
    /// Which row index (0 = newest) is currently expanded; None = none.
    pub oplog_expanded: Option<usize>,
    // ── T-BP-007 / W4-TABS: Terminal sessions ────────────────────
    /// Terminal sessions keyed by repository path so each tab keeps its own
    /// live PTY across tab switches (W4-TABS / ADR-0027).  A session is created
    /// lazily when the Terminal tab is first displayed for a given repo.
    pub terminal_sessions: HashMap<PathBuf, terminal::KagiTerminalSession>,
    // ── W4-TABS: Repository tabs (ADR-0027) ──────────────────────
    /// Open repository tabs.  Empty → Welcome screen is shown.
    pub tabs: Vec<tabs::RepoTab>,
    /// Index of the active tab in `tabs` (meaningless when `tabs` is empty).
    pub active_tab: usize,
    /// Monotonic watcher generation.  Bumped on every switch/open/close so the
    /// previously-armed watcher loop detects a mismatch and terminates itself.
    pub watcher_generation: u64,
    // ── W2-GRAPH: Compact graph mode ────────────────────────────
    /// When `true` row height is 18px (compact); `false` (default) = 24px.
    pub graph_compact: bool,
    /// Horizontal scroll offset (px) of the graph column. Lanes hidden by a
    /// narrow column width are revealed by horizontal scrolling (clamped in
    /// render against the current lane count).
    pub graph_scroll_x: f32,
    // ── W2-INSPECTOR: Changed-files display mode ─────────────────
    /// When `true` the inspector shows files in tree view; `false` = flat path list.
    /// Default: `true`.
    pub inspector_tree_view: bool,
    /// W7-INSPECTOR2: vertical split ratio between the message scroll box (top)
    /// and the changed-files list (bottom) inside the inspector.  `0.5` = 1:1.
    /// Clamped to `0.2..=0.8` when dragged via the `InspectorSplit` divider.
    pub inspector_split: f32,
    /// Measured (top, bottom) window-px bounds of the inspector's
    /// message+files region, written by a paint-time canvas in inspector.rs.
    /// The drag handler maps cursor_y against these real bounds; static
    /// offsets cannot account for the variable-height header (caused a
    /// visible jump on drag start).
    pub inspector_geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    // ── W2-SIDEBAR: Repository Navigator ────────────────────────
    /// Remote-tracking branches from the snapshot (for REMOTE BRANCHES section).
    pub remote_branches: Vec<RemoteBranch>,
    /// Tags from the snapshot (for TAGS section).
    pub tags: Vec<Tag>,
    /// Worktrees from the snapshot (for WORKTREES section).
    pub worktrees: Vec<Worktree>,
    /// Upstream info per local branch name (for ↑A ↓B display).
    pub branch_upstream_info: HashMap<String, UpstreamInfo>,
    /// Collapsed sections in the sidebar (HashSet of section keys).
    /// Preserved across reloads so the user's collapse state survives checkout.
    pub sidebar_collapsed: HashSet<&'static str>,
    /// Lazy InputState for the sidebar filter input (gpui-component IME対応).
    /// Created on first click of the filter area (requires &mut Window).
    pub sidebar_filter: Option<Entity<InputState>>,
    // ── W3-NOTIFY: snackbar toasts + async-op state ──────────────
    /// Visible toast stack (bottom-right). Newest last.
    pub toasts: Vec<Toast>,
    /// Monotonic id source for toasts.
    pub next_toast_id: u64,
    /// True while the 500ms auto-dismiss ticker task is alive.
    pub toast_ticker_alive: bool,
    /// Debounce generation for modal live re-planning. Each input change
    /// bumps it; a 250ms timer task re-plans only if no newer change arrived.
    /// Per-keystroke synchronous re-planning (Repository::open + plan build,
    /// the stash modal even scans status) was the user-reported input lag.
    pub modal_replan_gen: u64,
    /// Name of the git operation currently running on a background thread
    /// (e.g. "pull"/"push"). While `Some`, toolbar git buttons are disabled
    /// and new plan modals are refused so operations never overlap.
    pub busy_op: Option<&'static str>,
    // ── W2-DELETE: Delete-branch modal ───────────────────────
    /// When `Some`, the delete-branch confirmation modal is visible.
    pub delete_branch_modal: Option<DeleteBranchModal>,
    /// Commit row context menu state (right-click anchor + target row).
    pub commit_menu: Option<CommitMenuState>,
    // ── W5-MENU: command registry / menu bar ─────────────────
    /// Whether the left sidebar (Repository Navigator) is shown (View → Toggle
    /// Sidebar).  Default `true`.
    pub sidebar_visible: bool,
    /// Whether the right commit-details inspector is shown (View → Toggle Commit
    /// Details).  Default `true`.
    pub inspector_visible: bool,
    /// Transient overlay opened from the menu bar (branch picker / About /
    /// Keyboard Shortcuts).  `None` when no menu overlay is visible.
    pub menu_overlay: Option<commands::MenuOverlay>,
    // ── W6-TABSPEED: async tab loading + stale-while-revalidate cache ──
    /// Cache of snapshot-derived display data keyed by repository path
    /// (ADR-0030).  A cached tab is applied instantly on switch (zero-frame
    /// swap) and then revalidated in the background.  Evicted in `close_tab`.
    pub tab_cache: HashMap<PathBuf, TabViewState>,
    /// Monotonic switch generation.  Bumped on every async tab switch so a
    /// stale background load (an earlier switch that lost a rapid-fire race)
    /// can detect a mismatch and discard its result before applying.
    pub switch_generation: u64,
    /// When `Some(name)`, the main pane shows a `Loading <name>…` placeholder
    /// (uncached first open) until the background load completes.
    pub loading_tab: Option<SharedString>,
    // ── W11-AVATAR: GitHub avatar images (ADR-0037) ──────────────
    /// Resolved avatar images keyed by author email.  Populated by a background
    /// resolution pass for GitHub repos; rows/inspector swap the initial circle
    /// for `img(...)` when an entry exists.  Memory cache (the disk cache lives
    /// under `~/.kagi/avatars/`).
    pub avatar_images: HashMap<String, std::sync::Arc<gpui::Image>>,
    /// Guard so avatar resolution runs at most once per repository path (avoids
    /// re-hitting the network on every reload / render).  Holds the repo path
    /// whose avatars have been (or are being) resolved.
    pub avatar_fetch_for: Option<PathBuf>,
}

/// W6-TABSPEED: snapshot-derived **pure data** for one repository tab.
///
/// This is the entire set of per-repo display fields that
/// [`KagiApp::from_snapshot`] computes from a [`RepoSnapshot`].  It contains
/// only owned, `Send` data (`SharedString`, `Vec`, `HashMap`, plain values) —
/// no `Entity`, `FocusHandle`, or `UniformListScrollHandle` — so it can be
/// built on a background thread (`cx.background_spawn`) and cached across tabs
/// (`tab_cache`).  [`build_tab_view`] is the pure, `Send` builder;
/// [`KagiApp::apply_tab_view`] does the main-thread assignment only.
#[derive(Clone)]
pub struct TabViewState {
    pub header: SharedString,
    pub rows: Vec<CommitRow>,
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
        Head::Detached { target } => format!(
            "detached: {}",
            target.get(..8).unwrap_or(target)
        ),
        Head::Unborn { branch } => format!("unborn ({branch})"),
    };

    let status = &snap.status;
    let status_label = if status.is_dirty() {
        let parts: Vec<String> = [
            (!status.staged.is_empty())
                .then(|| format!("{}S", status.staged.len())),
            (!status.unstaged.is_empty())
                .then(|| format!("{}M", status.unstaged.len())),
            (!status.untracked.is_empty())
                .then(|| format!("{}?", status.untracked.len())),
            (!status.conflicted.is_empty())
                .then(|| format!("{}!", status.conflicted.len())),
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

    let rows = build_commit_rows(snap);
    let details = build_commit_details(snap);

    // T009: log lane count derived from the first row (all rows share the same value).
    let lane_count = rows.first().map(|r| r.lane_count).unwrap_or(0);
    eprintln!("[kagi] graph: lane_count={}", lane_count);
    eprintln!("[kagi] commit list rows: {}", rows.len());

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
    }
}

impl KagiApp {
    /// Construct from a successful [`RepoSnapshot`].
    ///
    /// W6-TABSPEED: the snapshot-derived display data is now produced by the
    /// pure [`build_tab_view`] free function; this constructor just folds that
    /// `TabViewState` into a fresh `KagiApp` together with the non-snapshot
    /// (handle / modal / preference) defaults.  Behaviour and log output are
    /// identical to the previous inline version.
    pub fn from_snapshot(repo_name: &str, snap: &RepoSnapshot) -> Self {
        let view = build_tab_view(snap, repo_name);

        // T-BP-004: load up to 100 entries from the oplog file at startup.
        let op_entries: VecDeque<OpLogEntry> = read_oplog_tail(OP_ENTRIES_LOAD).into();

        let TabViewState {
            header,
            rows,
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
            worktrees,
        } = view;

        KagiApp {
            root_focus: None,
            header,
            rows,
            details,
            selected: None,
            error: None,
            repo_path: None,
            diff_cache: HashMap::new(),
            main_diff: None,
            compare_view: None,
            main_diff_scroll_handle: UniformListScrollHandle::new(),
            branches,
            plan_modal: None,
            pull_modal: None,
            undo_modal: None,
            pop_modal: None,
            push_modal: None,
            create_branch_modal: None,
            create_worktree_modal: None,
            modal_focus: None,
            stashes,
            is_dirty,
            stash_push_modal: None,
            stash_apply_modal: None,
            stash_push_focus: None,
            cherry_pick_modal: None,
            revert_modal: None,
            status_footer: FooterStatus::Idle(SharedString::from("Ready")),
            sidebar_width: SIDEBAR_DEFAULT,
            panel_width: PANEL_DEFAULT,
            badge_col_w: BADGE_COL_DEFAULT,
            graph_col_w: GRAPH_COL_DEFAULT,
            bottom_panel_open: false,
            bottom_panel_height: BOTTOM_PANEL_H_UNSET,
            bottom_tab: BottomTab::OperationLog,
            commit_panel_open: false,
            commit_panel: None,
            commit_input: None,
            commit_scroll_handle: UniformListScrollHandle::new(),
            branch_targets,
            commit_row_index,
            status_summary,
            toolbar_state,
            op_entries,
            oplog_scroll_handle: UniformListScrollHandle::new(),
            oplog_expanded: None,
            terminal_sessions: HashMap::new(),
            tabs: Vec::new(),
            active_tab: 0,
            watcher_generation: 0,
            inspector_tree_view: true,
            inspector_split: INSPECTOR_SPLIT_DEFAULT,
            inspector_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            graph_compact: false,
            graph_scroll_x: 0.0,
            // W2-SIDEBAR
            remote_branches,
            tags,
            worktrees,
            branch_upstream_info,
            sidebar_collapsed: HashSet::new(),
            sidebar_filter: None,
            // W3-NOTIFY
            toasts: Vec::new(),
            next_toast_id: 0,
            toast_ticker_alive: false,
            busy_op: None,
            modal_replan_gen: 0,
            // W2-DELETE
            delete_branch_modal: None,
            commit_menu: None,
            // W5-MENU
            sidebar_visible: true,
            inspector_visible: true,
            menu_overlay: None,
            // W6-TABSPEED
            tab_cache: HashMap::new(),
            switch_generation: 0,
            loading_tab: None,
            // W11-AVATAR
            avatar_images: HashMap::new(),
            avatar_fetch_for: None,
        }
    }

    /// Construct a placeholder for the no-argument / error case.
    pub fn with_error(message: impl Into<String>) -> Self {
        KagiApp {
            root_focus: None,
            header: SharedString::from("kagi"),
            rows: Vec::new(),
            details: Vec::new(),
            selected: None,
            error: Some(SharedString::from(message.into())),
            repo_path: None,
            diff_cache: HashMap::new(),
            main_diff: None,
            compare_view: None,
            main_diff_scroll_handle: UniformListScrollHandle::new(),
            branches: Vec::new(),
            plan_modal: None,
            pull_modal: None,
            undo_modal: None,
            pop_modal: None,
            push_modal: None,
            create_branch_modal: None,
            create_worktree_modal: None,
            modal_focus: None,
            stashes: Vec::new(),
            is_dirty: false,
            stash_push_modal: None,
            stash_apply_modal: None,
            stash_push_focus: None,
            cherry_pick_modal: None,
            revert_modal: None,
            status_footer: FooterStatus::Idle(SharedString::from("Ready")),
            sidebar_width: SIDEBAR_DEFAULT,
            panel_width: PANEL_DEFAULT,
            badge_col_w: BADGE_COL_DEFAULT,
            graph_col_w: GRAPH_COL_DEFAULT,
            bottom_panel_open: false,
            bottom_panel_height: BOTTOM_PANEL_H_UNSET,
            bottom_tab: BottomTab::OperationLog,
            commit_panel_open: false,
            commit_panel: None,
            commit_input: None,
            commit_scroll_handle: UniformListScrollHandle::new(),
            branch_targets: HashMap::new(),
            commit_row_index: HashMap::new(),
            status_summary: StatusBarSummary::default(),
            toolbar_state: ToolbarState::default(),
            op_entries: VecDeque::new(),
            oplog_scroll_handle: UniformListScrollHandle::new(),
            oplog_expanded: None,
            terminal_sessions: HashMap::new(),
            tabs: Vec::new(),
            active_tab: 0,
            watcher_generation: 0,
            inspector_tree_view: true,
            inspector_split: INSPECTOR_SPLIT_DEFAULT,
            inspector_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            graph_compact: false,
            graph_scroll_x: 0.0,
            // W2-SIDEBAR
            remote_branches: Vec::new(),
            tags: Vec::new(),
            worktrees: Vec::new(),
            branch_upstream_info: HashMap::new(),
            sidebar_collapsed: HashSet::new(),
            sidebar_filter: None,
            // W3-NOTIFY
            toasts: Vec::new(),
            next_toast_id: 0,
            toast_ticker_alive: false,
            busy_op: None,
            modal_replan_gen: 0,
            // W2-DELETE
            delete_branch_modal: None,
            commit_menu: None,
            // W5-MENU
            sidebar_visible: true,
            inspector_visible: true,
            menu_overlay: None,
            // W6-TABSPEED
            tab_cache: HashMap::new(),
            switch_generation: 0,
            loading_tab: None,
            // W11-AVATAR
            avatar_images: HashMap::new(),
            avatar_fetch_for: None,
        }
    }

    /// Reload all display data from the repository at `repo_path`.
    ///
    /// Called after a successful checkout to update the commit list, header,
    /// branch list, and badges without restarting the application.
    pub fn reload(&mut self) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        // Re-open and snapshot.
        let mut repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] reload: repo open error: {}", e.message());
                return;
            }
        };
        let snap = match kagi::git::snapshot(&mut repo, 10_000) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[kagi] reload: snapshot error: {}", e);
                return;
            }
        };

        // Derive repo name from path.
        let repo_name = repo_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| repo_path.display().to_string());

        // W6-TABSPEED: rebuild the pure display data (same log output as before),
        // reset per-repo transient UI state, then fold the view in via
        // `apply_tab_view`.  ADR-0030 §5: reload() also refreshes the cache.
        let view = build_tab_view(&snap, &repo_name);

        // Per-repo transient state reset (unchanged behaviour).
        self.selected = None;
        self.diff_cache = HashMap::new();
        self.main_diff = None;
        self.compare_view = None;
        self.plan_modal = None;
        self.pull_modal = None;
        self.undo_modal = None;
        self.pop_modal = None;
        self.create_branch_modal = None;
        self.create_worktree_modal = None;
        self.modal_focus = None;
        self.stash_push_modal = None;
        self.stash_apply_modal = None;
        self.stash_push_focus = None;
        self.cherry_pick_modal = None;
        self.revert_modal = None;
        self.commit_menu = None;
        // T025/T026: reset commit panel and input so it reflects fresh status after reload.
        self.commit_panel_open = false;
        self.commit_panel = None;
        self.commit_input = None;
        // commit_scroll_handle is preserved so the existing Rc<RefCell<...>> reference
        // wired into the uniform_list continues to work after reload.
        // status_footer is intentionally preserved across reloads so the last
        // operation result remains visible after the commit list refreshes.
        // sidebar_width / panel_width are also preserved so the user's resize
        // is not lost on checkout/reload (T023).
        // T-BP-004: op_entries, oplog_scroll_handle, oplog_expanded are preserved
        // so the Operation Log keeps its contents across repository reloads.
        // sidebar_collapsed / sidebar_filter are preserved so the user's
        // collapse + filter state survives reload.

        // ADR-0030 §5: keep the stale-while-revalidate cache fresh.
        self.tab_cache.insert(repo_path.clone(), view.clone());

        // Fold the snapshot-derived data in (assignment only).
        self.apply_tab_view(view);
    }

    /// W6-TABSPEED: assign a [`TabViewState`] into `self` (main thread, no I/O).
    ///
    /// This is pure field assignment — the snapshot read + `build_tab_view`
    /// happens elsewhere (inline in `reload`, or on a background thread for
    /// async tab switches).  It deliberately does *not* touch transient UI
    /// state (selection / modals / panels); callers reset those as needed.
    pub fn apply_tab_view(&mut self, view: TabViewState) {
        self.header = view.header;
        self.rows = view.rows;
        self.details = view.details;
        self.branches = view.branches;
        self.stashes = view.stashes;
        self.is_dirty = view.is_dirty;
        // T028: refresh branch/commit lookup maps so jump works after checkout.
        self.branch_targets = view.branch_targets;
        self.commit_row_index = view.commit_row_index;
        // T-BP-003 / T-HT-001: update StatusBarSummary + ToolbarState
        // (already logged by build_tab_view).
        self.status_summary = view.status_summary;
        self.toolbar_state = view.toolbar_state;
        // W2-SIDEBAR: refresh remote_branches, tags, and upstream info.
        self.remote_branches = view.remote_branches;
        self.tags = view.tags;
        self.worktrees = view.worktrees;
        self.branch_upstream_info = view.branch_upstream_info;
    }

    /// Reload triggered by an external git change (T029: FS watcher).
    ///
    /// Behaves identically to `reload()` but additionally:
    /// - Emits the required `[kagi] refreshed (external change)` log line.
    /// - Updates the status footer to show the refresh message.
    /// - Attempts to re-select the previously selected commit by CommitId;
    ///   if the commit no longer exists the selection is cleared.
    pub fn reload_external(&mut self, cx: &mut Context<Self>) {
        // Capture the CommitId of the currently selected row (if any) so we
        // can attempt to re-select it after the snapshot is refreshed.
        // `details[idx].full_sha` is the canonical commit hash string.
        let prev_commit_id: Option<CommitId> = self.selected
            .and_then(|idx| self.details.get(idx))
            .map(|detail| CommitId(detail.full_sha.to_string()));

        // Delegate to the core reload logic (resets self.selected to None).
        self.reload();

        // Attempt to restore selection by CommitId.
        if let Some(ref cid) = prev_commit_id {
            if let Some(&new_idx) = self.commit_row_index.get(cid) {
                self.selected = Some(new_idx);
            }
            // If the commit is no longer present, selected stays None.
        }

        // Emit the required log line and update the footer.
        eprintln!("[kagi] refreshed (external change)");
        self.status_footer = FooterStatus::Idle(
            SharedString::from("[kagi] refreshed (external change)")
        );

        // Notify gpui that state has changed so the window repaints.
        cx.notify();
    }

    /// W11-AVATAR (ADR-0037): start GitHub avatar resolution for the current
    /// repo, at most once per repository path.
    ///
    /// Resolution runs entirely on a background thread (`cx.background_spawn`):
    /// it determines the GitHub `(owner, repo)` from the repo's remotes, then
    /// resolves each distinct author email to an avatar image (noreply parse →
    /// Commits API batch → disk/network fetch).  When it completes the resolved
    /// images are merged into `self.avatar_images` on the main thread and a
    /// `cx.notify()` repaints rows/inspector with real avatars.
    ///
    /// No-op for non-GitHub repos, `KAGI_OFFLINE=1`, or a repo already started.
    /// The required startup log line is emitted exactly once per repo.
    fn ensure_avatars(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else { return };

        // Run at most once per repository path.
        if self.avatar_fetch_for.as_deref() == Some(repo_path.as_path()) {
            return;
        }
        self.avatar_fetch_for = Some(repo_path.clone());

        // Distinct author emails across the loaded commit rows.
        let mut seen: HashSet<String> = HashSet::new();
        let mut emails: Vec<String> = Vec::new();
        for row in &self.rows {
            if !row.author_email.is_empty() && seen.insert(row.author_email.clone()) {
                emails.push(row.author_email.clone());
            }
        }

        let offline = avatar_fetch::offline();

        // Determine GitHub coordinates (read-only git2). Non-GitHub repos get
        // the initial circle and emit a pending-only log line.
        let coords = avatar_fetch::repo_github_coords(&repo_path);
        let Some((owner, repo)) = coords else {
            eprintln!(
                "[kagi] avatar: resolved=0 pending={} offline={}",
                emails.len(),
                offline
            );
            return;
        };

        let task = cx.background_spawn(async move {
            avatar_fetch::resolve_avatars(&owner, &repo, &emails)
        });
        cx.spawn(async move |this, acx| {
            let outcome = task.await;
            let _ = this.update(acx, |app, cx| {
                for (email, img) in outcome.images {
                    app.avatar_images.insert(email, img);
                }
                eprintln!(
                    "[kagi] avatar: resolved={} pending={} offline={}",
                    outcome.resolved, outcome.pending, offline
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// Open the checkout plan modal for `branch`.
    ///
    /// Plans the checkout using the current repository state and stores the
    /// result in `self.plan_modal`.  Emits a plan log entry.
    pub fn open_plan_modal(&mut self, branch: impl Into<String>) {
        let branch = branch.into();
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                eprintln!("[kagi] open_plan_modal: no repo_path set");
                return;
            }
        };

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] plan: repo open error: {}", e.message());
                return;
            }
        };

        match plan_checkout(&repo, &branch) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: checkout {} blockers={} warnings={}",
                    branch,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.plan_modal = Some(CheckoutPlanModal {
                    target: CheckoutPlanTarget::Branch(branch.clone()),
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                eprintln!("[kagi] plan: error: {}", e);
            }
        }
    }

    /// Open the detached checkout plan modal for commit `commit_id`.
    pub fn open_checkout_commit_modal(&mut self, commit_id: CommitId) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                eprintln!("[kagi] open_checkout_commit_modal: no repo_path set");
                return;
            }
        };

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] checkout-commit plan: repo open error: {}", e.message());
                return;
            }
        };

        match plan_checkout_commit(&repo, &commit_id) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: checkout-commit {} blockers={} warnings={}",
                    commit_id.short(),
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.plan_modal = Some(CheckoutPlanModal {
                    target: CheckoutPlanTarget::Commit(commit_id),
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                eprintln!("[kagi] checkout-commit plan: error: {}", e);
            }
        }
    }

    /// Cancel and close the checkout plan modal without making any changes.
    pub fn cancel_modal(&mut self) {
        self.plan_modal = None;
    }

    // ── Create-branch modal (T014) ───────────────────────────

    /// Open the create-branch modal for the commit at `at`.
    ///
    /// The input is initially empty; the live plan will show a "name is empty"
    /// blocker until the user types a valid name.
    pub fn open_create_branch_modal(&mut self, at: CommitId, cx: &mut Context<Self>) {
        // Allocate a focus handle if we don't have one yet.
        if self.modal_focus.is_none() {
            self.modal_focus = Some(cx.focus_handle());
        }
        let start_title = self.commit_title_for(&at);
        self.create_branch_modal = Some(CreateBranchModal {
            at,
            start_title,
            input: String::new(),
            input_state: None, // created lazily on first render (needs Window)
            checkout_after: false,
            plan: None,
            error: None,
        });
        // Re-plan immediately (empty name → blocker).
        self.replan_create_branch();
    }

    fn commit_title_for(&self, at: &CommitId) -> String {
        self.row_for_commit_id(at)
            .and_then(|idx| self.details.get(idx))
            .map(|detail| {
                detail
                    .full_message
                    .as_ref()
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string()
            })
            .unwrap_or_default()
    }

    /// Close the create-branch modal without making any changes.
    pub fn cancel_create_branch_modal(&mut self) {
        self.create_branch_modal = None;
    }

    /// Re-generate the live plan from the current modal input.
    fn replan_create_branch(&mut self) {
        let (at, name, checkout_after) = match self.create_branch_modal.as_ref() {
            Some(m) => (m.at.clone(), m.input.clone(), m.checkout_after),
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] replan_create_branch: repo open error: {}", e.message());
                return;
            }
        };
        match plan_create_branch_with_checkout(&repo, &name, &at, checkout_after) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: create-branch '{}' checkout_after={} blockers={} warnings={}",
                    name,
                    checkout_after,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                if let Some(ref mut modal) = self.create_branch_modal {
                    modal.plan = Some(std::sync::Arc::new(plan));
                }
            }
            Err(e) => {
                eprintln!("[kagi] plan: create-branch error: {}", e);
            }
        }
    }

    /// Confirm the create-branch plan: run preflight, execute, then reload.
    ///
    /// On failure the modal remains open and shows the error text.
    pub fn confirm_create_branch(&mut self) {
        // The live plan is debounced; rebuild it from the latest input so a
        // fast type-then-click can never execute a stale plan.
        self.run_modal_replans();
        let modal = match self.create_branch_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let plan = match modal.plan.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        // Defence in depth: refuse if blockers exist.
        if !plan.blockers.is_empty() {
            eprintln!("[kagi] refused: create-branch plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "create-branch",
                    plan.current.clone(),
                    OpOutcome::Refused { blockers: plan.blockers.clone() },
                    rp,
                );
            }
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e.message());
                self.record_op(
                    "create-branch",
                    plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                if let Some(ref mut m) = self.create_branch_modal {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
        };

        // Preflight check (re-use checkout preflight: verifies HEAD unchanged).
        if let Err(e) = preflight_check(&repo, &plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "create-branch",
                plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            if let Some(ref mut m) = self.create_branch_modal {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        // Execute create-branch.
        if let Err(e) = execute_create_branch(&repo, &modal.input, &modal.at) {
            let err_msg = format!("Create branch failed: {}", e);
            self.record_op(
                "create-branch",
                plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            if let Some(ref mut m) = self.create_branch_modal {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        eprintln!("[kagi] executed: create-branch '{}' @ {}", modal.input, modal.at.short());

        // Verify: confirm the branch now exists.
        let repo2 = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] verify: repo open error: {}", e.message());
                self.reload();
                return;
            }
        };
        let branch_exists = repo2
            .find_branch(&modal.input, git2::BranchType::Local)
            .is_ok();
        if branch_exists {
            eprintln!("[kagi] verified: branch '{}' exists", modal.input);
        } else {
            eprintln!("[kagi] verify: branch '{}' NOT found after create", modal.input);
        }

        // Record branch creation success first. If checkout_after is on, the
        // checkout below records its own second operation entry.
        let create_after = StateSummary {
            head: plan.current.head.clone(),
            dirty: plan.current.dirty.clone(),
        };
        self.record_op(
            "create-branch",
            plan.current.clone(),
            OpOutcome::Success { after: create_after.clone() },
            &repo_path,
        );

        if modal.checkout_after {
            let checkout_plan = match plan_checkout(&repo2, &modal.input) {
                Ok(plan) => plan,
                Err(e) => {
                    let err_msg = format!("Checkout plan failed after branch creation: {}", e);
                    self.record_op(
                        "checkout",
                        create_after,
                        OpOutcome::Failed { error: err_msg.clone() },
                        &repo_path,
                    );
                    if let Some(ref mut m) = self.create_branch_modal {
                        m.error = Some(SharedString::from(err_msg));
                    }
                    return;
                }
            };
            if !checkout_plan.blockers.is_empty() {
                self.record_op(
                    "checkout",
                    checkout_plan.current.clone(),
                    OpOutcome::Refused { blockers: checkout_plan.blockers.clone() },
                    &repo_path,
                );
                if let Some(ref mut m) = self.create_branch_modal {
                    m.error = Some(SharedString::from(
                        "Branch created, but checkout was refused by the checkout plan.",
                    ));
                }
                return;
            }
            if let Err(e) = preflight_check(&repo2, &checkout_plan) {
                let err_msg = format!("Checkout preflight failed: {}", e);
                self.record_op(
                    "checkout",
                    checkout_plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                if let Some(ref mut m) = self.create_branch_modal {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
            if let Err(e) = execute_checkout(&repo2, &modal.input) {
                let err_msg = format!("Checkout failed: {}", e);
                self.record_op(
                    "checkout",
                    checkout_plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                if let Some(ref mut m) = self.create_branch_modal {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
            eprintln!("[kagi] executed: checkout {}", modal.input);
            self.record_op(
                "checkout",
                checkout_plan.current.clone(),
                OpOutcome::Success { after: checkout_plan.predicted.clone() },
                &repo_path,
            );
        }

        // Reload display data (new branch badge should appear).
        self.reload();
    }

    // ── Create-worktree modal (T-CM-023) ─────────────────────

    pub fn open_create_worktree_modal(&mut self, at: CommitId, cx: &mut Context<Self>) {
        if self.modal_focus.is_none() {
            self.modal_focus = Some(cx.focus_handle());
        }
        let start_title = self.commit_title_for(&at);
        let branch_input = String::new();
        let path_input = self.default_worktree_path("new-branch");
        self.create_worktree_modal = Some(CreateWorktreeModal {
            at,
            start_title,
            branch_input,
            branch_state: None, // lazy (render)
            path_input,
            path_state: None, // lazy (render)
            path_touched: false,
            active_field: WorktreeModalField::Branch,
            plan: None,
            error: None,
        });
        self.replan_create_worktree();
    }

    pub fn cancel_create_worktree_modal(&mut self) {
        self.create_worktree_modal = None;
    }

    fn default_worktree_path(&self, branch: &str) -> String {
        let repo_path = match self.repo_path.as_ref() {
            Some(path) => path,
            None => return String::new(),
        };
        let repo_name = repo_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("repo");
        let safe_branch: String = branch
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                    ch
                } else {
                    '-'
                }
            })
            .collect();
        let safe_branch = if safe_branch.is_empty() {
            "new-branch".to_string()
        } else {
            safe_branch
        };
        format!("../{}-worktrees/{}", repo_name, safe_branch)
    }

    fn replan_create_worktree(&mut self) {
        let (at, branch, path) = match self.create_worktree_modal.as_ref() {
            Some(m) => (m.at.clone(), m.branch_input.clone(), m.path_input.clone()),
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] replan_create_worktree: repo open error: {}", e.message());
                return;
            }
        };
        match plan_create_worktree(&repo, &branch, &path, &at) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: create-worktree '{}' path='{}' blockers={} warnings={}",
                    branch,
                    path,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                if let Some(ref mut modal) = self.create_worktree_modal {
                    modal.plan = Some(std::sync::Arc::new(plan));
                }
            }
            Err(e) => {
                eprintln!("[kagi] plan: create-worktree error: {}", e);
            }
        }
    }

    pub fn confirm_create_worktree(&mut self) {
        // The live plan is debounced; rebuild it from the latest input so a
        // fast type-then-click can never execute a stale plan.
        self.run_modal_replans();
        let modal = match self.create_worktree_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let plan = match modal.plan.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        if !plan.blockers.is_empty() {
            eprintln!("[kagi] refused: create-worktree plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "create-worktree",
                    plan.current.clone(),
                    OpOutcome::Refused { blockers: plan.blockers.clone() },
                    rp,
                );
            }
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e.message());
                self.record_op(
                    "create-worktree",
                    plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                if let Some(ref mut m) = self.create_worktree_modal {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
        };
        if let Err(e) = preflight_check(&repo, &plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "create-worktree",
                plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            if let Some(ref mut m) = self.create_worktree_modal {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }
        if let Err(e) = execute_create_worktree(&repo, &modal.branch_input, &modal.path_input, &modal.at) {
            let err_msg = format!("Create worktree failed: {}", e);
            self.record_op(
                "create-worktree",
                plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            if let Some(ref mut m) = self.create_worktree_modal {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        eprintln!(
            "[kagi] executed: create-worktree '{}' path='{}' @ {}",
            modal.branch_input,
            modal.path_input,
            modal.at.short()
        );
        let verify_path = {
            let path = std::path::PathBuf::from(&modal.path_input);
            if path.is_absolute() {
                path
            } else {
                repo_path.join(path)
            }
        };
        match git2::Repository::open(&verify_path) {
            Ok(linked) => {
                let head = linked
                    .head()
                    .ok()
                    .and_then(|h| h.shorthand().ok().map(|s| s.to_string()));
                eprintln!(
                    "[kagi] verified: worktree '{}' HEAD={}",
                    verify_path.display(),
                    head.unwrap_or_else(|| "?".to_string())
                );
            }
            Err(e) => {
                eprintln!("[kagi] verify: worktree open error: {}", e.message());
            }
        }
        self.record_op(
            "create-worktree",
            plan.current.clone(),
            OpOutcome::Success { after: plan.predicted.clone() },
            &repo_path,
        );
        self.reload();
    }

    // ── Stash push modal (T015) ──────────────────────────────

    /// Open the stash push modal.
    ///
    /// Plans the stash push immediately and stores the result in
    /// `self.stash_push_modal`.  The input is initially empty (no message).
    pub fn open_stash_push_modal(&mut self, cx: &mut Context<Self>) {
        if self.stash_push_focus.is_none() {
            self.stash_push_focus = Some(cx.focus_handle());
        }
        self.stash_push_modal = Some(StashPushModal {
            input: String::new(),
            input_state: None, // lazy (render)
            plan: None,
            error: None,
        });
        self.replan_stash_push();
    }

    /// Close the stash push modal without making any changes.
    pub fn cancel_stash_push_modal(&mut self) {
        self.stash_push_modal = None;
    }

    /// Re-generate the live stash push plan from the current input.
    fn replan_stash_push(&mut self) {
        let message_str = match self.stash_push_modal.as_ref() {
            Some(m) => m.input.clone(),
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let mut repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] replan_stash_push: repo open error: {}", e.message());
                return;
            }
        };
        let msg_opt = if message_str.is_empty() { None } else { Some(message_str.as_str()) };
        match plan_stash_push(&mut repo, msg_opt, true) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: stash-push blockers={} warnings={}",
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                if let Some(ref mut modal) = self.stash_push_modal {
                    modal.plan = Some(std::sync::Arc::new(plan));
                }
            }
            Err(e) => {
                eprintln!("[kagi] plan: stash-push error: {}", e);
            }
        }
    }

    /// Confirm the stash push plan: run preflight, execute, then reload.
    ///
    /// On failure the modal remains open and shows the error text.
    pub fn confirm_stash_push(&mut self) {
        // The live plan is debounced; rebuild it from the latest input so a
        // fast type-then-click can never execute a stale plan.
        self.run_modal_replans();
        let modal = match self.stash_push_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let plan = match modal.plan.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        // Defence in depth: refuse if blockers exist.
        if !plan.blockers.is_empty() {
            eprintln!("[kagi] refused: stash-push plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "stash-push",
                    plan.current.clone(),
                    OpOutcome::Refused { blockers: plan.blockers.clone() },
                    rp,
                );
            }
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        let mut repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e.message());
                self.record_op(
                    "stash-push",
                    plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                if let Some(ref mut m) = self.stash_push_modal {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
        };

        // Preflight check (HEAD + stash count).
        if let Err(e) = preflight_check_stash(&mut repo, &plan, plan.stash_count_at_plan()) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "stash-push",
                plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            if let Some(ref mut m) = self.stash_push_modal {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        let msg_opt: Option<&str> = if modal.input.is_empty() { None } else { Some(modal.input.as_str()) };

        // Execute stash push.
        if let Err(e) = execute_stash_push(&mut repo, msg_opt, true) {
            let err_msg = format!("Stash push failed: {}", e);
            self.record_op(
                "stash-push",
                plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            if let Some(ref mut m) = self.stash_push_modal {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        eprintln!("[kagi] executed: stash-push message={:?}", modal.input);

        // Verify: check working tree is now clean and stash count increased.
        let mut repo2 = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] verify: repo open error: {}", e.message());
                self.reload();
                return;
            }
        };
        let after_summary = match kagi::git::snapshot(&mut repo2, 10_000) {
            Ok(snap) => {
                let is_clean = !snap.status.is_dirty();
                let stash_count = snap.stashes.len();
                if is_clean {
                    eprintln!("[kagi] verified: working tree clean after stash-push");
                } else {
                    eprintln!("[kagi] verify: working tree NOT clean after stash-push");
                }
                eprintln!("[kagi] verified: stash count={}", stash_count);
                StateSummary {
                    head: snap.head.display(),
                    dirty: if is_clean { "clean".to_string() } else { "dirty".to_string() },
                }
            }
            Err(e) => {
                eprintln!("[kagi] verify: snapshot error: {}", e);
                plan.predicted.clone()
            }
        };

        // Record success to oplog + update footer.
        self.record_op(
            "stash-push",
            plan.current.clone(),
            OpOutcome::Success { after: after_summary },
            &repo_path,
        );

        // Reload display data.
        self.reload();
    }

    // ── Stash apply modal (T015) ─────────────────────────────

    /// Open the stash apply modal for stash entry at `index`.
    ///
    /// Plans the apply using the current repository state and stores the result.
    pub fn open_stash_apply_modal(&mut self, index: usize) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                eprintln!("[kagi] open_stash_apply_modal: no repo_path set");
                return;
            }
        };

        let mut repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] plan: stash-apply repo open error: {}", e.message());
                return;
            }
        };

        match plan_stash_apply(&mut repo, index) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: stash-apply index={} blockers={} warnings={}",
                    index,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.stash_apply_modal = Some(StashApplyModal {
                    index,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                eprintln!("[kagi] plan: stash-apply error: {}", e);
            }
        }
    }

    /// Close the stash apply modal without making any changes.
    pub fn cancel_stash_apply_modal(&mut self) {
        self.stash_apply_modal = None;
    }

    /// Confirm the stash apply plan: run preflight, execute, then reload.
    ///
    /// On failure the modal remains open and shows the error text.
    /// The stash entry is **never** removed (apply, not pop).
    pub fn confirm_stash_apply(&mut self) {
        let modal = match self.stash_apply_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let plan = modal.plan.clone();
        // Defence in depth: refuse if blockers exist.
        if !plan.blockers.is_empty() {
            eprintln!("[kagi] refused: stash-apply plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "stash-apply",
                    plan.current.clone(),
                    OpOutcome::Refused { blockers: plan.blockers.clone() },
                    rp,
                );
            }
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        let mut repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e.message());
                self.record_op(
                    "stash-apply",
                    plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                if let Some(ref mut m) = self.stash_apply_modal {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
        };

        // Preflight check (HEAD + stash count).
        if let Err(e) = preflight_check_stash(&mut repo, &plan, plan.stash_count_at_plan()) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "stash-apply",
                plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            if let Some(ref mut m) = self.stash_apply_modal {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        // Execute stash apply (apply only — no pop, no drop).
        if let Err(e) = execute_stash_apply(&mut repo, modal.index) {
            let err_msg = format!("Stash apply failed: {}", e);
            self.record_op(
                "stash-apply",
                plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            if let Some(ref mut m) = self.stash_apply_modal {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        eprintln!("[kagi] executed: stash-apply index={}", modal.index);

        // Verify: check working tree is dirty and stash entry still exists.
        let mut repo2 = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] verify: repo open error: {}", e.message());
                self.reload();
                return;
            }
        };
        let after_summary = match kagi::git::snapshot(&mut repo2, 10_000) {
            Ok(snap) => {
                let is_dirty = snap.status.is_dirty();
                let stash_count = snap.stashes.len();
                if is_dirty {
                    eprintln!("[kagi] verified: working tree dirty (stash applied)");
                } else {
                    eprintln!("[kagi] verify: working tree NOT dirty after stash-apply");
                }
                // Stash must remain (apply, not pop).
                if stash_count >= plan.stash_count_at_plan() {
                    eprintln!("[kagi] verified: stash count={} (entry preserved)", stash_count);
                } else {
                    eprintln!("[kagi] verify: stash count={} (expected >= {})", stash_count, plan.stash_count_at_plan());
                }
                StateSummary {
                    head: snap.head.display(),
                    dirty: if is_dirty { "dirty".to_string() } else { "clean".to_string() },
                }
            }
            Err(e) => {
                eprintln!("[kagi] verify: snapshot error: {}", e);
                plan.predicted.clone()
            }
        };

        // Record success to oplog + update footer.
        self.record_op(
            "stash-apply",
            plan.current.clone(),
            OpOutcome::Success { after: after_summary },
            &repo_path,
        );

        // Reload display data.
        self.reload();
    }

    // ── Cherry-pick modal (T016) ─────────────────────────────

    /// Open the cherry-pick plan modal for commit `id`.
    ///
    /// Plans the cherry-pick using the current repository state (in-memory,
    /// no working-tree modification) and stores the result in
    /// `self.cherry_pick_modal`.  Emits a plan log entry.
    pub fn open_cherry_pick_modal(&mut self, commit_id: CommitId) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                eprintln!("[kagi] open_cherry_pick_modal: no repo_path set");
                return;
            }
        };

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] cherry-pick plan: repo open error: {}", e.message());
                return;
            }
        };

        match plan_cherry_pick(&repo, &commit_id) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: cherry-pick {} blockers={} preview_files={}",
                    commit_id.short(),
                    plan.blockers.len(),
                    plan.preview_files.len()
                );
                self.cherry_pick_modal = Some(CherryPickModal {
                    commit_id,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                eprintln!("[kagi] cherry-pick plan: error: {}", e);
            }
        }
    }

    /// Cancel and close the cherry-pick modal without making any changes.
    pub fn cancel_cherry_pick_modal(&mut self) {
        self.cherry_pick_modal = None;
    }

    /// Confirm the cherry-pick plan: run preflight, execute, then reload.
    ///
    /// On failure the modal remains open and shows the error text.
    pub fn confirm_cherry_pick(&mut self) {
        let modal = match self.cherry_pick_modal.clone() {
            Some(m) => m,
            None => return,
        };
        // Defence in depth: refuse if blockers exist.
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: cherry-pick plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "cherry-pick",
                    modal.plan.current.clone(),
                    OpOutcome::Refused { blockers: modal.plan.blockers.clone() },
                    rp,
                );
            }
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e.message());
                self.record_op(
                    "cherry-pick",
                    modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                if let Some(ref mut m) = self.cherry_pick_modal {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
        };

        // Preflight check (HEAD unchanged since planning).
        if let Err(e) = preflight_check(&repo, &modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "cherry-pick",
                modal.plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            if let Some(ref mut m) = self.cherry_pick_modal {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        // Execute cherry-pick (in-memory index → commit → checkout_head safe).
        match execute_cherry_pick(&repo, &modal.commit_id) {
            Ok(new_id) => {
                eprintln!(
                    "[kagi] executed: cherry-pick {} -> {}",
                    modal.commit_id.short(),
                    new_id.short()
                );

                // Verify: re-snapshot, check HEAD is a new commit.
                let mut repo2 = match git2::Repository::open(&repo_path) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("[kagi] verify: repo open error: {}", e.message());
                        self.reload();
                        return;
                    }
                };
                let after_summary = match kagi::git::snapshot(&mut repo2, 10_000) {
                    Ok(snap) => {
                        if let Head::Attached { target, branch } = &snap.head {
                            if *target == new_id.0 {
                                eprintln!("[kagi] verified: cherry-pick HEAD={} on {}", new_id.short(), branch);
                            } else {
                                eprintln!("[kagi] verify: HEAD={} expected {}", &target[..8.min(target.len())], new_id.short());
                            }
                            let is_clean = !snap.status.is_dirty();
                            eprintln!("[kagi] verified: working tree {}", if is_clean { "clean" } else { "dirty (unexpected)" });
                        }
                        StateSummary {
                            head: snap.head.display(),
                            dirty: if snap.status.is_dirty() { "dirty".to_string() } else { "clean".to_string() },
                        }
                    }
                    Err(e) => {
                        eprintln!("[kagi] verify: snapshot error: {}", e);
                        modal.plan.predicted.clone()
                    }
                };

                // Record success to oplog + update footer.
                self.record_op(
                    "cherry-pick",
                    modal.plan.current.clone(),
                    OpOutcome::Success { after: after_summary },
                    &repo_path,
                );
            }
            Err(e) => {
                let err_msg = format!("Cherry-pick failed: {}", e);
                self.record_op(
                    "cherry-pick",
                    modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                if let Some(ref mut m) = self.cherry_pick_modal {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
        }

        // Reload display data (new commit should appear in graph).
        self.reload();
    }

    // ── Revert modal (T-CM-034) ─────────────────────────────

    /// Open the revert plan modal for commit `id`.
    pub fn open_revert_modal(&mut self, commit_id: CommitId) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                eprintln!("[kagi] open_revert_modal: no repo_path set");
                return;
            }
        };

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] revert plan: repo open error: {}", e.message());
                return;
            }
        };

        match plan_revert(&repo, &commit_id) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: revert {} blockers={} preview_files={}",
                    commit_id.short(),
                    plan.blockers.len(),
                    plan.preview_files.len()
                );
                self.revert_modal = Some(RevertModal {
                    commit_id,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                eprintln!("[kagi] revert plan: error: {}", e);
            }
        }
    }

    /// Cancel and close the revert modal without making any changes.
    pub fn cancel_revert_modal(&mut self) {
        self.revert_modal = None;
    }

    /// Confirm the revert plan: run preflight, execute, verify, record, reload.
    pub fn confirm_revert(&mut self) {
        let modal = match self.revert_modal.clone() {
            Some(m) => m,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: revert plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "revert",
                    modal.plan.current.clone(),
                    OpOutcome::Refused { blockers: modal.plan.blockers.clone() },
                    rp,
                );
            }
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e.message());
                self.record_op(
                    "revert",
                    modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                if let Some(ref mut m) = self.revert_modal {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
        };

        if let Err(e) = preflight_check(&repo, &modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "revert",
                modal.plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            if let Some(ref mut m) = self.revert_modal {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        match execute_revert(&repo, &modal.commit_id) {
            Ok(new_id) => {
                eprintln!(
                    "[kagi] executed: revert {} -> {}",
                    modal.commit_id.short(),
                    new_id.short()
                );

                let mut repo2 = match git2::Repository::open(&repo_path) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("[kagi] verify: repo open error: {}", e.message());
                        self.reload();
                        return;
                    }
                };
                let after_summary = match kagi::git::snapshot(&mut repo2, 10_000) {
                    Ok(snap) => {
                        if let Head::Attached { target, branch } = &snap.head {
                            if *target == new_id.0 {
                                eprintln!("[kagi] verified: revert HEAD={} on {}", new_id.short(), branch);
                            } else {
                                eprintln!("[kagi] verify: HEAD={} expected {}", &target[..8.min(target.len())], new_id.short());
                            }
                        }
                        StateSummary {
                            head: snap.head.display(),
                            dirty: if snap.status.is_dirty() { "dirty".to_string() } else { "clean".to_string() },
                        }
                    }
                    Err(e) => {
                        eprintln!("[kagi] verify: snapshot error: {}", e);
                        modal.plan.predicted.clone()
                    }
                };

                self.record_op(
                    "revert",
                    modal.plan.current.clone(),
                    OpOutcome::Success { after: after_summary },
                    &repo_path,
                );
            }
            Err(e) => {
                let err_msg = format!("Revert failed: {}", e);
                self.record_op(
                    "revert",
                    modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                if let Some(ref mut m) = self.revert_modal {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
        }

        self.reload();
    }

    // ── Oplog + footer helper (T017) ────────────────────────

    /// Record an operation to the oplog and update the status footer.
    ///
    /// Write failures are non-fatal: they emit a stderr warning only.
    // ── W3-NOTIFY: toast helpers ──────────────────────────────

    /// Queue a snackbar toast (bottom-right). Callable without a Window:
    /// the auto-dismiss ticker is (re)started from `render`.
    pub(crate) fn push_toast(&mut self, kind: ToastKind, message: impl Into<SharedString>) {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        self.toasts.push(Toast {
            id,
            kind,
            message: message.into(),
            born: Instant::now(),
        });
        if self.toasts.len() > TOASTS_MAX {
            self.toasts.remove(0);
        }
    }

    /// Remove a toast by id (× button).
    pub fn dismiss_toast(&mut self, id: u64) {
        self.toasts.retain(|t| t.id != id);
    }

    /// Debounced live re-plan for the open modal(s): waits 250ms of input
    /// silence before doing git work, so typing stays fluid.
    fn schedule_modal_replan(&mut self, cx: &mut Context<Self>) {
        self.modal_replan_gen = self.modal_replan_gen.wrapping_add(1);
        let gen = self.modal_replan_gen;
        cx.spawn(async move |this, acx| {
            gpui::Timer::after(Duration::from_millis(250)).await;
            let _ = this.update(acx, |app, cx| {
                if app.modal_replan_gen == gen {
                    app.run_modal_replans();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Re-plan whichever input-bearing modal is open (used by the debounce
    /// timer and as a freshness guard right before confirm).
    fn run_modal_replans(&mut self) {
        if self.create_branch_modal.is_some() {
            self.replan_create_branch();
        }
        if self.create_worktree_modal.is_some() {
            self.replan_create_worktree();
        }
        if self.stash_push_modal.is_some() {
            self.replan_stash_push();
        }
    }

    /// Lazily create + sync the real text inputs of the create-branch /
    /// create-worktree / stash-push modals (gpui-component `InputState`).
    ///
    /// The old hand-rolled inputs (KeyDown capture + a fake `_` caret) had no
    /// caret, no IME, no click focus and re-planned on every frame
    /// (user-reported). `InputState` needs a `Window`, which open_* callers
    /// (incl. headless) don't all have — so creation happens here, on the
    /// first render after the modal opens, and the modal's plain-`String`
    /// field is kept in sync for the plan/confirm/headless paths.
    fn sync_modal_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // ── Create-branch ───────────────────────────────────
        if let Some(m) = self.create_branch_modal.as_mut() {
            if m.input_state.is_none() {
                let st = cx.new(|cx| InputState::new(window, cx).placeholder("branch-name"));
                st.update(cx, |s, cx| s.focus(window, cx));
                m.input_state = Some(st);
            }
            let v = m
                .input_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            if v != m.input {
                m.input = v;
                m.error = None;
                self.schedule_modal_replan(cx);
            }
        }

        // ── Create-worktree (branch + path fields) ──────────
        // Auto-path: while the user has not touched the path field, it
        // follows the branch name (same behaviour as before).
        let mut set_path: Option<String> = None;
        if let Some(m) = self.create_worktree_modal.as_mut() {
            if m.branch_state.is_none() {
                let st = cx.new(|cx| InputState::new(window, cx).placeholder("branch-name"));
                st.update(cx, |s, cx| s.focus(window, cx));
                m.branch_state = Some(st);
            }
            if m.path_state.is_none() {
                let initial = m.path_input.clone();
                let st = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("worktree path")
                        .default_value(initial)
                });
                m.path_state = Some(st);
            }
            let branch_v = m
                .branch_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            let path_v = m
                .path_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            let mut dirty = false;
            if path_v != m.path_input {
                // Path text differs from what we last wrote → user edit.
                m.path_input = path_v;
                m.path_touched = true;
                dirty = true;
            }
            if branch_v != m.branch_input {
                m.branch_input = branch_v.clone();
                if !m.path_touched {
                    set_path = Some(branch_v);
                }
                dirty = true;
            }
            if dirty {
                m.error = None;
            }
            if dirty && set_path.is_none() {
                self.schedule_modal_replan(cx);
            }
        }
        if let Some(branch) = set_path {
            // Recompute the suggested path outside the &mut borrow.
            let auto = self.default_worktree_path(if branch.is_empty() { "new-branch" } else { &branch });
            if let Some(m) = self.create_worktree_modal.as_mut() {
                m.path_input = auto.clone();
                if let Some(st) = m.path_state.clone() {
                    st.update(cx, |s, cx| s.set_value(auto, window, cx));
                }
            }
            self.schedule_modal_replan(cx);
        }

        // ── Stash push (message) ────────────────────────────
        if let Some(m) = self.stash_push_modal.as_mut() {
            if m.input_state.is_none() {
                let st = cx.new(|cx| InputState::new(window, cx).placeholder("stash message (optional)"));
                st.update(cx, |s, cx| s.focus(window, cx));
                m.input_state = Some(st);
            }
            let v = m
                .input_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            if v != m.input {
                m.input = v;
                m.error = None;
                self.schedule_modal_replan(cx);
            }
        }
    }

    /// Apply a horizontal wheel delta to the graph column scroll offset.
    /// Vertical deltas are ignored (the commit list owns vertical scroll).
    fn scroll_graph_by(&mut self, delta: &gpui::ScrollDelta, cx: &mut Context<Self>) {
        let dx = match delta {
            gpui::ScrollDelta::Pixels(p) => f32::from(p.x),
            gpui::ScrollDelta::Lines(l) => l.x * graph_view::LANE_W,
        };
        if dx.abs() < 0.01 {
            return;
        }
        let lane_count = self.rows.first().map(|r| r.lane_count).unwrap_or(0);
        let max = (lane_count as f32 * graph_view::LANE_W - self.graph_col_w).max(0.0);
        let next = (self.graph_scroll_x - dx).clamp(0.0, max);
        if (next - self.graph_scroll_x).abs() > 0.1 {
            self.graph_scroll_x = next;
            cx.notify();
        }
    }

    /// Spawn the 500ms auto-dismiss ticker if toasts exist and it is not
    /// already running. The task exits as soon as the stack drains.
    fn ensure_toast_ticker(&mut self, cx: &mut Context<Self>) {
        if self.toast_ticker_alive || self.toasts.is_empty() {
            return;
        }
        self.toast_ticker_alive = true;
        cx.spawn(async move |this, acx| {
            loop {
                gpui::Timer::after(Duration::from_millis(500)).await;
                let finished = this.update(acx, |app, cx| {
                    let before = app.toasts.len();
                    app.toasts.retain(|t| !t.expired());
                    if app.toasts.len() != before {
                        cx.notify();
                    }
                    if app.toasts.is_empty() {
                        app.toast_ticker_alive = false;
                        true
                    } else {
                        false
                    }
                });
                match finished {
                    Ok(true) | Err(_) => break,
                    Ok(false) => {}
                }
            }
        })
        .detach();
    }

    /// Render the toast stack as an absolute overlay (bottom-right, above
    /// the status bar). Returns `None` when there is nothing to show.
    fn render_toasts(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.toasts.is_empty() {
            return None;
        }
        let mut stack = div()
            .absolute()
            .bottom(px(34.))
            .left(px(12.))
            .w(px(460.))
            .flex()
            .flex_col()
            .gap_2();

        for toast in &self.toasts {
            let (accent, icon) = match toast.kind {
                ToastKind::Info => (theme().color_branch, "\u{27f3}"),    // ⟳
                ToastKind::Success => (theme().color_success, "\u{2713}"), // ✓
                ToastKind::Error => (theme().color_blocker, "\u{2715}"),   // ✕
            };
            let id = toast.id;
            let dismiss = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
                this.dismiss_toast(id);
                cx.notify();
            });
            stack = stack.child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .rounded(px(8.))
                    .bg(rgb(theme().panel))
                    .border_1()
                    .border_color(rgb(accent))
                    .text_base()
                    .text_color(rgb(theme().text_main))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(rgb(accent))
                            .child(SharedString::from(icon)),
                    )
                    .child(div().flex_1().overflow_hidden().child(toast.message.clone()))
                    .child(
                        div()
                            .id(("toast-dismiss", id))
                            .flex_shrink_0()
                            .px_1()
                            .text_color(rgb(theme().text_muted))
                            .hover(|s| s.text_color(rgb(theme().text_main)))
                            .on_click(dismiss)
                            .child(SharedString::from("\u{00d7}")),
                    ),
            );
        }
        Some(stack.into_any())
    }

    fn record_op(
        &mut self,
        op: &str,
        before: StateSummary,
        outcome: OpOutcome,
        repo_path: &std::path::Path,
    ) {
        // Build the footer message before moving `outcome`.
        let (footer_msg, footer_ok) = match &outcome {
            OpOutcome::Success { after } => {
                (
                    SharedString::from(format!(
                        "{}: {} → {}",
                        op,
                        before.head,
                        after.head
                    )),
                    true,
                )
            }
            OpOutcome::Failed { error } => {
                (SharedString::from(format!("{}: failed — {}", op, error)), false)
            }
            OpOutcome::Refused { blockers } => (
                SharedString::from(format!(
                    "{}: refused ({} blocker{})",
                    op,
                    blockers.len(),
                    if blockers.len() == 1 { "" } else { "s" }
                )),
                false,
            ),
        };

        // W3-NOTIFY: snackbar mirror of the footer message — every plan-pipeline
        // outcome (Success / Failed / Refused) becomes a toast.
        let toast_kind = if matches!(outcome, OpOutcome::Success { .. }) {
            ToastKind::Success
        } else {
            ToastKind::Error
        };
        self.push_toast(toast_kind, footer_msg.clone());

        // T-BP-004: auto-open bottom panel on Failed.
        let is_failed = matches!(outcome, OpOutcome::Failed { .. });

        let repo_str = repo_path.display().to_string();
        let entry = OpLogEntry::new(op, &repo_str, before, outcome);

        if let Err(e) = append_oplog(&entry) {
            eprintln!("[kagi] oplog: write failed (non-fatal): {}", e);
        }

        // T-BP-004: push to in-memory ring-buffer (newest at front).
        self.op_entries.push_front(entry);
        if self.op_entries.len() > OP_ENTRIES_MAX {
            self.op_entries.pop_back();
        }
        // Reset expanded state when new entries arrive.
        self.oplog_expanded = None;

        // T-BP-004: auto-open panel on failure.
        if is_failed {
            self.bottom_panel_open = true;
            self.bottom_tab = BottomTab::OperationLog;
            eprintln!("[kagi] bottom-panel: open (Failed auto-open)");
        }

        if footer_ok {
            eprintln!("[kagi] footer: {}", footer_msg);
            self.status_footer = FooterStatus::Success(footer_msg);
        } else {
            eprintln!("[kagi] footer: {}", footer_msg);
            self.status_footer = FooterStatus::Failed(footer_msg);
        }
    }

    // ── T-BP-007: Terminal session ────────────────────────────

    /// Ensure the terminal session is initialised and the shell is running.
    ///
    /// * Creates a `KagiTerminalSession` on first call if `repo_path` is set.
    /// * Delegates to `terminal::ensure_terminal` which handles PTY startup,
    ///   focus, and failure recording.
    /// * On startup failure, calls `record_op` with `op="terminal-start"` and
    ///   `OpOutcome::Failed`.
    pub fn ensure_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                eprintln!("[kagi] terminal: no repo_path — cannot start terminal");
                return;
            }
        };

        // W4-TABS: sessions are keyed by repo path so each tab keeps its PTY.
        // Initialise the session container for this repo if we haven't yet.
        self.terminal_sessions
            .entry(repo_path.clone())
            .or_insert_with(|| terminal::KagiTerminalSession::new(repo_path.clone()));

        // Mutably borrow the session; split the borrow so record_op can run after.
        let session = self
            .terminal_sessions
            .get_mut(&repo_path)
            .expect("just inserted above");

        let mut failure_msg: Option<String> = None;
        terminal::ensure_terminal(session, window, cx, |msg| {
            failure_msg = Some(msg);
        });

        if let Some(err) = failure_msg {
            use kagi::git::oplog::OpOutcome;
            use kagi::git::ops::StateSummary;
            self.record_op(
                "terminal-start",
                StateSummary {
                    head: "n/a".to_string(),
                    dirty: "n/a".to_string(),
                },
                OpOutcome::Failed { error: err },
                &repo_path,
            );
        }
    }

    /// Confirm the plan: run preflight, execute checkout, then reload.
    ///
    /// On preflight or execute failure the modal remains open and shows the
    /// error text + recovery guidance.  The app never crashes.
    pub fn confirm_checkout(&mut self) {
        let modal = match self.plan_modal.clone() {
            Some(m) => m,
            None => return,
        };
        // Defence in depth: the UI never renders the confirm button when
        // blockers exist, but refuse here too so no code path can execute a
        // blocked plan.
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "checkout",
                    modal.plan.current.clone(),
                    OpOutcome::Refused { blockers: modal.plan.blockers.clone() },
                    rp,
                );
            }
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let op_name = match &modal.target {
            CheckoutPlanTarget::Branch(_) => "checkout",
            CheckoutPlanTarget::Commit(_) => "checkout-commit",
        };

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e.message());
                self.record_op(
                    op_name,
                    modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                self.plan_modal = Some(CheckoutPlanModal {
                    target: modal.target.clone(),
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
                return;
            }
        };

        // Preflight check.
        if let Err(e) = preflight_check(&repo, &modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                op_name,
                modal.plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            self.plan_modal = Some(CheckoutPlanModal {
                target: modal.target.clone(),
                plan: modal.plan.clone(),
                error: Some(SharedString::from(err_msg)),
            });
            return;
        }

        // Execute checkout (safe mode only).
        let execute_result = match &modal.target {
            CheckoutPlanTarget::Branch(branch) => execute_checkout(&repo, branch),
            CheckoutPlanTarget::Commit(commit_id) => execute_checkout_commit(&repo, commit_id),
        };
        if let Err(e) = execute_result {
            let err_msg = format!("Checkout failed: {}", e);
            self.record_op(
                op_name,
                modal.plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            self.plan_modal = Some(CheckoutPlanModal {
                target: modal.target.clone(),
                plan: modal.plan.clone(),
                error: Some(SharedString::from(err_msg)),
            });
            return;
        }

        match &modal.target {
            CheckoutPlanTarget::Branch(branch) => eprintln!("[kagi] executed: checkout {}", branch),
            CheckoutPlanTarget::Commit(commit_id) => {
                eprintln!("[kagi] executed: checkout-commit {}", commit_id.short())
            }
        }

        // Verify: re-snapshot and confirm HEAD.
        let mut repo2 = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] verify: repo open error: {}", e.message());
                self.reload();
                return;
            }
        };
        let after_summary = match kagi::git::snapshot(&mut repo2, 10_000) {
            Ok(snap) => {
                match (&modal.target, &snap.head) {
                    (
                        CheckoutPlanTarget::Branch(branch),
                        Head::Attached { branch: actual_branch, .. },
                    ) if actual_branch == branch => {
                        eprintln!("[kagi] verified: HEAD={}", actual_branch);
                    }
                    (
                        CheckoutPlanTarget::Commit(commit_id),
                        Head::Detached { target },
                    ) if target == &commit_id.0 => {
                        eprintln!("[kagi] verified: detached HEAD={}", commit_id.short());
                    }
                    other => {
                        eprintln!("[kagi] verify: unexpected HEAD state after checkout: {:?}", other);
                    }
                }
                StateSummary {
                    head: snap.head.display(),
                    dirty: if snap.status.is_dirty() { "dirty".to_string() } else { "clean".to_string() },
                }
            }
            Err(e) => {
                eprintln!("[kagi] verify: snapshot error: {}", e);
                modal.plan.predicted.clone()
            }
        };

        // Record success to oplog + update footer.
        self.record_op(
            op_name,
            modal.plan.current.clone(),
            OpOutcome::Success { after: after_summary },
            &repo_path,
        );

        // Reload display data.
        self.reload();
    }

    // ── T-HT-003: Pull ────────────────────────────────────────

    /// Build a pull plan and open the confirmation modal.
    pub fn open_pull_modal(&mut self) {
        // W3-NOTIFY: refuse while a background op runs.
        if self.busy_op.is_some() {
            self.status_footer =
                FooterStatus::Idle(SharedString::from("別の操作が実行中です"));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("pull: repo open error: {}", e.message())));
                return;
            }
        };
        match plan_pull(&repo) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: pull blockers={} warnings={}",
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.pull_modal = Some(PullPlanModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("pull plan error: {}", e)));
            }
        }
    }

    /// Close the pull modal without executing.
    pub fn cancel_pull_modal(&mut self) {
        self.pull_modal = None;
    }

    /// Confirm the pull plan synchronously: preflight, fetch via CLI, then
    /// FF / in-memory merge (see `execute_pull`).  Used by the headless
    /// KAGI_PULL path (no event loop). The UI button uses `start_pull`,
    /// which runs the same blocking core on a background thread (W3-NOTIFY).
    pub fn confirm_pull(&mut self) {
        let modal = match self.pull_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // Defence in depth: refuse blocked plans even if a code path slips through.
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: pull plan has blockers, not executing");
            self.record_op(
                "pull",
                modal.plan.current.clone(),
                OpOutcome::Refused { blockers: modal.plan.blockers.clone() },
                &repo_path,
            );
            return;
        }

        match pull_blocking(&repo_path, &modal.plan) {
            Ok((summary, after_summary)) => {
                self.pull_modal = None;
                self.record_op(
                    "pull",
                    modal.plan.current.clone(),
                    OpOutcome::Success { after: after_summary },
                    &repo_path,
                );
                self.status_footer =
                    FooterStatus::Success(SharedString::from(format!("pull: {}", summary)));
                self.reload();
            }
            Err(err_msg) => {
                self.record_op(
                    "pull",
                    modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                self.pull_modal = Some(PullPlanModal {
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
            }
        }
    }

    /// W3-NOTIFY: UI-path pull — runs `pull_blocking` on a background thread
    /// so the window stays responsive, with start/finish toasts.
    pub fn start_pull(&mut self, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(
                "別の操作が実行中です",
            ));
            return;
        }
        let modal = match self.pull_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: pull plan has blockers, not executing");
            self.record_op(
                "pull",
                modal.plan.current.clone(),
                OpOutcome::Refused { blockers: modal.plan.blockers.clone() },
                &repo_path,
            );
            self.pull_modal = None;
            cx.notify();
            return;
        }

        self.busy_op = Some("pull");
        self.pull_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from("pull 実行中…"));
        self.push_toast(ToastKind::Info, "pull: 開始しました");
        eprintln!("[kagi] async: pull started");

        let plan = modal.plan.clone();
        let bg_path = repo_path.clone();
        let task = cx.background_spawn(async move { pull_blocking(&bg_path, &plan) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.finish_pull(result, modal, repo_path);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Apply the result of a background pull on the main thread.
    fn finish_pull(
        &mut self,
        result: Result<(String, StateSummary), String>,
        modal: PullPlanModal,
        repo_path: PathBuf,
    ) {
        self.busy_op = None;
        match result {
            Ok((summary, after_summary)) => {
                eprintln!("[kagi] async: pull finished — {}", summary);
                self.record_op(
                    "pull",
                    modal.plan.current.clone(),
                    OpOutcome::Success { after: after_summary },
                    &repo_path,
                );
                self.status_footer =
                    FooterStatus::Success(SharedString::from(format!("pull: {}", summary)));
                self.reload();
            }
            Err(err_msg) => {
                eprintln!("[kagi] async: pull failed — {}", err_msg);
                self.record_op(
                    "pull",
                    modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg },
                    &repo_path,
                );
            }
        }
    }

    // ── T-HT-004: Push ────────────────────────────────────────

    /// Build a push plan and open the confirmation modal.
    pub fn open_push_modal(&mut self) {
        // W3-NOTIFY: refuse while a background op runs.
        if self.busy_op.is_some() {
            self.status_footer =
                FooterStatus::Idle(SharedString::from("別の操作が実行中です"));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("push: repo open error: {}", e.message())));
                return;
            }
        };
        match plan_push(&repo) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: push blockers={} warnings={} preview_commits={}",
                    plan.blockers.len(),
                    plan.warnings.len(),
                    plan.preview_commits.len(),
                );
                self.push_modal = Some(PushPlanModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("push plan error: {}", e)));
            }
        }
    }

    /// Close the push modal without executing.
    pub fn cancel_push_modal(&mut self) {
        self.push_modal = None;
    }

    /// Confirm the push plan synchronously: preflight, execute push via CLI.
    /// Used by the headless KAGI_PUSH path. The UI button uses `start_push`
    /// (background thread + toasts, W3-NOTIFY).
    pub fn confirm_push(&mut self) {
        let modal = match self.push_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // Defence in depth: refuse blocked plans even if a code path slips through.
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: push plan has blockers, not executing");
            self.record_op(
                "push",
                modal.plan.current.clone(),
                OpOutcome::Refused { blockers: modal.plan.blockers.clone() },
                &repo_path,
            );
            return;
        }

        match push_blocking(&repo_path, &modal.plan) {
            Ok((summary, after_summary)) => {
                self.push_modal = None;
                self.record_op(
                    "push",
                    modal.plan.current.clone(),
                    OpOutcome::Success { after: after_summary },
                    &repo_path,
                );
                self.status_footer =
                    FooterStatus::Success(SharedString::from(format!("push: {}", summary)));
                self.reload();
            }
            Err(err_msg) => {
                self.record_op(
                    "push",
                    modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                self.push_modal = Some(PushPlanModal {
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
            }
        }
    }

    /// W3-NOTIFY: UI-path push — background thread + start/finish toasts.
    pub fn start_push(&mut self, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(
                "別の操作が実行中です",
            ));
            return;
        }
        let modal = match self.push_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: push plan has blockers, not executing");
            self.record_op(
                "push",
                modal.plan.current.clone(),
                OpOutcome::Refused { blockers: modal.plan.blockers.clone() },
                &repo_path,
            );
            self.push_modal = None;
            cx.notify();
            return;
        }

        self.busy_op = Some("push");
        self.push_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from("push 実行中…"));
        self.push_toast(ToastKind::Info, "push: 開始しました");
        eprintln!("[kagi] async: push started");

        let plan = modal.plan.clone();
        let bg_path = repo_path.clone();
        let task = cx.background_spawn(async move { push_blocking(&bg_path, &plan) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.finish_push(result, modal, repo_path);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Apply the result of a background push on the main thread.
    fn finish_push(
        &mut self,
        result: Result<(String, StateSummary), String>,
        modal: PushPlanModal,
        repo_path: PathBuf,
    ) {
        self.busy_op = None;
        match result {
            Ok((summary, after_summary)) => {
                eprintln!("[kagi] async: push finished — {}", summary);
                self.record_op(
                    "push",
                    modal.plan.current.clone(),
                    OpOutcome::Success { after: after_summary },
                    &repo_path,
                );
                self.status_footer =
                    FooterStatus::Success(SharedString::from(format!("push: {}", summary)));
                self.reload();
            }
            Err(err_msg) => {
                eprintln!("[kagi] async: push failed — {}", err_msg);
                self.record_op(
                    "push",
                    modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg },
                    &repo_path,
                );
            }
        }
    }

    // ── T-HT-009: Undo Commit / T-HT-007: Stash Pop ──────────

    /// Build an undo-commit plan and open the confirmation modal.
    pub fn open_undo_modal(&mut self) {
        let repo_path = match self.repo_path.clone() { Some(p) => p, None => return };
        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!("undo: repo open error: {}", e.message())));
                return;
            }
        };
        match plan_undo_commit(&repo) {
            Ok(plan) => {
                eprintln!("[kagi] plan: undo blockers={} warnings={}", plan.blockers.len(), plan.warnings.len());
                self.undo_modal = Some(UndoPlanModal { plan: std::sync::Arc::new(plan), error: None });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!("undo plan error: {}", e)));
            }
        }
    }

    pub fn cancel_undo_modal(&mut self) { self.undo_modal = None; }

    /// Confirm undo: preflight → execute (ref-only) → oplog → reload.
    pub fn confirm_undo(&mut self) {
        let modal = match self.undo_modal.clone() { Some(m) => m, None => return };
        let repo_path = match self.repo_path.clone() { Some(p) => p, None => return };
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: undo plan has blockers, not executing");
            self.record_op("undo-commit", modal.plan.current.clone(),
                OpOutcome::Refused { blockers: modal.plan.blockers.clone() }, &repo_path);
            return;
        }
        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e.message());
                self.record_op("undo-commit", modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() }, &repo_path);
                self.undo_modal = Some(UndoPlanModal { plan: modal.plan.clone(), error: Some(SharedString::from(err_msg)) });
                return;
            }
        };
        if let Err(e) = preflight_check(&repo, &modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op("undo-commit", modal.plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() }, &repo_path);
            self.undo_modal = Some(UndoPlanModal { plan: modal.plan.clone(), error: Some(SharedString::from(err_msg)) });
            return;
        }
        match execute_undo_commit(&repo) {
            Ok(outcome) => {
                eprintln!("[kagi] executed: undo {} -> now at {}", outcome.undone.short(), outcome.now_at.short());
                self.undo_modal = None;
                let after = StateSummary {
                    head: format!("branch @ {}", outcome.now_at.short()),
                    dirty: "changes staged".to_string(),
                };
                self.record_op("undo-commit", modal.plan.current.clone(),
                    OpOutcome::Success { after }, &repo_path);
                self.status_footer = FooterStatus::Success(SharedString::from(
                    format!("undo: {} (restore: git reset --soft {})", outcome.undone.short(), outcome.undone.short())));
                self.reload();
            }
            Err(e) => {
                let err_msg = format!("Undo failed: {}", e);
                self.record_op("undo-commit", modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() }, &repo_path);
                self.undo_modal = Some(UndoPlanModal { plan: modal.plan.clone(), error: Some(SharedString::from(err_msg)) });
            }
        }
    }

    /// Build a stash-pop plan and open the confirmation modal.
    pub fn open_pop_modal(&mut self, index: usize) {
        let repo_path = match self.repo_path.clone() { Some(p) => p, None => return };
        let mut repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!("pop: repo open error: {}", e.message())));
                return;
            }
        };
        match plan_stash_pop(&mut repo, index) {
            Ok(plan) => {
                eprintln!("[kagi] plan: stash-pop index={} blockers={} warnings={}", index, plan.blockers.len(), plan.warnings.len());
                self.pop_modal = Some(PopPlanModal { plan: std::sync::Arc::new(plan), error: None, stash_index: index });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!("pop plan error: {}", e)));
            }
        }
    }

    pub fn cancel_pop_modal(&mut self) { self.pop_modal = None; }

    /// Confirm stash pop: preflight → apply-then-drop → oplog → reload.
    pub fn confirm_pop(&mut self) {
        let modal = match self.pop_modal.clone() { Some(m) => m, None => return };
        let repo_path = match self.repo_path.clone() { Some(p) => p, None => return };
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: pop plan has blockers, not executing");
            self.record_op("stash-pop", modal.plan.current.clone(),
                OpOutcome::Refused { blockers: modal.plan.blockers.clone() }, &repo_path);
            return;
        }
        let mut repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e.message());
                self.record_op("stash-pop", modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() }, &repo_path);
                self.pop_modal = Some(PopPlanModal { plan: modal.plan.clone(), error: Some(SharedString::from(err_msg)), stash_index: modal.stash_index });
                return;
            }
        };
        if let Err(e) = preflight_check(&repo, &modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op("stash-pop", modal.plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() }, &repo_path);
            self.pop_modal = Some(PopPlanModal { plan: modal.plan.clone(), error: Some(SharedString::from(err_msg)), stash_index: modal.stash_index });
            return;
        }
        match execute_stash_pop(&mut repo, modal.stash_index) {
            Ok(()) => {
                eprintln!("[kagi] executed: stash-pop index={}", modal.stash_index);
                self.pop_modal = None;
                let after = StateSummary { head: modal.plan.current.head.clone(), dirty: "changes restored (stash removed)".to_string() };
                self.record_op("stash-pop", modal.plan.current.clone(),
                    OpOutcome::Success { after }, &repo_path);
                self.status_footer = FooterStatus::Success(SharedString::from("stash pop: applied and dropped"));
                self.reload();
            }
            Err(e) => {
                let err_msg = format!("Pop failed: {}", e);
                self.record_op("stash-pop", modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() }, &repo_path);
                self.pop_modal = Some(PopPlanModal { plan: modal.plan.clone(), error: Some(SharedString::from(err_msg)), stash_index: modal.stash_index });
            }
        }
    }

    // ── W2-DELETE: Delete-branch modal ───────────────────────

    /// Build a delete-branch plan for `branch_name` and open the confirmation modal.
    pub fn open_delete_branch_modal(&mut self, branch_name: impl Into<String>) {
        let branch_name = branch_name.into();
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                eprintln!("[kagi] open_delete_branch_modal: no repo_path set");
                return;
            }
        };
        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    format!("delete-branch: repo open error: {}", e.message()),
                ));
                return;
            }
        };
        match plan_delete_branch(&repo, &branch_name) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: delete-branch {} blockers={}",
                    branch_name,
                    plan.blockers.len()
                );
                self.delete_branch_modal = Some(DeleteBranchModal {
                    branch_name,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(
                    format!("delete-branch plan error: {}", e),
                ));
            }
        }
    }

    pub fn cancel_delete_branch_modal(&mut self) {
        self.delete_branch_modal = None;
    }

    /// Confirm delete-branch: preflight → execute → oplog → reload.
    pub fn confirm_delete_branch(&mut self) {
        let modal = match self.delete_branch_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        if !modal.plan.blockers.is_empty() {
            eprintln!(
                "[kagi] refused: delete-branch plan has {} blocker(s), not executing",
                modal.plan.blockers.len()
            );
            self.record_op(
                "delete-branch",
                modal.plan.current.clone(),
                kagi::git::oplog::OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            return;
        }

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e.message());
                self.record_op(
                    "delete-branch",
                    modal.plan.current.clone(),
                    kagi::git::oplog::OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                self.delete_branch_modal = Some(DeleteBranchModal {
                    branch_name: modal.branch_name.clone(),
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
                return;
            }
        };

        if let Err(e) = kagi::git::ops::preflight_check(&repo, &modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "delete-branch",
                modal.plan.current.clone(),
                kagi::git::oplog::OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            self.delete_branch_modal = Some(DeleteBranchModal {
                branch_name: modal.branch_name.clone(),
                plan: modal.plan.clone(),
                error: Some(SharedString::from(err_msg)),
            });
            return;
        }

        match execute_delete_branch(&repo, &modal.plan, &modal.branch_name) {
            Ok(()) => {
                eprintln!("[kagi] executed: delete-branch {}", modal.branch_name);
                self.delete_branch_modal = None;
                let after = kagi::git::ops::StateSummary {
                    head: modal.plan.current.head.clone(),
                    dirty: format!("branch '{}' deleted", modal.branch_name),
                };
                self.record_op(
                    "delete-branch",
                    modal.plan.current.clone(),
                    kagi::git::oplog::OpOutcome::Success { after },
                    &repo_path,
                );
                self.status_footer = FooterStatus::Success(SharedString::from(format!(
                    "delete-branch: '{}' deleted (restore: {})",
                    modal.branch_name,
                    modal.plan.recovery.lines().nth(1).unwrap_or("git branch …")
                )));
                self.reload();
            }
            Err(e) => {
                let err_msg = format!("Delete failed: {}", e);
                self.record_op(
                    "delete-branch",
                    modal.plan.current.clone(),
                    kagi::git::oplog::OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                self.delete_branch_modal = Some(DeleteBranchModal {
                    branch_name: modal.branch_name.clone(),
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
            }
        }
    }

    // ── T025: Commit Panel ────────────────────────────────────

    /// Open the commit panel (triggered by clicking the WIP row).
    ///
    /// Loads the current staging status from the repository.
    /// Clears any existing commit selection so the two views are exclusive.
    pub fn open_commit_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // T026: lazy-create the InputState (requires &mut Window) on first open.
        if self.commit_input.is_none() {
            let input_entity = cx.new(|cx| InputState::new(window, cx).placeholder("Commit message"));
            self.commit_input = Some(input_entity);
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                eprintln!("[kagi] open_commit_panel: no repo_path set");
                return;
            }
        };
        let mut panel = CommitPanelState::from_repo(&repo_path);
        // Preserve tree_view toggle if we're reopening an existing panel.
        if let Some(ref existing) = self.commit_panel {
            panel.tree_view = existing.tree_view;
        }
        self.commit_panel = Some(panel);
        self.commit_panel_open = true;
        self.selected = None;
        self.main_diff = None;

        // T026: focus the InputState after opening the panel.
        if let Some(ref input_entity) = self.commit_input {
            input_entity.update(cx, |state, cx| {
                state.focus(window, cx);
            });
        }

        // Log for headless verification.
        if let Some(ref p) = self.commit_panel {
            eprintln!(
                "[kagi] commit-panel: unstaged={} staged={}",
                p.unstaged.len(),
                p.staged.len()
            );
        }
    }

    /// Stage a single file in the commit panel.
    ///
    /// Calls `stage_file` from T024 and then refreshes the staging status.
    /// Stage every non-conflicted unstaged file (T-UI-002: Stage all).
    pub fn do_stage_all(&mut self) {
        let repo_path = match self.repo_path.clone() { Some(p) => p, None => return };
        let paths: Vec<std::path::PathBuf> = match self.commit_panel.as_ref() {
            Some(p) => p.unstaged.iter()
                .filter(|f| !p.is_conflicted(&f.path))
                .map(|f| f.path.clone())
                .collect(),
            None => return,
        };
        if paths.is_empty() { return; }
        let repo = match git2::Repository::open(&repo_path) { Ok(r) => r, Err(_) => return };
        match kagi::git::stage_files(&repo, &paths) {
            Ok(n) => {
                eprintln!("[kagi] staged-all: {} file(s)", n);
                if let Some(panel) = self.commit_panel.as_mut() {
                    panel.reload_status(&repo_path);
                }
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!("stage all failed: {}", e)));
            }
        }
    }

    /// Unstage every staged file (T-UI-002: Unstage all).
    pub fn do_unstage_all(&mut self) {
        let repo_path = match self.repo_path.clone() { Some(p) => p, None => return };
        let paths: Vec<std::path::PathBuf> = match self.commit_panel.as_ref() {
            Some(p) => p.staged.iter().map(|f| f.path.clone()).collect(),
            None => return,
        };
        if paths.is_empty() { return; }
        let repo = match git2::Repository::open(&repo_path) { Ok(r) => r, Err(_) => return };
        match kagi::git::unstage_files(&repo, &paths) {
            Ok(n) => {
                eprintln!("[kagi] unstaged-all: {} file(s)", n);
                if let Some(panel) = self.commit_panel.as_mut() {
                    panel.reload_status(&repo_path);
                }
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!("unstage all failed: {}", e)));
            }
        }
    }

    pub fn do_stage_file(&mut self, index: usize) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let path = match self.commit_panel.as_ref().and_then(|p| p.unstaged.get(index)) {
            Some(f) => f.path.clone(),
            None => return,
        };
        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] stage_file: repo open error: {}", e.message());
                return;
            }
        };
        if let Err(e) = stage_file(&repo, &path) {
            eprintln!("[kagi] stage_file error: {}", e);
        } else {
            eprintln!("[kagi] staged: {}", path.display());
        }
        if let Some(ref mut panel) = self.commit_panel {
            panel.reload_status(&repo_path);
            eprintln!(
                "[kagi] commit-panel: unstaged={} staged={}",
                panel.unstaged.len(),
                panel.staged.len()
            );
        }
    }

    /// Unstage a single file in the commit panel.
    ///
    /// Calls `unstage_file` from T024 and then refreshes the staging status.
    pub fn do_unstage_file(&mut self, index: usize) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let path = match self.commit_panel.as_ref().and_then(|p| p.staged.get(index)) {
            Some(f) => f.path.clone(),
            None => return,
        };
        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] unstage_file: repo open error: {}", e.message());
                return;
            }
        };
        if let Err(e) = unstage_file(&repo, &path) {
            eprintln!("[kagi] unstage_file error: {}", e);
        } else {
            eprintln!("[kagi] unstaged: {}", path.display());
        }
        if let Some(ref mut panel) = self.commit_panel {
            panel.reload_status(&repo_path);
            eprintln!(
                "[kagi] commit-panel: unstaged={} staged={}",
                panel.unstaged.len(),
                panel.staged.len()
            );
        }
    }

    /// T-UI-003: Select a file in the commit panel and open it in the main diff pane.
    pub fn select_commit_panel_file(&mut self, file_ref: CommitPanelFileRef) {
        self.open_main_diff_wip(file_ref);
    }


    /// Handle a key-down event for the commit message input.
    ///
    /// Uses the T014 simple pattern: printable chars appended, backspace removes last.
    #[allow(dead_code)]
    pub fn handle_commit_msg_key(&mut self, event: &KeyDownEvent) {
        let panel = match self.commit_panel.as_mut() {
            Some(p) => p,
            None => return,
        };
        let key = &event.keystroke.key;
        let modifiers = &event.keystroke.modifiers;

        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }

        if key == "backspace" {
            panel.commit_msg.pop();
        } else if key == "space" {
            panel.commit_msg.push(' ');
        } else if key.len() == 1 {
            let ch = key.chars().next().unwrap();
            if !ch.is_control() {
                panel.commit_msg.push(ch);
            }
        }
    }

    /// Open the commit plan modal for the current staged files and message.
    ///
    /// Uses `plan_commit` from T024.
    /// T026: reads message from InputState if available, else falls back to commit_panel.commit_msg
    /// (used by the headless KAGI_COMMIT_MSG path).
    pub fn open_commit_plan_modal(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // T026: prefer InputState value (UI path); fall back to commit_msg (headless path).
        let msg: String = if let Some(ref input_entity) = self.commit_input {
            input_entity.read(cx).value().to_string()
        } else {
            match self.commit_panel.as_ref() {
                Some(p) => p.commit_msg.clone(),
                None => return,
            }
        };
        if msg.trim().is_empty() {
            return;
        }
        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] plan_commit: repo open error: {}", e.message());
                return;
            }
        };
        match plan_commit(&repo, &msg) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: commit blockers={} warnings={}",
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                if let Some(ref mut panel) = self.commit_panel {
                    panel.plan_modal = Some(CommitPlanModal {
                        plan: std::sync::Arc::new(plan),
                        error: None,
                    });
                }
            }
            Err(e) => {
                eprintln!("[kagi] plan_commit error: {}", e);
            }
        }
    }

    /// Cancel the commit plan modal.
    pub fn cancel_commit_plan_modal(&mut self) {
        if let Some(ref mut panel) = self.commit_panel {
            panel.plan_modal = None;
        }
    }

    /// Confirm the commit plan: run execute_commit then reload.
    ///
    /// On failure the modal remains open with the error text.
    /// T026: cx is needed to read the InputState value.
    pub fn confirm_commit(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // T026: read message from InputState if available, else from commit_panel.commit_msg.
        let commit_message: String = if let Some(ref input_entity) = self.commit_input {
            input_entity.read(cx).value().to_string()
        } else {
            self.commit_panel.as_ref().map(|p| p.commit_msg.clone()).unwrap_or_default()
        };
        let (msg, plan) = match self.commit_panel.as_ref().and_then(|p| p.plan_modal.as_ref()) {
            Some(modal) => (
                commit_message,
                modal.plan.clone(),
            ),
            None => return,
        };

        // Defence: refuse if blockers exist.
        if !plan.blockers.is_empty() {
            eprintln!("[kagi] refused: commit plan has blockers");
            return;
        }

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e.message());
                if let Some(ref mut panel) = self.commit_panel {
                    if let Some(ref mut modal) = panel.plan_modal {
                        modal.error = Some(SharedString::from(err_msg.clone()));
                    }
                }
                return;
            }
        };

        match execute_commit(&repo, &msg) {
            Ok(new_id) => {
                eprintln!("[kagi] executed: commit {}", new_id.short());

                // Verify: re-snapshot, check HEAD is the new commit.
                let mut repo2 = match git2::Repository::open(&repo_path) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("[kagi] verify: repo open error: {}", e.message());
                        self.record_op(
                            "commit",
                            plan.current.clone(),
                            OpOutcome::Success { after: plan.predicted.clone() },
                            &repo_path,
                        );
                        self.reload();
                        return;
                    }
                };
                let after_summary = match kagi::git::snapshot(&mut repo2, 10_000) {
                    Ok(snap) => {
                        if let Head::Attached { target, branch } = &snap.head {
                            if *target == new_id.0 {
                                eprintln!("[kagi] verified: commit HEAD={} on {}", new_id.short(), branch);
                            } else {
                                eprintln!("[kagi] verify: HEAD mismatch after commit");
                            }
                        }
                        // Unstaged should still be there.
                        let is_dirty = snap.status.is_dirty();
                        eprintln!("[kagi] verified: working tree {} after commit",
                            if is_dirty { "dirty (unstaged remain)" } else { "clean" });
                        StateSummary {
                            head: snap.head.display(),
                            dirty: if is_dirty { "dirty".to_string() } else { "clean".to_string() },
                        }
                    }
                    Err(e) => {
                        eprintln!("[kagi] verify: snapshot error: {}", e);
                        plan.predicted.clone()
                    }
                };

                self.record_op("commit", plan.current.clone(),
                    OpOutcome::Success { after: after_summary }, &repo_path);
                self.reload();
            }
            Err(e) => {
                let err_msg = format!("Commit failed: {}", e);
                eprintln!("[kagi] {}", err_msg);
                if let Some(ref mut panel) = self.commit_panel {
                    if let Some(ref mut modal) = panel.plan_modal {
                        modal.error = Some(SharedString::from(err_msg));
                    }
                }
            }
        }
    }

    /// Select the commit at `index` (or deselect if already selected).
    /// Emits a `[kagi] selected:` log for automated verification.
    /// On first selection of a row, fetches changed files on-demand and caches
    /// the result; subsequent selections of the same row reuse the cache.
    /// Clears any open diff view when the selection changes.
    /// Also closes the commit panel since commit selection and commit panel are exclusive.
    pub fn select(&mut self, index: usize) {
        // Close commit panel when selecting a normal commit row.
        self.commit_panel_open = false;

        // Toggle: clicking the same row again deselects it.
        if self.selected == Some(index) {
            self.selected = None;
            self.main_diff = None;
            self.compare_view = None;
            return;
        }
        self.selected = Some(index);
        // Clear any open main diff when the commit selection changes.
        self.main_diff = None;
        self.compare_view = None;

        if let Some(detail) = self.details.get(index) {
            let parent_count = detail.parent_ids.len();
            eprintln!(
                "[kagi] selected: {} parents={}",
                detail.full_sha.as_ref().get(..8).unwrap_or(&detail.full_sha),
                parent_count,
            );
        }

        // Fetch changed files on-demand (only once per row).
        if !self.diff_cache.contains_key(&index) {
            let files_opt = self.fetch_changed_files(index);
            let n = files_opt.as_ref().map(|v| v.len()).unwrap_or(0);
            eprintln!("[kagi] changed files: {}", n);
            self.diff_cache.insert(index, files_opt);
        } else {
            // Already cached — just emit the log.
            let n = self
                .diff_cache
                .get(&index)
                .and_then(|v| v.as_ref())
                .map(|v| v.len())
                .unwrap_or(0);
            eprintln!("[kagi] changed files: {}", n);
        }

        // T018: emit tree structure log when KAGI_SELECT_FIRST=1
        if std::env::var("KAGI_SELECT_FIRST").as_deref() == Ok("1") {
            const MAX_FILES: usize = 100;
            if let Some(Some(files)) = self.diff_cache.get(&index) {
                let truncated: Vec<_> = files.iter().take(MAX_FILES).cloned().collect();
                let rows = file_tree::build_file_tree(&truncated);
                for row in &rows {
                    match row {
                        file_tree::TreeRow::Dir { depth, name } => {
                            eprintln!("[kagi] tree: {}DIR  {}", "  ".repeat(*depth), name);
                        }
                        file_tree::TreeRow::File { depth, name, file_index, .. } => {
                            eprintln!("[kagi] tree: {}FILE {} (idx={})", "  ".repeat(*depth), name, file_index);
                        }
                    }
                }
            }
        }
    }

    /// T-UI-003: Open the diff for the file at `file_index` in the currently
    /// selected commit in the full-width main pane.
    ///
    /// Emits the legacy `[kagi] diff:` log (headless compat) plus
    /// `[kagi] main-diff: open <path> rows=N`.
    /// No-op if no commit is selected.
    /// Step the open main diff to the previous/next file (arrow keys).
    /// No-op when no diff is open or already at the list edge.
    pub fn main_diff_step(&mut self, delta: i64) {
        let source = match self.main_diff.as_ref() {
            Some(d) => d.source.clone(),
            None => return,
        };
        match source {
            MainDiffSource::Commit { row_index, file_index } => {
                let len = self
                    .diff_cache
                    .get(&row_index)
                    .and_then(|o| o.as_ref())
                    .map(|v| v.len())
                    .unwrap_or(0);
                if len == 0 {
                    return;
                }
                let next = (file_index as i64 + delta).clamp(0, len as i64 - 1) as usize;
                if next != file_index {
                    self.open_main_diff_commit(next);
                }
            }
            MainDiffSource::Compare { base, target, file_index } => {
                let len = match self.compare_view.as_ref() {
                    Some(view) if view.base == base && view.target == target => view.files.len(),
                    _ => 0,
                };
                if len == 0 {
                    return;
                }
                let next = (file_index as i64 + delta).clamp(0, len as i64 - 1) as usize;
                if next != file_index {
                    self.open_main_diff_compare(next);
                }
            }
            MainDiffSource::Unstaged { path } => {
                let (cur, len) = match self.commit_panel.as_ref() {
                    Some(p) => (
                        p.unstaged.iter().position(|f| f.path == path),
                        p.unstaged.len(),
                    ),
                    None => return,
                };
                let cur = match cur { Some(c) => c, None => return };
                if len == 0 { return; }
                let next = (cur as i64 + delta).clamp(0, len as i64 - 1) as usize;
                if next != cur {
                    self.open_main_diff_wip(commit_panel::CommitPanelFileRef::Unstaged { index: next });
                }
            }
            MainDiffSource::Staged { path } => {
                let (cur, len) = match self.commit_panel.as_ref() {
                    Some(p) => (
                        p.staged.iter().position(|f| f.path == path),
                        p.staged.len(),
                    ),
                    None => return,
                };
                let cur = match cur { Some(c) => c, None => return };
                if len == 0 { return; }
                let next = (cur as i64 + delta).clamp(0, len as i64 - 1) as usize;
                if next != cur {
                    self.open_main_diff_wip(commit_panel::CommitPanelFileRef::Staged { index: next });
                }
            }
        }
    }

    pub fn open_main_diff_inspector_file(&mut self, file_index: usize) {
        if self.compare_view.is_some() {
            self.open_main_diff_compare(file_index);
        } else {
            self.open_main_diff_commit(file_index);
        }
    }

    pub fn open_main_diff_commit(&mut self, file_index: usize) {
        use kagi::git::{CommitId, commit_file_diff};

        let selected = match self.selected {
            Some(s) => s,
            None => return,
        };
        let repo_path = match self.repo_path.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        let detail = match self.details.get(selected) {
            Some(d) => d,
            None => return,
        };
        let files = match self.diff_cache.get(&selected).and_then(|v| v.as_ref()) {
            Some(f) => f,
            None => return,
        };
        let file_status = match files.get(file_index) {
            Some(f) => f,
            None => return,
        };

        let id = CommitId(detail.full_sha.as_ref().to_string());
        let path = file_status.path.clone();

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };

        match commit_file_diff(&repo, &id, &path) {
            Ok(file_diff) => {
                // Count added / removed lines for the log.
                let added: usize = file_diff
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| l.kind == DiffLineKind::Added)
                    .count();
                let removed: usize = file_diff
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| l.kind == DiffLineKind::Removed)
                    .count();
                let hunks = file_diff.hunks.len();

                // Legacy headless compat log (검증スクリプトを壊さない).
                eprintln!(
                    "[kagi] diff: {} hunks={} (+{} -{})",
                    path.display(),
                    hunks,
                    added,
                    removed,
                );

                let fdv = FileDiffView::from_file_diff(&file_diff, file_index);
                let stats = SharedString::from(format!("+{} \u{2212}{}", added, removed));
                let title = fdv.file_name.clone();
                let mut rows = fdv.rows;
                let row_count = rows.len();

                // T-UI-004: apply syntax highlighting once at open time.
                let hl_lang = highlight_diff_rows(&mut rows, &path);
                eprintln!("[kagi] main-diff: open {} rows={} highlight={}", path.display(), row_count, hl_lang);

                self.main_diff = Some(MainDiffView {
                    title,
                    stats,
                    rows,
                    source: MainDiffSource::Commit { row_index: selected, file_index },
                });
            }
            Err(e) => {
                eprintln!("[kagi] diff error: {}", e);
            }
        }
    }

    pub fn open_main_diff_compare(&mut self, file_index: usize) {
        use kagi::git::{compare_commit_to_workdir_file_diff, compare_file_diff};

        let repo_path = match self.repo_path.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        let view = match self.compare_view.as_ref() {
            Some(v) => v.clone(),
            None => return,
        };
        let file_status = match view.files.get(file_index) {
            Some(f) => f,
            None => return,
        };
        let path = file_status.path.clone();

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };

        let file_diff_result = match view.target {
            CompareTarget::Head => {
                let head = match repo.head().ok().and_then(|h| h.target()) {
                    Some(oid) => CommitId(oid.to_string()),
                    None => return,
                };
                compare_file_diff(&repo, &view.base, &head, &path)
            }
            CompareTarget::WorkingTree => compare_commit_to_workdir_file_diff(&repo, &view.base, &path),
        };

        match file_diff_result {
            Ok(file_diff) => {
                let added: usize = file_diff
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| l.kind == DiffLineKind::Added)
                    .count();
                let removed: usize = file_diff
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| l.kind == DiffLineKind::Removed)
                    .count();
                let hunks = file_diff.hunks.len();

                eprintln!(
                    "[kagi] diff: {} hunks={} (+{} -{})",
                    path.display(),
                    hunks,
                    added,
                    removed,
                );

                let fdv = FileDiffView::from_file_diff(&file_diff, file_index);
                let stats = SharedString::from(format!("+{} \u{2212}{}", added, removed));
                let title = fdv.file_name.clone();
                let mut rows = fdv.rows;
                let row_count = rows.len();

                let hl_lang = highlight_diff_rows(&mut rows, &path);
                eprintln!("[kagi] main-diff: open {} rows={} highlight={}", path.display(), row_count, hl_lang);

                self.main_diff = Some(MainDiffView {
                    title,
                    stats,
                    rows,
                    source: MainDiffSource::Compare {
                        base: view.base,
                        target: view.target,
                        file_index,
                    },
                });
            }
            Err(e) => {
                eprintln!("[kagi] compare diff error: {}", e);
            }
        }
    }

    /// T-UI-003: Open the diff for a Commit Panel file in the full-width main pane.
    pub fn open_main_diff_wip(&mut self, file_ref: commit_panel::CommitPanelFileRef) {
        use kagi::git::{unstaged_file_diff, staged_file_diff};

        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let panel = match self.commit_panel.as_ref() {
            Some(p) => p,
            None => return,
        };

        let (is_staged, path) = match &file_ref {
            commit_panel::CommitPanelFileRef::Unstaged { index } => {
                if let Some(f) = panel.unstaged.get(*index) {
                    (false, f.path.clone())
                } else {
                    return;
                }
            }
            commit_panel::CommitPanelFileRef::Staged { index } => {
                if let Some(f) = panel.staged.get(*index) {
                    (true, f.path.clone())
                } else {
                    return;
                }
            }
        };

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };

        let file_diff_result = if is_staged {
            staged_file_diff(&repo, &path)
        } else {
            unstaged_file_diff(&repo, &path)
        };

        match file_diff_result {
            Ok(fd) => {
                let added: usize = fd.hunks.iter().flat_map(|h| h.lines.iter())
                    .filter(|l| l.kind == DiffLineKind::Added).count();
                let removed: usize = fd.hunks.iter().flat_map(|h| h.lines.iter())
                    .filter(|l| l.kind == DiffLineKind::Removed).count();
                eprintln!("[kagi] commit-panel diff: {} (+{} -{})", path.display(), added, removed);

                let fdv = FileDiffView::from_file_diff(&fd, 0);
                let stats = SharedString::from(format!("+{} \u{2212}{}", added, removed));
                let title = fdv.file_name.clone();
                let mut rows = fdv.rows;
                let row_count = rows.len();

                // T-UI-004: apply syntax highlighting once at open time.
                let hl_lang = highlight_diff_rows(&mut rows, &path);
                eprintln!("[kagi] main-diff: open {} rows={} highlight={}", path.display(), row_count, hl_lang);

                let source = if is_staged {
                    MainDiffSource::Staged { path }
                } else {
                    MainDiffSource::Unstaged { path }
                };
                self.main_diff = Some(MainDiffView { title, stats, rows, source });
            }
            Err(e) => {
                eprintln!("[kagi] commit-panel diff error: {}", e);
            }
        }
    }

    /// T-UI-003: Close the main diff view and return to the commit graph.
    /// No-op when main_diff is None.
    pub fn close_main_diff(&mut self) {
        self.main_diff = None;
    }

    /// Fetch changed files for the commit at `index`.  Returns `None` on
    /// failure (so the UI can show "(diff unavailable)").
    fn fetch_changed_files(&self, index: usize) -> Option<Vec<FileStatus>> {
        use kagi::git::{CommitId, commit_changed_files};

        let repo_path = self.repo_path.as_ref()?;
        let detail = self.details.get(index)?;
        let id = CommitId(detail.full_sha.as_ref().to_string());

        let repo = git2::Repository::open(repo_path).ok()?;
        commit_changed_files(&repo, &id).ok()
    }

    pub fn close_compare_view(&mut self) {
        self.compare_view = None;
        self.main_diff = None;
    }

    pub fn show_changed_files_for_commit(&mut self, target: CommitId) {
        let row_index = match self.row_for_commit_id(&target) {
            Some(ix) => ix,
            None => return,
        };
        self.close_compare_view();
        if self.selected != Some(row_index) {
            self.select(row_index);
        } else if !self.diff_cache.contains_key(&row_index) {
            let files_opt = self.fetch_changed_files(row_index);
            let n = files_opt.as_ref().map(|v| v.len()).unwrap_or(0);
            eprintln!("[kagi] changed files: {}", n);
            self.diff_cache.insert(row_index, files_opt);
        }
    }

    pub fn open_compare_with_head(&mut self, target: CommitId) {
        use kagi::git::compare_commits;

        let row_index = match self.row_for_commit_id(&target) {
            Some(ix) => ix,
            None => return,
        };
        if self.selected != Some(row_index) {
            self.select(row_index);
        }

        let repo_path = match self.repo_path.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] compare: repo open error: {}", e.message());
                return;
            }
        };
        let head = match repo.head().ok().and_then(|h| h.target()) {
            Some(oid) => CommitId(oid.to_string()),
            None => {
                eprintln!("[kagi] compare: HEAD unavailable");
                return;
            }
        };

        match compare_commits(&repo, &target, &head) {
            Ok(files) => {
                let title = SharedString::from(format!("{} \u{2194} HEAD", target.short()));
                eprintln!(
                    "[kagi] compare: {} <-> HEAD files={}",
                    target.short(),
                    files.len()
                );
                self.main_diff = None;
                self.compare_view = Some(CompareView {
                    base: target,
                    target: CompareTarget::Head,
                    files,
                    title,
                });
            }
            Err(e) => {
                eprintln!("[kagi] compare: error: {}", e);
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "Compare failed: {}",
                    e
                )));
            }
        }
    }

    pub fn open_compare_with_working_tree(&mut self, target: CommitId) {
        use kagi::git::{compare_commit_to_workdir, working_tree_status};

        let row_index = match self.row_for_commit_id(&target) {
            Some(ix) => ix,
            None => return,
        };
        if self.selected != Some(row_index) {
            self.select(row_index);
        }

        let repo_path = match self.repo_path.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] compare: repo open error: {}", e.message());
                return;
            }
        };
        match working_tree_status(&repo) {
            Ok(status) if !status.is_dirty() => {
                eprintln!(
                    "[kagi] compare: {} <-> working tree disabled(local changes がありません)",
                    target.short()
                );
                self.status_footer = FooterStatus::Idle(SharedString::from(
                    "local changes がありません",
                ));
                return;
            }
            Err(e) => {
                eprintln!("[kagi] compare: status error: {}", e);
                return;
            }
            _ => {}
        }

        match compare_commit_to_workdir(&repo, &target) {
            Ok(files) => {
                let title = SharedString::from(format!(
                    "{} \u{2194} working tree (staged+unstaged)",
                    target.short()
                ));
                eprintln!(
                    "[kagi] compare: {} <-> working tree files={}",
                    target.short(),
                    files.len()
                );
                self.main_diff = None;
                self.compare_view = Some(CompareView {
                    base: target,
                    target: CompareTarget::WorkingTree,
                    files,
                    title,
                });
            }
            Err(e) => {
                eprintln!("[kagi] compare: error: {}", e);
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "Compare failed: {}",
                    e
                )));
            }
        }
    }

    pub fn open_compare_with_head_row(&mut self, row_index: usize) {
        match self.commit_id_for_row(row_index) {
            Some(target) => self.open_compare_with_head(target),
            None => eprintln!("[kagi] compare: row={} out of range", row_index),
        }
    }

    pub fn open_compare_with_working_tree_row(&mut self, row_index: usize) {
        match self.commit_id_for_row(row_index) {
            Some(target) => self.open_compare_with_working_tree(target),
            None => eprintln!("[kagi] compare: row={} out of range", row_index),
        }
    }

    // ── T028: branch jump ────────────────────────────────────

    /// Jump to the commit at the tip of `branch_name` in the commit list.
    ///
    /// - Scrolls the "commit-list" uniform_list to the row via the stored
    ///   [`UniformListScrollHandle`].
    /// - Selects the row (detail panel opens).
    /// - Emits `[kagi] jump: <branch> -> row N` for headless verification.
    /// - If the branch target is outside the 10 k commit window (not in
    ///   `commit_row_index`), logs a warning and returns without crashing.
    pub fn jump_to_branch(&mut self, branch_name: &str) {
        // Look up the CommitId the branch points to.
        let target = match self.branch_targets.get(branch_name) {
            Some(t) => t.clone(),
            None => {
                eprintln!("[kagi] jump: branch '{}' not found in branch_targets", branch_name);
                return;
            }
        };

        // Look up the row index for that commit.
        let row_ix = match self.commit_row_index.get(&target) {
            Some(&ix) => ix,
            None => {
                eprintln!(
                    "[kagi] jump: branch '{}' tip commit {} is outside the 10k window — cannot jump",
                    branch_name,
                    target.short()
                );
                self.status_footer = FooterStatus::Idle(SharedString::from(format!(
                    "Cannot jump to '{}': commit is outside the loaded window",
                    branch_name
                )));
                return;
            }
        };

        eprintln!("[kagi] jump: {} -> row {}", branch_name, row_ix);

        // Scroll the list so the row is visible (centered in viewport).
        self.commit_scroll_handle
            .scroll_to_item(row_ix, ScrollStrategy::Center);

        // Select the row (opens detail panel, emits selected log).
        // `select` toggles on a repeated index; a jump must stay selected.
        if self.selected != Some(row_ix) {
            self.select(row_ix);
        }
    }

    /// W2-SIDEBAR: Jump directly to a commit by its CommitId.
    ///
    /// Used for remote branch and tag clicks where there is no branch name.
    /// Scrolls the commit list to the row and selects it.
    pub fn jump_to_commit(&mut self, target: &CommitId) {
        let row_ix = match self.commit_row_index.get(target) {
            Some(&ix) => ix,
            None => {
                eprintln!(
                    "[kagi] jump: commit {} is outside the 10k window — cannot jump",
                    target.short()
                );
                self.status_footer = FooterStatus::Idle(SharedString::from(format!(
                    "Cannot jump: commit {} is outside the loaded window",
                    target.short()
                )));
                return;
            }
        };
        eprintln!("[kagi] jump: commit {} -> row {}", target.short(), row_ix);
        self.commit_scroll_handle
            .scroll_to_item(row_ix, ScrollStrategy::Center);
        // `select` toggles on a repeated index; a jump must stay selected.
        if self.selected != Some(row_ix) {
            self.select(row_ix);
        }
    }

    /// Open the commit context menu for a row, selecting the row first without
    /// toggling off an already-selected row.
    pub fn open_commit_menu(&mut self, row_index: usize, position: gpui::Point<gpui::Pixels>) {
        if self.rows.get(row_index).is_none() {
            return;
        }
        if self.selected != Some(row_index) {
            self.select(row_index);
        }
        self.commit_menu = Some(CommitMenuState { row_index, position });
        eprintln!("[kagi] context-menu: open row={}", row_index);
        self.log_commit_menu(row_index);
    }

    /// Headless path for KAGI_CONTEXT_MENU=<row>.
    pub fn open_commit_menu_headless(&mut self, row_index: usize) {
        if self.rows.get(row_index).is_none() {
            eprintln!("[kagi] context-menu: row={} out of range", row_index);
            return;
        }
        if self.selected != Some(row_index) {
            self.select(row_index);
        }
        eprintln!("[kagi] context-menu: open row={}", row_index);
        self.log_commit_menu(row_index);
    }

    fn commit_id_for_row(&self, row_index: usize) -> Option<CommitId> {
        self.details
            .get(row_index)
            .map(|detail| CommitId(detail.full_sha.as_ref().to_string()))
    }

    fn row_for_commit_id(&self, target: &CommitId) -> Option<usize> {
        self.commit_row_index.get(target).copied().or_else(|| {
            self.details
                .iter()
                .position(|detail| detail.full_sha.as_ref() == target.0)
        })
    }

    fn menu_context(&self, row_index: usize) -> Option<MenuContext> {
        let row = self.rows.get(row_index)?;
        let target = self.commit_id_for_row(row_index)?;
        let is_ancestor_of_head = if row.is_head {
            true
        } else {
            self.repo_path
                .as_ref()
                .and_then(|repo_path| git2::Repository::open(repo_path).ok())
                .and_then(|repo| {
                    let head_oid = repo.head().ok()?.target()?;
                    let target_oid = git2::Oid::from_str(&target.0).ok()?;
                    Some(
                        head_oid == target_oid
                            || repo
                                .graph_descendant_of(head_oid, target_oid)
                                .unwrap_or(false),
                    )
                })
                .unwrap_or(false)
        };

        Some(MenuContext {
            is_head: row.is_head,
            is_ancestor_of_head,
            is_merge: row.is_merge,
            dirty: self.is_dirty,
            detached: self.status_summary.is_detached,
            has_local_changes: self.is_dirty,
            refs_here: row.badges.clone(),
        })
    }

    fn log_commit_menu(&self, row_index: usize) {
        if let Some(ctx) = self.menu_context(row_index) {
            let groups = context_menu::build_commit_menu(&ctx);
            context_menu::log_commit_menu(row_index, &groups);
        }
    }

    fn render_commit_menu_overlay(
        &self,
        state: CommitMenuState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let detail = self.details.get(state.row_index)?;
        let target = self.commit_id_for_row(state.row_index)?;
        let ctx = self.menu_context(state.row_index)?;
        let groups = context_menu::build_commit_menu(&ctx);
        let title = detail.full_message.as_ref().lines().next().unwrap_or("");
        let header = context_menu::short_title_header(detail.full_sha.as_ref(), title);
        Some(context_menu::render_commit_menu_overlay(
            state, target, header, groups, window, cx,
        ))
    }

    pub fn dispatch_commit_action(
        &mut self,
        action: CommitAction,
        target: CommitId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            CommitAction::ShowDetails => {
                if let Some(row_index) = self.row_for_commit_id(&target) {
                    if self.selected != Some(row_index) {
                        self.select(row_index);
                    }
                }
            }
            CommitAction::CopySha => {
                if let Some(row_index) = self.row_for_commit_id(&target) {
                    if let Some(detail) = self.details.get(row_index) {
                        context_menu::copy_full_sha(
                            self,
                            detail.full_sha.as_ref().to_string(),
                            cx,
                        );
                    }
                }
            }
            CommitAction::CopyShortSha => {
                if let Some(row_index) = self.row_for_commit_id(&target) {
                    if let Some(detail) = self.details.get(row_index) {
                        let full_sha = detail.full_sha.as_ref().to_string();
                        context_menu::copy_short_sha(self, &full_sha, cx);
                    }
                }
            }
            CommitAction::CopyMessage => {
                if let Some(row_index) = self.row_for_commit_id(&target) {
                    if let Some(detail) = self.details.get(row_index) {
                        let full_sha = detail.full_sha.as_ref().to_string();
                        context_menu::copy_message(
                            self,
                            &full_sha,
                            detail.full_message.as_ref().to_string(),
                            cx,
                        );
                    }
                }
            }
            CommitAction::CheckoutCommit => {
                self.open_checkout_commit_modal(target);
            }
            CommitAction::CheckoutRef(ref_name) => {
                if ref_name.is_empty() {
                    self.status_footer =
                        FooterStatus::Idle(SharedString::from("Checkout ref unavailable"));
                    eprintln!("[kagi] context-menu: checkout-ref unavailable {}", target.short());
                } else {
                    self.open_plan_modal(ref_name);
                }
            }
            CommitAction::CreateBranchHere => {
                self.open_create_branch_modal(target, cx);
                eprintln!("[kagi] context-menu: create-branch {}", self.create_branch_modal.as_ref().map(|m| m.at.short()).unwrap_or_default());
            }
            CommitAction::CreateWorktreeHere => {
                self.open_create_worktree_modal(target, cx);
                eprintln!("[kagi] context-menu: create-worktree {}", self.create_worktree_modal.as_ref().map(|m| m.at.short()).unwrap_or_default());
            }
            CommitAction::CherryPick => {
                self.open_cherry_pick_modal(target);
            }
            CommitAction::Revert => {
                self.open_revert_modal(target);
            }
            // ADR-0024: reset stays unimplemented; the menu item is disabled,
            // this arm is defence in depth.
            CommitAction::ResetToCommit => {
                self.status_footer = FooterStatus::Idle(SharedString::from(
                    "Reset 未実装 (ADR-0024)",
                ));
                eprintln!("[kagi] context-menu: stub Reset {}", target.short());
            }
            CommitAction::CompareWithHead => {
                self.open_compare_with_head(target);
            }
            CommitAction::CompareWithWorkingTree => {
                self.open_compare_with_working_tree(target);
            }
            CommitAction::ShowChangedFiles => {
                self.show_changed_files_for_commit(target);
            }
        }
    }

    /// W2-SIDEBAR: Lazily create the sidebar filter InputState (requires &mut Window).
    ///
    /// Called from the on_click handler on the filter placeholder area.
    /// No-op if already created.
    pub fn ensure_sidebar_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sidebar_filter.is_none() {
            let input_entity = cx.new(|cx| InputState::new(window, cx).placeholder("filter…"));
            self.sidebar_filter = Some(input_entity);
        }
        // Focus the input after creation (or if already exists).
        if let Some(ref ent) = self.sidebar_filter {
            ent.update(cx, |state, cx| {
                state.focus(window, cx);
            });
        }
    }
}

impl Render for KagiApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // W2-STATUS / ADR-0017: resolve the bottom-panel default height on
        // first render, once the viewport size is known (18% of viewport).
        if self.bottom_panel_height <= BOTTOM_PANEL_H_UNSET {
            let viewport_h = f32::from(window.viewport_size().height);
            let h = (viewport_h * BOTTOM_PANEL_DEFAULT_FRAC).max(BOTTOM_PANEL_MIN_H);
            self.bottom_panel_height = h;
            eprintln!(
                "[kagi] bottom-panel: default height={:.0} ({:.0}% of viewport {:.0})",
                h, BOTTOM_PANEL_DEFAULT_FRAC * 100.0, viewport_h
            );
        }

        // W11-AVATAR: kick off GitHub avatar resolution once per repo (no-op
        // for non-GitHub repos / offline / already-started).
        self.ensure_avatars(cx);

        // W3-NOTIFY: drop expired toasts and keep the auto-dismiss ticker
        // alive while any remain.
        self.toasts.retain(|t| !t.expired());
        self.ensure_toast_ticker(cx);

        // Modal text inputs: lazy-create + sync (needs Window).
        self.sync_modal_inputs(window, cx);

        // Graph horizontal scroll: clamp against the current repo's lane
        // count so the offset self-heals after tab switches and column
        // resizes.
        {
            let lane_count = self.rows.first().map(|r| r.lane_count).unwrap_or(0);
            let max = (lane_count as f32 * graph_view::LANE_W - self.graph_col_w).max(0.0);
            if self.graph_scroll_x > max {
                self.graph_scroll_x = max;
            }
        }

        let row_count = self.rows.len();
        let selected = self.selected;

        // W4-TABS / ADR-0028: a non-empty error string still shows the error
        // screen (genuine repo-open failure at startup; headless log compat).
        if let Some(err) = self.error.clone().filter(|e| !e.is_empty()) {
            // ── Error / usage state ──────────────────────────
            return div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .size_full()
                .bg(rgb(theme().bg_base))
                .child(
                    div()
                        .text_xl()
                        .text_color(rgb(theme().text_main))
                        .child(err),
                )
                .into_any();
        }

        // W4-TABS / ADR-0028: no open tabs → Welcome screen.
        if self.tabs.is_empty() {
            return self.render_welcome(cx).into_any();
        }

        // ── Pre-fetch detail for panel (if any row is selected) ─
        let detail = selected.and_then(|i| self.details.get(i)).cloned();
        // Clone cached changed-files list for the render closure.
        // `None` outer = no selection; `Some(None)` = diff unavailable; `Some(Some(v))` = files.
        let changed_files: Option<Option<Vec<FileStatus>>> = selected
            .map(|i| self.diff_cache.get(&i).cloned().unwrap_or(None));
        // W2-INSPECTOR: badges for the selected commit row and tree-view toggle state.
        let selected_badges: Vec<commit_list::RefBadge> =
            selected.and_then(|i| self.rows.get(i)).map(|r| r.badges.clone()).unwrap_or_default();
        let inspector_tree_view = self.inspector_tree_view;

        // T-UI-003: Clone main diff state if present.
        let main_diff = self.main_diff.clone();
        let compare_view = self.compare_view.clone();
        let main_diff_scroll_handle = self.main_diff_scroll_handle.clone();

        // Clone branch list and modal state for render.
        let branches = self.branches.clone();
        let stashes = self.stashes.clone();
        let is_dirty = self.is_dirty;
        // W2-SIDEBAR: clone navigator data for sidebar render.
        let remote_branches = self.remote_branches.clone();
        let tags = self.tags.clone();
        let worktrees = self.worktrees.clone();
        let branch_upstream_info = self.branch_upstream_info.clone();
        let sidebar_collapsed = self.sidebar_collapsed.clone();
        let sidebar_filter = self.sidebar_filter.clone();
        let plan_modal = self.plan_modal.clone();
        let pull_modal = self.pull_modal.clone();
        let undo_modal = self.undo_modal.clone();
        let pop_modal = self.pop_modal.clone();
        let push_modal = self.push_modal.clone();
        let create_branch_modal = self.create_branch_modal.clone();
        let create_worktree_modal = self.create_worktree_modal.clone();
        let delete_branch_modal = self.delete_branch_modal.clone();
        let modal_focus = self.modal_focus.clone();
        let stash_push_modal = self.stash_push_modal.clone();
        let stash_push_focus = self.stash_push_focus.clone();
        let stash_apply_modal = self.stash_apply_modal.clone();
        let cherry_pick_modal = self.cherry_pick_modal.clone();
        let revert_modal = self.revert_modal.clone();
        let status_footer = self.status_footer.clone();
        let commit_menu_overlay = self
            .commit_menu
            .clone()
            .and_then(|state| self.render_commit_menu_overlay(state, window, cx));
        // T-HT-001: clone toolbar/summary state for header render.
        // W3-NOTIFY: while a background git op runs, disable every git button
        // so operations never overlap.
        let mut toolbar_state = self.toolbar_state.clone();
        if self.busy_op.is_some() {
            toolbar_state.pull_on = false;
            toolbar_state.push_on = false;
            toolbar_state.stash_on = false;
            toolbar_state.pop_on = false;
            toolbar_state.undo_on = false;
        }
        let status_summary = self.status_summary.clone();

        // T023: pane widths for divider rendering.
        let sidebar_width = self.sidebar_width;
        let panel_width = self.panel_width;
        // T030: inner column widths for the commit list.
        let badge_col_w = self.badge_col_w;
        let graph_col_w = self.graph_col_w;

        // T028: clone scroll handle for wiring into uniform_list via track_scroll.
        let commit_scroll_handle = self.commit_scroll_handle.clone();

        // T023: divider drag-move handler callback (single listener handles both dividers).
        // Placed on the root div so it fires even when the mouse moves outside
        // the narrow 4px divider strip.
        // Widths are derived from the ABSOLUTE cursor position, not deltas:
        // the sidebar starts at the window's left edge and the panel ends at
        // its right edge, so the divider should simply track the cursor.
        // (The previous delta-based approach needed a drag-start anchor that
        // `on_drag` cannot provide, which made the divider jump to its
        // clamp bounds — the "two positions / inverted" bug.)
        let divider_drag_move = cx.listener(move |this, event: &gpui::DragMoveEvent<DividerDrag>, window, cx| {
            let drag = *event.drag(cx);
            let cursor_x = f32::from(event.event.position.x);
            match drag.kind {
                DividerKind::Sidebar => {
                    // Divider sits at x = sidebar_width; centre it on the cursor.
                    let new_width = (cursor_x - 2.0).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
                    if (new_width - this.sidebar_width).abs() > 0.5 {
                        this.sidebar_width = new_width;
                        cx.notify();
                    }
                }
                DividerKind::Panel => {
                    // Divider sits at x = viewport_width - panel_width.
                    let viewport_w = f32::from(window.viewport_size().width);
                    let new_width = (viewport_w - cursor_x - 2.0).clamp(PANEL_MIN, PANEL_MAX);
                    if (new_width - this.panel_width).abs() > 0.5 {
                        this.panel_width = new_width;
                        cx.notify();
                    }
                }
                DividerKind::BadgeCol => {
                    // T030: badge column left edge = sidebar_width + INNER_DIV_W (sidebar divider).
                    // badge_col_w = cursor_x - badge_col_left_edge
                    let badge_col_left = this.sidebar_width + INNER_DIV_W; // sidebar divider = 4px
                    let new_w = (cursor_x - badge_col_left - INNER_DIV_W / 2.0)
                        .clamp(BADGE_COL_MIN, BADGE_COL_MAX);
                    if (new_w - this.badge_col_w).abs() > 0.5 {
                        this.badge_col_w = new_w;
                        cx.notify();
                    }
                }
                DividerKind::GraphCol => {
                    // T030: graph column left edge = badge_col_left_edge + badge_col_w + INNER_DIV_W
                    let badge_col_left = this.sidebar_width + INNER_DIV_W;
                    let graph_col_left = badge_col_left + this.badge_col_w + INNER_DIV_W;
                    let new_w = (cursor_x - graph_col_left - INNER_DIV_W / 2.0)
                        .clamp(GRAPH_COL_MIN, GRAPH_COL_MAX);
                    if (new_w - this.graph_col_w).abs() > 0.5 {
                        this.graph_col_w = new_w;
                        cx.notify();
                    }
                }
                DividerKind::BottomPanel => {
                    // T-BP-002: absolute-coordinate formula from ADR-0007:
                    //   height = viewport_h - cursor_y - status_bar_h(22) - 2
                    let viewport_h = f32::from(window.viewport_size().height);
                    let cursor_y = f32::from(event.event.position.y);
                    let max_h = viewport_h * BOTTOM_PANEL_MAX_FRAC;
                    let new_h = (viewport_h - cursor_y - 22.0 - 2.0)
                        .clamp(BOTTOM_PANEL_MIN_H, max_h);
                    if (new_h - this.bottom_panel_height).abs() > 0.5 {
                        this.bottom_panel_height = new_h;
                        cx.notify();
                    }
                }
                DividerKind::InspectorSplit => {
                    // W7-INSPECTOR2: absolute-coordinate ratio against the
                    // *measured* message+files region (paint-time canvas in
                    // inspector.rs).  Static offsets miss the variable-height
                    // header above the region, which showed up as a ~2cm jump
                    // when starting a drag.  Falls back to the constant-based
                    // approximation until the first paint has run.
                    let cursor_y = f32::from(event.event.position.y);
                    let (geom_top, geom_bottom) = this.inspector_geom.get();
                    let (top, bottom) = if geom_bottom - geom_top > 1.0 {
                        (geom_top, geom_bottom)
                    } else {
                        let viewport_h = f32::from(window.viewport_size().height);
                        let bottom_taken = if this.bottom_panel_open {
                            STATUS_BAR_H + this.bottom_panel_height + BOTTOM_PANEL_DIVIDER_H
                        } else {
                            STATUS_BAR_H
                        };
                        (INSPECTOR_TOP_OFFSET, viewport_h - bottom_taken)
                    };
                    // The divider itself occupies INSPECTOR_SPLIT_DIVIDER_H of
                    // the region; the flex split applies to the remainder.
                    let span = bottom - top - inspector::INSPECTOR_SPLIT_DIVIDER_H;
                    if std::env::var("KAGI_DEBUG_SPLIT").as_deref() == Ok("1") {
                        eprintln!(
                            "[kagi] split-drag: cursor_y={:.1} top={:.1} bottom={:.1} split={:.3}",
                            cursor_y, top, bottom, this.inspector_split
                        );
                    }
                    if span > 1.0 {
                        let ratio = ((cursor_y - top) / span)
                            .clamp(INSPECTOR_SPLIT_MIN, INSPECTOR_SPLIT_MAX);
                        if (ratio - this.inspector_split).abs() > 0.001 {
                            this.inspector_split = ratio;
                            cx.notify();
                        }
                    }
                }
            }
        });

        // T025/T026: extract commit panel state for render.
        let commit_panel_open = self.commit_panel_open;
        let commit_panel = self.commit_panel.clone();
        let commit_input = self.commit_input.clone();

        // T-BP-002: bottom panel state.
        let bottom_panel_open = self.bottom_panel_open;
        let bottom_panel_height = self.bottom_panel_height;
        let bottom_tab = self.bottom_tab;

        // T-BP-002: cmd-j toggle action handler.
        let toggle_bottom_panel = cx.listener(|this, _: &ToggleBottomPanel, _window, cx| {
            this.bottom_panel_open = !this.bottom_panel_open;
            cx.notify();
        });

        // T-UI-003: Esc closes the main diff view (no-op when main_diff is None).
        let close_main_diff = cx.listener(|this, _: &CloseMainDiff, _window, cx| {
            if this.commit_menu.is_some() {
                this.commit_menu = None;
                cx.notify();
            } else if this.main_diff.is_some() {
                this.close_main_diff();
                cx.notify();
            }
        });

        // ── Normal state: header + body + bottom panel slot + status bar ─────
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(theme().bg_base))
            // Key events only dispatch along the focus path, so the root must
            // own (and initially hold) focus for window-wide actions to work.
            .when_some(self.root_focus.clone(), |el, fh| el.track_focus(&fh))
            // T023: capture drag-move for both dividers on the root element.
            .on_drag_move::<DividerDrag>(divider_drag_move)
            // T-BP-002: cmd-j toggle action (window-wide via on_action on root div).
            .on_action(toggle_bottom_panel)
            // T-UI-003: Esc closes the main diff view.
            .on_action(close_main_diff)
            .on_action(cx.listener(|this, _: &DiffPrevFile, _window, cx| {
                this.main_diff_step(-1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &DiffNextFile, _window, cx| {
                this.main_diff_step(1);
                cx.notify();
            }))
            // ── W5-MENU / ADR-0029: conditional command handlers ──────────
            // Each menu action's handler is registered on the focused root ONLY
            // when `command_state == Enabled`.  gpui's macOS menu validation
            // (`is_action_available`, walks the dispatch tree) then greys out
            // any command whose handler is absent — the ADR-0029 disabled model.
            .map(|el| self.register_menu_actions(el, cx))
            // ── W4-TABS: repository tab strip (above the header toolbar) ──
            .children(self.render_tab_strip(cx))
            // ── Header slot ──────────────────────────────────
            // ADR-0013: pass HEAD commit summary for Undo label (first row = HEAD).
            .child(self.render_header_slot(toolbar_state, status_summary, self.rows.first().map(|r| r.summary.to_string()), cx))
            // ── Body slot: sidebar | list | optional panel ───
            .child(self.render_body(
                row_count, selected, detail, changed_files, selected_badges, inspector_tree_view,
                main_diff, compare_view, main_diff_scroll_handle,
                branches, remote_branches, tags, stashes, worktrees, branch_upstream_info,
                sidebar_collapsed, sidebar_filter,
                is_dirty, sidebar_width, panel_width,
                badge_col_w, graph_col_w, commit_scroll_handle,
                commit_panel_open, commit_panel.clone(), commit_input.clone(),
                cx,
            ))
            // ── Bottom panel slot (T-BP-002) ─────────────────
            .children(self.render_bottom_panel_slot(bottom_panel_open, bottom_panel_height, bottom_tab, cx))
            // ── Commit context menu overlay (below modals) ─────
            .children(commit_menu_overlay)
            // ── W5-MENU: menu-driven overlay (branch picker / About / shortcuts) ──
            .children(self.render_menu_overlay(cx))
            // ── Plan modal overlay (above everything) ──────
            .when_some(plan_modal, |el, modal| {
                el.child(render_plan_modal(modal, cx))
            })
            // ── Pull plan modal overlay (T-HT-003) ──────────
            .when_some(pull_modal, |el, modal| {
                el.child(render_pull_modal(modal, cx))
            })
            // ── Undo / Pop plan modal overlays ───────────────
            .when_some(undo_modal, |el, modal| {
                el.child(render_undo_modal(modal, cx))
            })
            .when_some(pop_modal, |el, modal| {
                el.child(render_pop_modal(modal, cx))
            })
            // ── Push plan modal overlay (T-HT-004) ──────────
            .when_some(push_modal, |el, modal| {
                el.child(render_push_modal(modal, cx))
            })
            // ── Create-branch modal overlay (above everything) ──
            .when_some(create_branch_modal, |el, modal| {
                el.child(render_create_branch_modal(modal, modal_focus.clone(), cx))
            })
            // ── Create-worktree modal overlay ───────────────
            .when_some(create_worktree_modal, |el, modal| {
                el.child(render_create_worktree_modal(modal, modal_focus.clone(), cx))
            })
            // ── Stash push modal overlay ─────────────────────
            .when_some(stash_push_modal, |el, modal| {
                el.child(render_stash_push_modal(modal, stash_push_focus, cx))
            })
            // ── Stash apply modal overlay ────────────────────
            .when_some(stash_apply_modal, |el, modal| {
                el.child(render_stash_apply_modal(modal, cx))
            })
            // ── Cherry-pick modal overlay (T016) ────────────
            .when_some(cherry_pick_modal, |el, modal| {
                el.child(render_cherry_pick_modal(modal, cx))
            })
            // ── Revert modal overlay (T-CM-034) ──────────────
            .when_some(revert_modal, |el, modal| {
                el.child(render_revert_modal(modal, cx))
            })
            // ── Delete-branch modal overlay (W2-DELETE) ──────
            .when_some(delete_branch_modal, |el, modal| {
                el.child(render_delete_branch_modal(modal, cx))
            })
            // ── Commit plan modal overlay (T025) ─────────────
            .when(
                commit_panel_open && commit_panel.as_ref().and_then(|p| p.plan_modal.as_ref()).is_some(),
                |el| {
                    if let Some(Some(plan_modal)) = commit_panel.as_ref().map(|p| p.plan_modal.clone()) {
                        el.child(render_commit_plan_modal(plan_modal, cx))
                    } else {
                        el
                    }
                },
            )
            // ── Status bar slot (T017) — last operation result ─
            .child(self.render_status_bar(status_footer, bottom_panel_open, cx))
            // ── W3-NOTIFY: toast stack (above everything) ──────
            .children(self.render_toasts(cx))
            .into_any()
    }

}

// ── AppShell layout slots ────────────────────────────────────────────────────
// ADR-0007 / T-BP-001: KagiApp::render is decomposed into four vertical
// flex slots.  Each slot is a plain method so that later tickets
// (T-BP-002, T-HT-001, …) can extend their signatures without
// touching the caller site.
impl KagiApp {
    /// W5-MENU / ADR-0029: register an `on_action` handler for every menu
    /// command, **but only when that command is currently enabled**.  Leaving a
    /// handler unregistered is exactly how macOS greys the matching menu item
    /// out (gpui validates each item via `is_action_available`, which checks the
    /// dispatch tree).  All handlers funnel into `handle_menu_command`, so the
    /// behaviour stays in `commands.rs` (no menu-specific logic lives here).
    fn register_menu_actions(&self, el: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        use commands as cmds;

        // Helper: conditionally attach one action handler bound to its registry
        // id.  `$ty` is the gpui Action type; `$id` is the registry id string.
        macro_rules! menu_act {
            ($el:expr, $ty:ty, $id:literal) => {{
                let enabled = cmds::is_enabled(self, $id);
                $el.when(enabled, |el| {
                    el.on_action(cx.listener(|this, _: &$ty, window, cx| {
                        this.handle_menu_command($id, window, cx);
                    }))
                })
            }};
        }

        let el = menu_act!(el, cmds::About, "app.about");
        let el = menu_act!(el, cmds::Quit, "app.quit");
        let el = menu_act!(el, cmds::NewTab, "file.newTab");
        let el = menu_act!(el, cmds::CloseTab, "file.closeTab");
        let el = menu_act!(el, cmds::CloneRepository, "file.cloneRepository");
        let el = menu_act!(el, cmds::OpenRepository, "file.openRepository");
        let el = menu_act!(el, cmds::OpenInTerminal, "file.openInTerminal");
        let el = menu_act!(el, cmds::RefreshRepository, "file.refresh");
        let el = menu_act!(el, cmds::ZoomIn, "view.zoomIn");
        let el = menu_act!(el, cmds::ZoomOut, "view.zoomOut");
        let el = menu_act!(el, cmds::ZoomReset, "view.zoomReset");
        let el = menu_act!(el, cmds::EnterFullScreen, "view.fullScreen");
        let el = menu_act!(el, cmds::ToggleSidebar, "view.toggleSidebar");
        let el = menu_act!(el, cmds::ToggleCommitDetails, "view.toggleCommitDetails");
        let el = menu_act!(el, cmds::ToggleDiffView, "view.toggleDiffView");
        let el = menu_act!(el, cmds::Fetch, "repo.fetch");
        let el = menu_act!(el, cmds::Pull, "repo.pull");
        let el = menu_act!(el, cmds::Push, "repo.push");
        let el = menu_act!(el, cmds::OpenInFinder, "repo.openInFinder");
        let el = menu_act!(el, cmds::NewBranch, "branch.new");
        let el = menu_act!(el, cmds::CheckoutBranch, "branch.checkout");
        let el = menu_act!(el, cmds::RenameBranch, "branch.rename");
        let el = menu_act!(el, cmds::DeleteBranch, "branch.delete");
        let el = menu_act!(el, cmds::CopyCommitHash, "commit.copyHash");
        let el = menu_act!(el, cmds::CheckoutCommit, "commit.checkout");
        let el = menu_act!(el, cmds::CreateBranchFromCommit, "commit.createBranch");
        let el = menu_act!(el, cmds::CherryPickCommit, "commit.cherryPick");
        let el = menu_act!(el, cmds::RevertCommit, "commit.revert");
        let el = menu_act!(el, cmds::ResetToCommit, "commit.reset");
        let el = menu_act!(el, cmds::CompareWithWorkingTree, "commit.compareWorkingTree");
        let el = menu_act!(el, cmds::MinimizeWindow, "window.minimize");
        let el = menu_act!(el, cmds::ZoomWindow, "window.zoom");
        let el = menu_act!(el, cmds::NewWindow, "window.new");
        let el = menu_act!(el, cmds::CloseWindow, "window.close");
        let el = menu_act!(el, cmds::KeyboardShortcuts, "help.shortcuts");
        let el = menu_act!(el, cmds::Documentation, "help.documentation");
        let el = menu_act!(el, cmds::ReportIssue, "help.reportIssue");
        // W9-THEME: theme switch actions (always enabled).
        let el = menu_act!(el, cmds::ThemeCatppuccin, "theme.catppuccin");
        let el = menu_act!(el, cmds::ThemeXcodeDark, "theme.xcodeDark");
        let el = menu_act!(el, cmds::ThemeXcodeLight, "theme.xcodeLight");
        let el = menu_act!(el, cmds::ThemeOneDark, "theme.oneDark");
        let el = menu_act!(el, cmds::ThemeOneLight, "theme.oneLight");
        let el = menu_act!(el, cmds::ThemeMonokai, "theme.monokai");
        el
    }

    /// Header slot — the Toolbar bar (T-HT-001 / ADR-0013).
    ///
    /// Layout (34 px):
    ///   LEFT:   repo-name | branch → upstream ↑A ↓B
    ///   CENTRE: Pull(↓N) Push(↑N) | Branch Stash Pop | Undo("<summary>") Terminal
    ///   RIGHT:  Refresh
    fn render_header_slot(
        &mut self,
        toolbar: ToolbarState,
        summary: StatusBarSummary,
        // HEAD commit summary for Undo label (first row in commit list). ADR-0013.
        undo_summary: Option<String>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // ── Click handlers ──────────────────────────────────────────────────
        // Pull — disabled when behind=0 or no upstream (ADR-0013).
        let pull_on = toolbar.pull_on;
        let pull_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if pull_on {
                this.open_pull_modal();
            } else {
                let reason = if this.busy_op.is_some() {
                    "Pull: 別の操作が実行中です"
                } else if this.status_summary.is_detached {
                    "Pull: detached HEAD — branch に切り替えてください"
                } else if this.status_summary.is_unborn {
                    "Pull: no commits yet — upstream がありません"
                } else if this.status_summary.no_upstream {
                    "Pull: upstream が設定されていません (no upstream)"
                } else {
                    "Pull: nothing to pull (behind=0)"
                };
                this.status_footer = FooterStatus::Idle(SharedString::from(reason));
            }
            cx.notify();
        });

        // Push (T-HT-004).
        let push_on = toolbar.push_on;
        let push_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if push_on {
                this.open_push_modal();
            } else {
                let reason = if this.busy_op.is_some() {
                    "Push: 別の操作が実行中です"
                } else if this.status_summary.is_detached {
                    "Push: detached HEAD — branch に切り替えてください"
                } else if this.status_summary.is_unborn {
                    "Push: no commits yet — upstream がありません"
                } else if this.status_summary.no_upstream && !this.status_summary.has_remote {
                    "Push: no upstream and no remote configured"
                } else {
                    "Push: nothing to push (ahead=0)"
                };
                this.status_footer = FooterStatus::Idle(SharedString::from(reason));
            }
            cx.notify();
        });

        // Branch — always enabled; use selected commit if any, else HEAD.
        let branch_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
            // Resolve target commit: selected row → HEAD commit (first detail).
            let at = this.selected
                .and_then(|i| this.details.get(i))
                .map(|d| CommitId(d.full_sha.to_string()))
                .or_else(|| {
                    // Fall back to HEAD commit (first detail entry).
                    this.details.first().map(|d| CommitId(d.full_sha.to_string()))
                });
            if let Some(id) = at {
                this.open_create_branch_modal(id, cx);
            }
            cx.notify();
        });

        // Stash — enabled only when dirty.
        let stash_on = toolbar.stash_on;
        let stash_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if stash_on {
                this.open_stash_push_modal(cx);
            } else {
                this.status_footer = FooterStatus::Idle(SharedString::from(
                    "Stash: working tree is clean — nothing to stash",
                ));
            }
            cx.notify();
        });

        // Pop — enabled only when stash exists.
        let pop_on = toolbar.pop_on;
        let pop_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if pop_on {
                // Pop the newest stash (index 0) — plan with conflict prediction.
                this.open_pop_modal(0);
            } else {
                this.status_footer = FooterStatus::Idle(SharedString::from(
                    "Pop: stash が空です",
                ));
            }
            cx.notify();
        });

        // Undo (not implemented yet — footer notice only).
        let undo_on = toolbar.undo_on;
        let undo_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if undo_on {
                this.open_undo_modal();
            } else {
                let reason = if this.status_summary.is_detached {
                    "Undo: detached HEAD — undo できません"
                } else if this.status_summary.is_unborn {
                    "Undo: no commits yet — undo できません"
                } else {
                    "Undo: ahead=0 — push 済みの commit はここでは undo できません"
                };
                this.status_footer = FooterStatus::Idle(SharedString::from(reason));
            }
            cx.notify();
        });

        // Refresh — always enabled.
        let refresh_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
            this.reload();
            this.status_footer = FooterStatus::Idle(SharedString::from("Refreshed"));
            // W3-NOTIFY: explicit refresh gets a completion toast (the
            // watcher's automatic reloads stay silent to avoid spam).
            this.push_toast(ToastKind::Success, "Refreshed");
            cx.notify();
        });

        // Terminal — toggles bottom panel to Terminal tab (ADR-0013).
        let terminal_on = self.bottom_panel_open && self.bottom_tab == BottomTab::Terminal;
        let terminal_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            if this.bottom_panel_open && this.bottom_tab == BottomTab::Terminal {
                // Same tab visible → close panel (toggle off).
                this.bottom_panel_open = false;
            } else {
                this.bottom_panel_open = true;
                this.bottom_tab = BottomTab::Terminal;
                // T-BP-007: lazy-start terminal session when first opened.
                this.ensure_terminal(window, cx);
            }
            cx.notify();
        });

        // ── Helper: build a single Finder/Keynote-style toolbar button ──────
        // W10-TOOLBAR: icon on top (20px ≈ Size::Medium), text_xs label below,
        // vertically stacked. Whole button gets a hover bg + rounded; width is
        // content-fit with a shared min-width so the row reads as a grid.
        //
        // `id` must be a unique string for GPUI element tracking.
        // `count` (>0) renders a small chip overlay at the icon's top-right;
        // 0 hides it (ADR-0013: Pull ↓N / Push ↑N).
        // `enabled` drives muted colour; disabled buttons keep their click
        // handler (which sets the reason footer) but render in muted colour.
        let make_btn = |id: &'static str,
                        label: &'static str,
                        icon: gpui_component::IconName,
                        enabled: bool,
                        count: usize| {
            let text_color = if enabled { theme().text_main } else { theme().text_muted };
            let chip_bg = theme().color_branch;
            let chip_fg = theme().bg_base;

            // Icon cell — `.relative()` so the count chip can be `.absolute()`
            // anchored to the icon's top-right corner (gpui has no negative
            // clip, so the chip is placed inside the icon bounds).
            let mut icon_cell = div()
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .w(px(22.0))
                .h(px(22.0))
                .child(
                    gpui_component::Icon::new(icon)
                        .with_size(gpui_component::Size::Size(px(20.0)))
                        .text_color(rgb(text_color)),
                );
            if count > 0 {
                let chip_text = if count > 99 { "99+".to_string() } else { count.to_string() };
                icon_cell = icon_cell.child(
                    div()
                        .absolute()
                        .top(px(-2.0))
                        .right(px(-2.0))
                        .min_w(px(14.0))
                        .h(px(14.0))
                        .px(px(3.0))
                        .rounded_full()
                        .bg(rgb(chip_bg))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(chip_fg))
                        .text_size(px(9.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .line_height(px(14.0))
                        .child(SharedString::from(chip_text)),
                );
            }

            div()
                .id(id)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(1.0))
                .min_w(px(52.0))
                .px_1()
                .py(px(2.0))
                .rounded_md()
                .hover(|style| style.bg(rgb(theme().selected)))
                .cursor(if enabled { gpui::CursorStyle::PointingHand } else { gpui::CursorStyle::Arrow })
                .child(icon_cell)
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(text_color))
                        .child(SharedString::from(label)),
                )
        };

        // ── Undo tooltip: target HEAD commit summary (ADR-0013) ─────────────
        // Label stays the fixed "Undo"; the (possibly long) commit summary is
        // surfaced on hover instead of being truncated into the label.
        let undo_tooltip_text: Option<SharedString> = if toolbar.undo_on {
            undo_summary
                .as_ref()
                .filter(|s| !s.is_empty())
                .map(|s| SharedString::from(format!("Undo: \u{201c}{}\u{201d}", s)))
        } else {
            None
        };

        // ── Left label: branch info (ADR-0013) ─────────────────────────────
        // Format: `branch → upstream ↑A ↓B`  or state labels when detached/unborn.
        let branch_label = if summary.is_detached {
            "detached HEAD".to_string()
        } else if summary.is_unborn {
            "no commits yet".to_string()
        } else if summary.no_upstream {
            format!("{} (no upstream)", summary.branch)
        } else {
            let ahead = summary.ahead.unwrap_or(0);
            let behind = summary.behind.unwrap_or(0);
            if summary.upstream_name.is_empty() {
                format!("{} \u{2191}{} \u{2193}{}", summary.branch, ahead, behind)
            } else {
                format!(
                    "{} \u{2192} {} \u{2191}{} \u{2193}{}",
                    summary.branch, summary.upstream_name, ahead, behind
                )
            }
        };

        // ── Vertical separator ──────────────────────────────────────────────
        let sep = || {
            div()
                .w(px(1.0))
                .h(px(16.0))
                .bg(rgb(theme().text_muted))
                .mx_1()
                .flex_shrink_0()
        };

        // ── Toolbar bar (52 px — W10-TOOLBAR vertical buttons) ──────────────
        div()
            .id("toolbar-bar")
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .px_3()
            .h(px(52.0))
            .flex_shrink_0()
            .bg(rgb(theme().surface))
            .text_color(rgb(theme().text_sub))
            // ── LEFT: Refresh (user request: left of the repo title) ──
            .child(
                div()
                    .id("tb-refresh")
                    .flex_shrink_0()
                    .mr_2()
                    .p_1()
                    .rounded_md()
                    .hover(|st| st.bg(rgb(theme().selected)).cursor_pointer())
                    .on_click(refresh_click)
                    .child(
                        gpui::svg()
                            .path("icons/refresh-cw.svg")
                            .w(px(16.0))
                            .h(px(16.0))
                            .text_color(rgb(theme().text_main)),
                    ),
            )
            // ── repo name + branch/upstream/ahead-behind ──
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_main))
                    .font_weight(gpui::FontWeight::BOLD)
                    .mr_1()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .child(SharedString::from(summary.repo_name.clone())),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_sub))
                    .mr_2()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .child(SharedString::from(branch_label)),
            )
            .child(sep())
            // ── CENTRE: Pull Push | Branch Stash Pop | Undo Terminal ──
            // Pull (↓N chip when behind>0)
            .child(
                make_btn("tb-pull", "Pull", gpui_component::IconName::ArrowDown, toolbar.pull_on, toolbar.behind)
                    .on_click(pull_click),
            )
            .child(div().w(px(2.0)))
            // Push (↑N chip when ahead>0)
            .child(
                make_btn("tb-push", "Push", gpui_component::IconName::ArrowUp, toolbar.push_on, toolbar.ahead)
                    .on_click(push_click),
            )
            .child(sep())
            // Branch
            .child(
                make_btn("tb-branch", "Branch", gpui_component::IconName::Plus, true, 0)
                    .on_click(branch_click),
            )
            .child(div().w(px(2.0)))
            // Stash
            .child(
                make_btn("tb-stash", "Stash", gpui_component::IconName::Inbox, toolbar.stash_on, 0)
                    .on_click(stash_click),
            )
            .child(div().w(px(2.0)))
            // Pop
            .child(
                make_btn("tb-pop", "Pop", gpui_component::IconName::FolderOpen, toolbar.pop_on, 0)
                    .on_click(pop_click),
            )
            .child(sep())
            // Undo — fixed "Undo" label; target commit summary in tooltip.
            .child(
                make_btn("tb-undo", "Undo", gpui_component::IconName::Undo2, toolbar.undo_on, 0)
                    .when_some(undo_tooltip_text, |btn, text| {
                        btn.tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
                    })
                    .on_click(undo_click),
            )
            .child(div().w(px(2.0)))
            // Terminal (toggles bottom panel Terminal tab)
            .child(
                make_btn("tb-terminal", "Terminal", gpui_component::IconName::SquareTerminal, terminal_on, 0)
                    .on_click(terminal_click),
            )
            .child(div().flex_1())
    }

    /// Body slot — the main content area: sidebar | divider | commit list | optional panel.
    ///
    /// All parameters are pre-cloned values from `render`; no additional
    /// state access is performed inside this method.
    #[allow(clippy::too_many_arguments)]
    fn render_body(
        &mut self,
        row_count: usize,
        selected: Option<usize>,
        detail: Option<detail_panel::CommitDetail>,
        changed_files: Option<Option<Vec<FileStatus>>>,
        selected_badges: Vec<commit_list::RefBadge>,
        inspector_tree_view: bool,
        main_diff: Option<MainDiffView>,
        compare_view: Option<CompareView>,
        main_diff_scroll_handle: UniformListScrollHandle,
        branches: Vec<(String, bool)>,
        remote_branches: Vec<RemoteBranch>,
        tags: Vec<Tag>,
        stashes: Vec<kagi::git::Stash>,
        worktrees: Vec<Worktree>,
        branch_upstream_info: HashMap<String, UpstreamInfo>,
        sidebar_collapsed: HashSet<&'static str>,
        sidebar_filter: Option<Entity<InputState>>,
        is_dirty: bool,
        sidebar_width: f32,
        panel_width: f32,
        badge_col_w: f32,
        graph_col_w: f32,
        commit_scroll_handle: UniformListScrollHandle,
        commit_panel_open: bool,
        commit_panel: Option<commit_panel::CommitPanelState>,
        commit_input: Option<Entity<InputState>>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // W11-AVATAR: snapshot the resolved avatar images so the inspector can
        // swap the initial circle for a real image without re-borrowing self.
        let avatar_images = self.avatar_images.clone();
        // Build divider 1: sidebar | main.
        let divider1 = div()
            .id("divider-sidebar")
            .w(px(4.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(theme().surface))
            .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
            .cursor_col_resize()
            .on_drag(
                DividerDrag { kind: DividerKind::Sidebar },
                |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
            );

        // ── WIP row (shown above the list when working tree is dirty) ──
        let wip_click = cx.listener(move |this, _event: &gpui::ClickEvent, window, cx| {
            this.open_commit_panel(window, cx);
            cx.notify();
        });
        let wip_bg = if commit_panel_open { theme().selected } else { theme().surface };

        // T030: column header row (fixed, above WIP and commit list).
        let col_header = div()
            .id("col-header")
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .px_3()
            .h(px(COL_HEADER_H))
            .flex_shrink_0()
            .bg(rgb(theme().surface))
            // Badge column label
            .child(
                div()
                    .w(px(badge_col_w))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_start()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from("BRANCH / TAG")),
            )
            // Handle between badge and graph columns
            .child(
                div()
                    .id("divider-badge-col")
                    .w(px(INNER_DIV_W))
                    .flex_shrink_0()
                    .h_full()
                    .bg(rgb(theme().surface))
                    // Subtle centre line so the resize boundary is visible
                    // without hovering (user request).
                    .flex()
                    .justify_center()
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().selected)))
                    .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
                    .cursor_col_resize()
                    .on_drag(
                        DividerDrag { kind: DividerKind::BadgeCol },
                        |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
                    ),
            )
            // Graph column label + compact toggle button (W2-GRAPH).
            .child({
                let is_compact = self.graph_compact;
                let compact_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
                    this.graph_compact = !this.graph_compact;
                    cx.notify();
                });
                div()
                    .w(px(graph_col_w))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_1()
                    .on_scroll_wheel(cx.listener(move |this, e: &gpui::ScrollWheelEvent, _w, cx| {
                        this.scroll_graph_by(&e.delta, cx);
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().text_muted))
                            .child(SharedString::from("GRAPH")),
                    )
                    .child(
                        div()
                            .id("compact-toggle")
                            .text_xs()
                            .cursor_pointer()
                            .text_color(rgb(if is_compact { theme().color_branch } else { theme().text_muted }))
                            .hover(|s| s.text_color(rgb(theme().color_branch)))
                            .on_click(compact_click)
                            .child(SharedString::from(if is_compact { "▥" } else { "▤" }))
                    )
            })
            // Handle between graph and message columns
            .child(
                div()
                    .id("divider-graph-col")
                    .w(px(INNER_DIV_W))
                    .flex_shrink_0()
                    .h_full()
                    .bg(rgb(theme().surface))
                    // Subtle centre line so the resize boundary is visible
                    // without hovering (user request).
                    .flex()
                    .justify_center()
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().selected)))
                    .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
                    .cursor_col_resize()
                    .on_drag(
                        DividerDrag { kind: DividerKind::GraphCol },
                        |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
                    ),
            )
            // Message column label
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from("MESSAGE")),
            );


        let commit_list_col = div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            // ── Column header row (T030) ──────────────
            .child(col_header)
            // ── WIP row (only when dirty) ────────────
            .when(is_dirty, |el| {
                el.child(
                    div()
                        .id("wip-row")
                        .flex()
                        .flex_row()
                        .items_center()
                        .w_full()
                        .px_3()
                        .h(px(row_height(self.graph_compact)))
                        .bg(rgb(wip_bg))
                        .on_click(wip_click)
                        .hover(|s| s.bg(rgb(theme().selected)))
                        // Badges column: user-resizable width (T030)
                        .child(
                            div()
                                .w(px(badge_col_w))
                                .flex_shrink_0()
                                .overflow_hidden()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_end()
                                .child(
                                    div()
                                        .px_1()
                                        .rounded_sm()
                                        .bg(rgb(theme().color_warning))
                                        .text_color(rgb(theme().bg_base))
                                        .text_sm()
                                        .flex_shrink_0()
                                        .child(SharedString::from("WIP")),
                                ),
                        )
                        // Inner divider spacer (badge|graph handle width)
                        .child(div().w(px(INNER_DIV_W)).flex_shrink_0().flex().justify_center()
                            .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))))
                        // Graph column placeholder (empty for WIP row)
                        .child(
                            div()
                                .w(px(graph_col_w))
                                .flex_shrink_0(),
                        )
                        // Inner divider spacer (graph|message handle width)
                        .child(div().w(px(INNER_DIV_W)).flex_shrink_0().flex().justify_center()
                            .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))))
                        // Summary area: "// WIP — N changes"
                        .child(
                            div()
                                .flex_1()
                                .text_color(rgb(theme().text_muted))
                                .overflow_hidden()
                                .child(SharedString::from("// WIP")),
                        ),
                )
            })
            // ── Virtualized commit list ──────────────
            .child({
                // W12-GCADOPT (§2.10): keep a handle clone for the Scrollbar
                // overlay; the other is moved into `track_scroll`.
                let scrollbar_handle = commit_scroll_handle.clone();
                with_vertical_scrollbar(
                    "commit-list-scroll",
                    &scrollbar_handle,
                    uniform_list(
                        "commit-list",
                        row_count,
                        cx.processor(move |this, range, _window, cx| {
                            render_rows(&this.rows, &this.avatar_images, range, selected, this.badge_col_w, this.graph_col_w, this.graph_compact, this.graph_scroll_x, cx)
                        }),
                    )
                    // T028: wire scroll handle so jump_to_branch can scroll the list.
                    .track_scroll(commit_scroll_handle)
                    .flex_1()
                    .min_h(px(0.)),
                )
            });

        // Active file (for list highlight) derived from the open main diff.
        let active_src = main_diff.as_ref().map(|d| d.source.clone());
        let active_commit_file: Option<usize> = match &active_src {
            Some(MainDiffSource::Commit { file_index, .. }) => Some(*file_index),
            Some(MainDiffSource::Compare { file_index, .. }) => Some(*file_index),
            _ => None,
        };
        let active_wip: Option<(bool, PathBuf)> = match &active_src {
            Some(MainDiffSource::Unstaged { path }) => Some((false, path.clone())),
            Some(MainDiffSource::Staged { path }) => Some((true, path.clone())),
            _ => None,
        };
        let main_diff_for_center = main_diff;

        // W5-MENU: View → Toggle Sidebar hides the navigator + its divider.
        let sidebar_visible = self.sidebar_visible;
        let mut body_row = div()
            .flex()
            .flex_row()
            .flex_1()
            // min_h(0) — NOT h_full: the body must be able to shrink below its
            // natural content height, otherwise it pushes the bottom panel and
            // status bar out of the window on small window sizes (user report).
            .min_h(px(0.))
            // ── Left sidebar (W5-MENU: hidden when toggled off) ──
            .when(sidebar_visible, |el| {
                el.child(sidebar::render_sidebar(
                    &branches, &remote_branches, &tags, &stashes, &worktrees,
                    &branch_upstream_info, &self.commit_row_index,
                    &sidebar_collapsed, sidebar_filter,
                    sidebar_width, cx,
                ))
                // ── Sidebar divider ───────────────────────
                .child(divider1)
            })
            // ── Center column: W6-TABSPEED loading placeholder, full-width
            //    diff (T-UI-003), or the commit list.  The right panel stays
            //    visible in BOTH non-loading modes so the user can click
            //    through files continuously (user request).
            .child(if let Some(loading_label) = self.loading_tab.clone() {
                render_loading_placeholder(loading_label).into_any_element()
            } else if let Some(diff_view) = main_diff_for_center {
                render_main_diff_view(diff_view, main_diff_scroll_handle, cx).into_any_element()
            } else {
                commit_list_col.into_any_element()
            });

        // ── Right panel: commit panel OR detail panel ───────────
        // Build divider 2 (shared between both panel modes).
        let divider2 = div()
            .id("divider-panel")
            .w(px(4.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(theme().surface))
            .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
            .cursor_col_resize()
            .on_drag(
                DividerDrag { kind: DividerKind::Panel },
                |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
            );

        if commit_panel_open {
            // ── Commit Panel mode (T025) ──────────────
            if let Some(panel_state) = commit_panel.clone() {
                body_row = body_row
                    .child(divider2)
                    .child(render_commit_panel(panel_state, panel_width, commit_input.clone(), active_wip.clone(), cx));
            }
        } else if self.inspector_visible {
            // ── Commit Inspector panel (W2-INSPECTOR; W5-MENU toggle) ──
            body_row = body_row.when_some(detail, |el, d| {
                // ── Commit metadata + changed files ─
                let at = CommitId(d.full_sha.as_ref().to_string());
                let compare_for_panel = compare_view.clone();
                let files = compare_for_panel
                    .as_ref()
                    .map(|view| Some(view.files.clone()))
                    .unwrap_or_else(|| changed_files.clone().unwrap_or(None));
                el.child(divider2)
                    .child(inspector::render_inspector(
                        d, at, selected_badges.clone(),
                        files, compare_for_panel,
                        active_commit_file, inspector_tree_view,
                        self.inspector_split, self.inspector_geom.clone(), panel_width,
                        &avatar_images, cx,
                    ))
            });
        }

        body_row
    }

    /// Bottom panel slot — T-BP-002: open/close + height resize.
    ///
    /// Returns `None` when the panel is closed (so `div().children(…)` adds no
    /// child element).  When open, returns the panel div with:
    /// - a 4px horizontal divider at the top (drag to resize)
    /// - a tab bar (OperationLog / Terminal)
    /// - a placeholder body area
    fn render_bottom_panel_slot(
        &mut self,
        open: bool,
        height: f32,
        active_tab: BottomTab,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if !open {
            return None;
        }

        // ── Horizontal resize divider at top of panel ──
        let h_divider = div()
            .id("divider-bottom-panel")
            .w_full()
            .h(px(BOTTOM_PANEL_DIVIDER_H))
            .flex_shrink_0()
            .bg(rgb(theme().surface))
            .hover(|style| style.bg(rgb(theme().color_branch)).cursor_row_resize())
            .cursor_row_resize()
            .on_drag(
                DividerDrag { kind: DividerKind::BottomPanel },
                |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
            );

        // ── Tab bar ──
        let tab_bar = {
            let tab_operationlog_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                this.bottom_tab = BottomTab::OperationLog;
                cx.notify();
            });
            let tab_terminal_click = cx.listener(|this, _: &gpui::ClickEvent, window, cx| {
                this.bottom_tab = BottomTab::Terminal;
                // T-BP-007: lazy-start the terminal on first show.
                this.ensure_terminal(window, cx);
                cx.notify();
            });

            let make_tab = |label: &'static str, is_active: bool| {
                let text_color = if is_active { theme().text_main } else { theme().text_muted };
                let bg_color = if is_active { theme().selected } else { theme().panel };
                div()
                    .px_3()
                    .h(px(BOTTOM_PANEL_TAB_H))
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .bg(rgb(bg_color))
                    .text_sm()
                    .text_color(rgb(text_color))
                    .hover(|s| s.bg(rgb(theme().surface)))
                    .child(SharedString::from(label))
            };

            div()
                .id("bottom-panel-tab-bar")
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .flex_shrink_0()
                .bg(rgb(theme().panel))
                .child(
                    div()
                        .id("tab-oplog")
                        .flex()
                        .flex_shrink_0()
                        .on_click(tab_operationlog_click)
                        .hover(|s| s.cursor_pointer())
                        .child(make_tab(BottomTab::OperationLog.label(), active_tab == BottomTab::OperationLog)),
                )
                .child(
                    div()
                        .id("tab-terminal")
                        .flex()
                        .flex_shrink_0()
                        .on_click(tab_terminal_click)
                        .hover(|s| s.cursor_pointer())
                        .child(make_tab(BottomTab::Terminal.label(), active_tab == BottomTab::Terminal)),
                )
        };

        // ── Body: Operation Log or Terminal ──
        let body = match active_tab {
            BottomTab::OperationLog => self.render_oplog_body(cx),
            BottomTab::Terminal => self.render_terminal_body(cx),
        };

        // ── Panel container (height = fixed, flex_shrink_0) ──
        let panel_h = height + BOTTOM_PANEL_DIVIDER_H + BOTTOM_PANEL_TAB_H;
        Some(
            div()
                .id("bottom-panel")
                .flex()
                .flex_col()
                .w_full()
                .h(px(panel_h))
                .flex_shrink_0()
                .child(h_divider)
                .child(tab_bar)
                .child(body),
        )
    }

    /// Render the Operation Log tab body (T-BP-004).
    ///
    /// Uses `uniform_list` for virtual scroll.  Each row shows:
    ///   `HH:MM:SS  op  outcome-summary` (outcome coloured green/red/yellow).
    /// Clicking a row toggles single-row expansion (before/after + error/blockers).
    fn render_oplog_body(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let entry_count = self.op_entries.len();

        if entry_count == 0 {
            return div()
                .flex_1()
                .min_h(px(0.))
                .bg(rgb(theme().panel))
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("No operations yet"))
                .into_any();
        }

        let scroll_handle = self.oplog_scroll_handle.clone();
        // W12-GCADOPT (§2.10): Scrollbar overlay on the Operation Log list.
        let scrollbar_handle = scroll_handle.clone();

        let oplog_list = uniform_list(
            "oplog-list",
            entry_count,
            cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                let entries: Vec<OpLogEntry> = this.op_entries.iter().cloned().collect();
                let expanded = this.oplog_expanded;
                range.filter_map(|i| entries.get(i).cloned().map(|e| (i, e)))
                    .map(move |(i, entry)| {
                        let time_label = SharedString::from(format_hms(entry.timestamp));
                        let op_label = SharedString::from(entry.op.clone());

                        let (outcome_label, outcome_color) = match &entry.outcome {
                            OpOutcome::Success { after } => (
                                SharedString::from(format!("Success \u{2192} {}", after.head)),
                                theme().color_success,
                            ),
                            OpOutcome::Failed { error } => (
                                SharedString::from(format!("Failed: {}", error)),
                                theme().color_blocker,
                            ),
                            OpOutcome::Refused { blockers } => (
                                SharedString::from(format!(
                                    "Refused ({} blocker{})",
                                    blockers.len(),
                                    if blockers.len() == 1 { "" } else { "s" }
                                )),
                                theme().color_warning,
                            ),
                        };

                        let is_expanded = expanded == Some(i);

                        let row_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
                            this.oplog_expanded = if this.oplog_expanded == Some(i) {
                                None
                            } else {
                                Some(i)
                            };
                            cx.notify();
                        });

                        let row_bg = if i % 2 == 0 { theme().panel } else { theme().bg_base };

                        // Summary row.
                        let mut row_div = div()
                            .id(("oplog-row", i))
                            .flex()
                            .flex_col()
                            .w_full()
                            .bg(rgb(row_bg))
                            .hover(|s| s.bg(rgb(theme().surface)).cursor_pointer())
                            .on_click(row_click)
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .px_3()
                                    .h(px(22.))
                                    .child(
                                        div()
                                            .w(px(60.))
                                            .flex_shrink_0()
                                            .text_xs()
                                            .text_color(rgb(theme().text_muted))
                                            .child(time_label),
                                    )
                                    .child(
                                        div()
                                            .w(px(100.))
                                            .flex_shrink_0()
                                            .ml(px(6.))
                                            .text_xs()
                                            .text_color(rgb(theme().text_sub))
                                            .child(op_label),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .ml(px(6.))
                                            .text_xs()
                                            .text_color(rgb(outcome_color))
                                            .truncate()
                                            .child(outcome_label),
                                    ),
                            );

                        // Expansion detail rows (before + outcome specifics).
                        if is_expanded {
                            let mut detail_lines: Vec<SharedString> = Vec::new();
                            detail_lines.push(SharedString::from(format!("  before:  {}", entry.before.head)));
                            detail_lines.push(SharedString::from(format!("  dirty:   {}", entry.before.dirty)));
                            match &entry.outcome {
                                OpOutcome::Success { after } => {
                                    detail_lines.push(SharedString::from(format!("  after:   {}", after.head)));
                                    detail_lines.push(SharedString::from(format!("  dirty:   {}", after.dirty)));
                                }
                                OpOutcome::Failed { error } => {
                                    detail_lines.push(SharedString::from(format!("  error:   {}", error)));
                                }
                                OpOutcome::Refused { blockers } => {
                                    for b in blockers {
                                        detail_lines.push(SharedString::from(format!("  blocker: {}", b)));
                                    }
                                }
                            }
                            let detail_div = div()
                                .flex()
                                .flex_col()
                                .w_full()
                                .px_3()
                                .py_1()
                                .bg(rgb(theme().selected))
                                .text_xs()
                                .text_color(rgb(theme().text_sub))
                                .children(detail_lines.into_iter().map(|line| {
                                    div().child(line)
                                }));
                            row_div = row_div.child(detail_div);
                        }

                        row_div
                    })
                    .collect()
            }),
        )
        .track_scroll(scroll_handle)
        .flex_1()
        .min_h(px(0.))
        .bg(rgb(theme().panel));

        with_vertical_scrollbar("oplog-list-scroll", &scrollbar_handle, oplog_list)
            .into_any_element()
    }

    /// Render the Terminal tab body (T-BP-007).
    ///
    /// Three possible states:
    /// 1. Session running → render `TerminalView` entity directly (flex_1 + min_h).
    /// 2. Session failed to start → show the error message.
    /// 3. Not yet started (session is None, or view is None with no error) →
    ///    show a "starting…" placeholder.  The Terminal tab click listener has
    ///    already called `ensure_terminal`; the view will appear on next repaint.
    fn render_terminal_body(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        // W4-TABS: look up the active repo's session in the HashMap.
        let active_session = self
            .repo_path
            .as_ref()
            .and_then(|rp| self.terminal_sessions.get(rp));
        // Case 1: running terminal view.
        if let Some(session) = active_session {
            if let Some(ref view_entity) = session.view {
                // cmd-v paste: gpui-terminal 0.1.0 has no built-in clipboard
                // paste, so an ancestor key listener reads the gpui clipboard
                // and writes straight to the PTY. Key events bubble along the
                // focus path, so this fires while the terminal is focused.
                let paste_writer = session.paste_writer.clone();
                let term_focus = view_entity.read(cx).focus_handle().clone();
                return div()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    // Clicking anywhere in the terminal area refocuses the
                    // terminal (the view's own mouse handling is a no-op in
                    // gpui-terminal 0.1.0, so a stray click could leave the
                    // keyboard focus elsewhere and break typing/cmd-v).
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_this, _e: &gpui::MouseDownEvent, window, _cx| {
                            window.focus(&term_focus);
                        }),
                    )
                    .on_key_down(cx.listener(move |_this, event: &KeyDownEvent, _window, cx| {
                        let ks = &event.keystroke;
                        if ks.modifiers.platform && ks.key == "v" {
                            if let Some(writer) = paste_writer.as_ref() {
                                if let Some(text) =
                                    cx.read_from_clipboard().and_then(|item| item.text())
                                {
                                    writer.paste_text(&text);
                                    eprintln!("[kagi] terminal: paste {} chars", text.chars().count());
                                }
                            }
                            cx.stop_propagation();
                        }
                    }))
                    .child(view_entity.clone())
                    .into_any();
            }

            // Case 2: start failed — show error.
            if let Some(ref err) = session.start_error {
                let msg = SharedString::from(format!("terminal error: {}", err));
                return div()
                    .flex_1()
                    .min_h(px(0.))
                    .bg(rgb(theme().panel))
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(rgb(theme().color_blocker))
                    .child(msg)
                    .into_any();
            }
        }

        // Case 3: placeholder (no session yet / shell exited, will restart).
        div()
            .flex_1()
            .min_h(px(0.))
            .bg(rgb(theme().panel))
            .px_3()
            .py_2()
            .text_sm()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from("(terminal exited — re-opening will restart)"))
            .into_any()
    }

    /// Status bar slot — the 22 px footer (T-BP-003 full implementation).
    ///
    /// Left → Right layout:
    ///   branch [● dirty] [↑A ↓B | no upstream] [staged N] [unstaged M]
    ///   HH:MM:SS  ·  <last operation message (flex_1, overflow_hidden)>
    ///   right end: >_ (Terminal icon) ≡ (Operation Log icon) — VSCode style
    ///
    /// The old ▲/▼ toggle is replaced by the icon buttons.
    fn render_status_bar(
        &mut self,
        status_footer: FooterStatus,
        bottom_panel_open: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let summary = self.status_summary.clone();
        let bottom_tab = self.bottom_tab;

        // ── Footer message colour ──────────────────────────────
        let (footer_color, footer_text) = match &status_footer {
            FooterStatus::Success(msg) => (theme().color_success, msg.clone()),
            FooterStatus::Failed(msg) => (theme().color_blocker, msg.clone()),
            FooterStatus::Idle(msg) => (theme().text_muted, msg.clone()),
            FooterStatus::Busy(msg) => (
                theme().color_branch,
                SharedString::from(format!("\u{27f3} {}", msg)), // ⟳ msg
            ),
        };

        // ── Branch label ───────────────────────────────────────
        let branch_text = SharedString::from(summary.branch.clone());

        // ── Dirty bullet ──────────────────────────────────────
        let dirty_chip = if summary.is_dirty {
            Some(
                div()
                    .ml(px(4.))
                    .text_color(rgb(theme().color_warning))
                    .flex_shrink_0()
                    .child(SharedString::from("\u{25cf}")), // ●
            )
        } else {
            None
        };

        // ── Staged / unstaged counts ───────────────────────────
        let staged_chip = if summary.staged > 0 {
            Some(
                div()
                    .ml(px(4.))
                    .text_color(rgb(theme().color_success))
                    .flex_shrink_0()
                    .child(SharedString::from(format!("+{}", summary.staged))),
            )
        } else {
            None
        };
        let unstaged_chip = if summary.unstaged > 0 {
            Some(
                div()
                    .ml(px(4.))
                    .text_color(rgb(theme().color_warning))
                    .flex_shrink_0()
                    .child(SharedString::from(format!("~{}", summary.unstaged))),
            )
        } else {
            None
        };

        // ── Conflict count (W2-STATUS) ─────────────────────────
        let conflict_chip = if summary.conflict_count > 0 {
            Some(
                div()
                    .ml(px(4.))
                    .text_color(rgb(theme().color_blocker))
                    .flex_shrink_0()
                    .child(SharedString::from(format!("!{}", summary.conflict_count))),
            )
        } else {
            None
        };

        // ── Stash count (W2-STATUS) ────────────────────────────
        let stash_chip = if summary.stash_count > 0 {
            Some(
                div()
                    .ml(px(4.))
                    .text_color(rgb(theme().text_sub))
                    .flex_shrink_0()
                    .child(SharedString::from(format!("\u{2691}{}", summary.stash_count))), // ⚑N
            )
        } else {
            None
        };

        // ── Upstream name (W2-STATUS) ──────────────────────────
        let upstream_name_chip = if !summary.upstream_name.is_empty() {
            Some(
                div()
                    .ml(px(6.))
                    .text_color(rgb(theme().text_muted))
                    .flex_shrink_0()
                    .child(SharedString::from(format!("\u{2192} {}", summary.upstream_name))), // → origin/main
            )
        } else {
            None
        };

        // ── Ahead / behind / no upstream ──────────────────────
        let upstream_chip = match (summary.ahead, summary.behind) {
            (Some(a), Some(b)) => {
                let label = format!("\u{2191}{} \u{2193}{}", a, b); // ↑A ↓B
                Some(
                    div()
                        .ml(px(6.))
                        .text_color(rgb(theme().text_sub))
                        .flex_shrink_0()
                        .child(SharedString::from(label)),
                )
            }
            _ if summary.no_upstream => Some(
                div()
                    .ml(px(6.))
                    .text_color(rgb(theme().text_muted))
                    .flex_shrink_0()
                    .child(SharedString::from("no upstream")),
            ),
            _ => None, // detached HEAD or unborn: nothing shown
        };

        // ── Last refresh time ──────────────────────────────────
        let refresh_label = if summary.last_refresh_secs > 0 {
            Some(
                div()
                    .ml(px(6.))
                    .text_color(rgb(theme().text_muted))
                    .flex_shrink_0()
                    .child(SharedString::from(format_hms(summary.last_refresh_secs))),
            )
        } else {
            None
        };

        // ── VSCode-style icon buttons (Terminal + Operation Log) ──────────
        // Clicking an inactive icon opens the panel on that tab.
        // Clicking the active icon closes the panel (toggle).
        let oplog_active = bottom_panel_open && bottom_tab == BottomTab::OperationLog;
        let terminal_active = bottom_panel_open && bottom_tab == BottomTab::Terminal;

        let icon_terminal_click = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
            if terminal_active {
                // Same tab visible → close panel.
                this.bottom_panel_open = false;
            } else {
                this.bottom_panel_open = true;
                this.bottom_tab = BottomTab::Terminal;
                // T-BP-007: lazy-start terminal when opening via status bar icon.
                this.ensure_terminal(window, cx);
            }
            cx.notify();
        });

        let icon_oplog_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if oplog_active {
                // Same tab visible → close panel.
                this.bottom_panel_open = false;
            } else {
                this.bottom_panel_open = true;
                this.bottom_tab = BottomTab::OperationLog;
            }
            cx.notify();
        });

        let icon_terminal_color = if terminal_active { theme().text_main } else { theme().text_muted };
        let icon_oplog_color = if oplog_active { theme().text_main } else { theme().text_muted };

        let icon_terminal = div()
            .id("status-icon-terminal")
            .ml(px(4.))
            .px_1()
            .flex_shrink_0()
            .text_color(rgb(icon_terminal_color))
            .hover(|s| s.text_color(rgb(theme().text_main)).cursor_pointer())
            .on_click(icon_terminal_click)
            .child(
                gpui_component::Icon::new(gpui_component::IconName::SquareTerminal)
                    .with_size(gpui_component::Size::XSmall)
                    .text_color(rgb(icon_terminal_color)),
            );

        let icon_oplog = div()
            .id("status-icon-oplog")
            .ml(px(2.))
            .px_1()
            .flex_shrink_0()
            .text_color(rgb(icon_oplog_color))
            .hover(|s| s.text_color(rgb(theme().text_main)).cursor_pointer())
            .on_click(icon_oplog_click)
            .child(
                gpui_component::Icon::new(gpui_component::IconName::Menu)
                    .with_size(gpui_component::Size::XSmall)
                    .text_color(rgb(icon_oplog_color)),
            );

        // ── Assemble status bar ────────────────────────────────
        let mut bar = div()
            .id("status-footer")
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(px(22.))
            .flex_shrink_0()
            .px_2()
            .bg(rgb(theme().panel))
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .overflow_hidden()
            // Branch label
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(rgb(theme().text_main))
                    .child(branch_text),
            );

        // Dirty bullet
        if let Some(chip) = dirty_chip {
            bar = bar.child(chip);
        }
        // Staged/unstaged counts
        if let Some(chip) = staged_chip {
            bar = bar.child(chip);
        }
        if let Some(chip) = unstaged_chip {
            bar = bar.child(chip);
        }
        // Conflict / stash counts (W2-STATUS)
        if let Some(chip) = conflict_chip {
            bar = bar.child(chip);
        }
        if let Some(chip) = stash_chip {
            bar = bar.child(chip);
        }
        // Upstream ahead/behind + tracking-ref name
        if let Some(chip) = upstream_chip {
            bar = bar.child(chip);
        }
        if let Some(chip) = upstream_name_chip {
            bar = bar.child(chip);
        }
        // Refresh time
        if let Some(chip) = refresh_label {
            bar = bar.child(chip);
        }

        // Last operation message: flex_1, overflow_hidden, only if space allows.
        bar = bar.child(
            div()
                .flex_1()
                .ml(px(6.))
                .overflow_hidden()
                .text_color(rgb(footer_color))
                .child(footer_text),
        );

        // Icon buttons at the right end.
        bar.child(icon_terminal)
           .child(icon_oplog)
    }
}

// ──────────────────────────────────────────────────────────────
// Row renderer
// ──────────────────────────────────────────────────────────────

/// Render commit rows for the given range.  Called by `uniform_list`
/// with only the visible subset, so this must be cheap.
///
/// `selected` — the currently selected row index (None = no selection).
/// `graph_compact` — when true use compact row height (18px) instead of 24px.
/// `cx` — the `Context<KagiApp>` from the `cx.processor` closure;
///         used to build `cx.listener(...)` for the on_click handler.
fn render_rows(
    rows: &[CommitRow],
    avatar_images: &HashMap<String, std::sync::Arc<gpui::Image>>,
    range: std::ops::Range<usize>,
    selected: Option<usize>,
    badge_col_w: f32,
    graph_col_w: f32,
    graph_compact: bool,
    graph_scroll_x: f32,
    cx: &mut Context<KagiApp>,
) -> Vec<impl IntoElement> {
    let rh = row_height(graph_compact);

    range
        .filter_map(|i| rows.get(i).map(|row| (i, row)))
        .map(|(ix, row)| {
            let row = row.clone();
            let is_selected = selected == Some(ix);

            // Selected row gets a prominent surface highlight;
            // even/odd stripes apply otherwise.
            let row_bg = if is_selected {
                theme().selected
            } else if ix % 2 == 0 {
                theme().bg_base
            } else {
                theme().bg_row_alt
            };

            // ── Graph lane area (T030) ────────────────────────
            // visible_lanes = how many lanes fit in the current graph column width.
            // This replaces the old MAX_LANES-based clipping.
            let visible_lanes = graph_view::lanes_for_width(graph_col_w);

            // on_click handler: update KagiApp.selected via cx.listener.
            let click_handler = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.commit_menu = None;
                this.select(ix);
                cx.notify();
            });
            let context_click_handler = cx.listener(
                move |this, event: &gpui::MouseDownEvent, _window, cx| {
                    this.open_commit_menu(ix, event.position);
                    cx.stop_propagation();
                    cx.notify();
                },
            );

            // ── Avatar (T020 / W11-AVATAR) ────────────────────
            let avatar_color = avatar::avatar_color(&row.author_email);
            let avatar_init = SharedString::from(avatar::avatar_initial(&row.author));
            // Convert Hsla to the rgb u32 that gpui's `bg()` accepts via hsla().
            let av_bg = avatar_color;
            // W11-AVATAR: real GitHub avatar if resolved, else initial circle.
            let avatar_image = avatar_images.get(&row.author_email).cloned();

            // W2-GRAPH: badge presence flag for label→node connector line.
            let has_badges = !row.badges.is_empty();

            div()
                .id(ix)
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                // W2-GRAPH item 3: 2px accent bar on the left edge of selected rows.
                // We use pl_3() normally and reduce the inner padding by 2px when
                // selected to make room for the bar without changing total row width.
                .when(is_selected, |el| {
                    el.pl(px(10.))  // 12 - 2 = 10px to account for 2px bar
                        .border_l_2()
                        .border_color(rgb(theme().color_branch))
                })
                .when(!is_selected, |el| el.px_3())
                .h(px(rh))
                .bg(rgb(row_bg))
                .on_click(click_handler)
                .on_mouse_down(MouseButton::Right, context_click_handler)
                // ── Badges column: user-resizable width (T030) ──
                .child(render_badges_column(&row.badges, badge_col_w))
                // ── Inner divider spacer (badge|graph handle width) ──
                .child(div().w(px(INNER_DIV_W)).flex_shrink_0().flex().justify_center()
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))))
                // ── Graph lane area (T030) ────────────────────────
                // Always render the graph column at graph_col_w width.
                // Clip by visible_lanes to prevent bleed into message column.
                .child(
                    div()
                        .w(px(graph_col_w))
                        .h_full()
                        .flex_shrink_0()
                        .overflow_hidden()
                        // Horizontal wheel/trackpad scroll reveals clipped
                        // lanes. Vertical deltas are left untouched so the
                        // commit list keeps scrolling normally.
                        .on_scroll_wheel(cx.listener(move |this, e: &gpui::ScrollWheelEvent, _w, cx| {
                            this.scroll_graph_by(&e.delta, cx);
                        }))
                        .when(visible_lanes > 0, |el| {
                            el.child(
                                graph_canvas(
                                    row.lane,
                                    row.edges.clone(),
                                    visible_lanes,
                                    row.is_head,
                                    row.is_merge,
                                    has_badges,
                                    graph_scroll_x,
                                )
                                .size_full(),
                            )
                        }),
                )
                // ── Inner divider spacer (graph|message handle width) ──
                .child(div().w(px(INNER_DIV_W)).flex_shrink_0().flex().justify_center()
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))))
                // ── Author avatar: 18px circle after graph ────────
                // W11-AVATAR: when a GitHub avatar is resolved, show the image
                // clipped to the circle; otherwise the initial-on-colour circle.
                .child({
                    let circle = div()
                        .w(px(18.))
                        .h(px(18.))
                        .flex_shrink_0()
                        .mr(px(4.))
                        .rounded_full()
                        .overflow_hidden();
                    match avatar_image {
                        Some(image) => circle.child(
                            gpui::img(gpui::ImageSource::Image(image))
                                .size_full()
                                .rounded_full(),
                        ),
                        None => circle
                            .bg(av_bg)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_color(gpui::white())
                                    .text_xs()
                                    .child(avatar_init),
                            ),
                    }
                })
                .child(
                    div()
                        .flex_1()
                        .text_color(rgb(theme().text_main))
                        // Single line, no wrapping: long summaries ellipsize
                        // (truncate = overflow_hidden + nowrap + ellipsis).
                        .truncate()
                        .child(row.summary.clone()),
                )
                .child(
                    div()
                        .w(px(130.))
                        .flex_shrink_0()
                        .text_color(rgb(theme().text_sub))
                        .truncate()
                        .child(row.author.clone()),
                )
                .child(
                    div()
                        .w(px(72.))
                        .flex_shrink_0()
                        .text_color(rgb(theme().text_muted))
                        .child(row.date.clone()),
                )
        })
        .collect()
}

// Note: render_detail_panel was extracted to src/ui/inspector.rs (W2-INSPECTOR).

// ──────────────────────────────────────────────────────────────
// T-UI-003: Main pane diff renderer (full-width)
// ──────────────────────────────────────────────────────────────

/// Render the full-width main pane diff view.
///
/// Layout (fills remaining width after sidebar + divider):
/// - Header row: `← Back` + file name + stats
/// - Body: `uniform_list` id `"main-diff-list"` with line numbers
/// W6-TABSPEED / ADR-0030: center-pane placeholder shown while an uncached tab
/// is loading on a background thread.  The tab strip stays operable above it.
fn render_loading_placeholder(label: SharedString) -> impl IntoElement {
    div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .bg(rgb(theme().bg_base))
        .child(
            div()
                .text_lg()
                .text_color(rgb(theme().text_sub))
                .child(label),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("\u{27f3}")), // ⟳
        )
}

fn render_main_diff_view(
    view: MainDiffView,
    scroll_handle: UniformListScrollHandle,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let row_count = view.rows.len();
    let rows = std::sync::Arc::new(view.rows);
    let rows_for_list = rows.clone();
    let title = view.title.clone();
    let stats = view.stats.clone();

    // "← Back" click handler: close the main diff view.
    let back_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.close_main_diff();
        cx.notify();
    });

    div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().panel))
        // ── Header row (fixed height) ─────────────────────────────────────
        .child(
            div()
                .id("main-diff-header")
                .flex()
                .flex_row()
                .items_center()
                .flex_shrink_0()
                .px_3()
                .py_1()
                .gap_2()
                .bg(rgb(theme().surface))
                // ← Back button
                .child(
                    div()
                        .id("main-diff-back")
                        .px_2()
                        .py_px()
                        .rounded_sm()
                        .bg(rgb(theme().bg_base))
                        .text_sm()
                        .text_color(rgb(theme().text_sub))
                        .on_click(back_click)
                        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                        .child(SharedString::from("\u{2190} Back")),
                )
                // File name
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .text_color(rgb(theme().text_main))
                        .truncate()
                        .child(title),
                )
                // Stats: +N −M
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_sub))
                        .flex_shrink_0()
                        .child(stats),
                ),
        )
        // ── Diff body: full remaining space ──────────────────────────────
        .child({
            // W12-GCADOPT (§2.10): Scrollbar overlay on the diff list.
            let scrollbar_handle = scroll_handle.clone();
            with_vertical_scrollbar(
                "main-diff-list-scroll",
                &scrollbar_handle,
                uniform_list(
                    "main-diff-list",
                    row_count,
                    cx.processor(move |_this, range, _window, _cx| {
                        render_main_diff_rows(&rows_for_list, range)
                    }),
                )
                .track_scroll(scroll_handle)
                .flex_1()
                .min_h(px(0.)),
            )
        })
}

/// Render a range of diff rows for the `"main-diff-list"` uniform_list.
/// Includes line numbers: old/new each 5 chars wide, theme().text_muted colour.
fn render_main_diff_rows(
    rows: &[DiffRow],
    range: std::ops::Range<usize>,
) -> Vec<impl IntoElement> {
    range
        .filter_map(|i| rows.get(i).map(|row| (i, row)))
        .map(|(i, row)| match row {
            DiffRow::HunkHeader(header) => {
                div()
                    .id(("main-diff-hunk", i))
                    .w_full()
                    .px_2()
                    .py_px()
                    .bg(rgb(theme().surface))
                    .text_sm()
                    .text_color(rgb(theme().diff_hunk))
                    .overflow_hidden()
                    .child(header.clone())
                    .into_any()
            }
            DiffRow::Line { kind, text, old_lineno, new_lineno, highlights } => {
                let bg = match kind {
                    DiffLineKind::Added   => theme().diff_added_bg,
                    DiffLineKind::Removed => theme().diff_removed_bg,
                    DiffLineKind::Context => theme().bg_base,
                };
                let text_color = match kind {
                    DiffLineKind::Added   => 0xa6e3a1u32, // green
                    DiffLineKind::Removed => 0xf38ba8u32, // red
                    DiffLineKind::Context => theme().text_main,
                };
                // Format line numbers: 5 chars fixed width, muted colour.
                let old_str = match old_lineno {
                    Some(n) => format!("{:5}", n),
                    None    => "     ".to_string(),
                };
                let new_str = match new_lineno {
                    Some(n) => format!("{:5}", n),
                    None    => "     ".to_string(),
                };

                // T-UI-004: build highlighted content element.
                // If we have pre-computed highlight spans, use StyledText; otherwise
                // fall back to a plain text element (keeps the existing colour).
                let content_el: gpui::AnyElement = if highlights.is_empty() {
                    div()
                        .flex_1()
                        .text_color(rgb(text_color))
                        .overflow_hidden()
                        .child(text.clone())
                        .into_any()
                } else {
                    // Validate that all highlight byte ranges lie within the text.
                    // Silently drop spans that fall outside to prevent panics.
                    let text_str: &str = text.as_ref();
                    let text_len = text_str.len();
                    let valid_highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> =
                        highlights
                            .iter()
                            .filter(|(r, _)| {
                                r.start <= r.end
                                    && r.end <= text_len
                                    && text_str.is_char_boundary(r.start)
                                    && text_str.is_char_boundary(r.end)
                            })
                            .cloned()
                            .collect();
                    div()
                        .flex_1()
                        .text_color(rgb(text_color))
                        .overflow_hidden()
                        .child(
                            gpui::StyledText::new(text.clone())
                                .with_highlights(valid_highlights),
                        )
                        .into_any()
                };

                div()
                    .id(("main-diff-line", i))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .py_px()
                    .bg(rgb(bg))
                    .text_sm()
                    .overflow_hidden()
                    // Old line number
                    .child(
                        div()
                            .flex_shrink_0()
                            .w(px(44.))
                            .text_color(rgb(theme().text_muted))
                            .child(SharedString::from(old_str)),
                    )
                    // New line number
                    .child(
                        div()
                            .flex_shrink_0()
                            .w(px(44.))
                            .text_color(rgb(theme().text_muted))
                            .child(SharedString::from(new_str)),
                    )
                    // Content (sigil + highlighted text)
                    .child(content_el)
                    .into_any()
            }
            DiffRow::Binary => {
                div()
                    .id(("main-diff-binary", i))
                    .w_full()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from("Binary file (no diff)"))
                    .into_any()
            }
        })
        .collect()
}

/// Render the badge chips for one commit row as a horizontal flex container.
///
/// Badge labels are capped at 24 visible chars with a trailing `…` to prevent
/// very long branch names from overflowing the commit list row (T019).
/// Sort key for badge priority: HeadBranch=0, Branch=1, Tag=2, Remote=3.
/// Right-aligned layout means the last-rendered badge is closest to the graph,
/// so we want the most important badge last → highest priority rendered last.
/// We render in priority order (0→3) so HeadBranch ends up leftmost and
/// Remote rightmost within the 150px column (closest to the graph).
fn badge_priority(kind: &BadgeKind) -> u8 {
    match kind {
        BadgeKind::HeadBranch => 0,
        BadgeKind::Branch => 1,
        BadgeKind::Tag => 2,
        BadgeKind::Remote => 3,
    }
}

/// Render the badges column: user-resizable width (T030), **left-aligned**
/// (user request), `overflow_hidden`.  An empty badges list still occupies
/// the full width so that all rows share the same graph start position
/// (GitKraken layout, T021).  `badge_col_w` is the current column width.
fn render_badges_column(badges: &[commit_list::RefBadge], badge_col_w: f32) -> impl IntoElement {
    // Content is built to fit rather than relying on clipping:
    //   - left-aligned, so the highest-priority chip (leftmost) is always
    //     fully visible and overflow happens rightward — the direction
    //     gpui's overflow_hidden actually clips,
    //   - the "+N" chip sits right after the primary chip so it can't be
    //     clipped,
    //   - the secondary chip flex-shrinks with an ellipsis; only its already
    //     ellipsized tail can ever be cut off.
    const MAX_BADGES: usize = 2;
    const MAX_BADGE_CHARS: usize = 20;

    let mut by_prio: Vec<&commit_list::RefBadge> = badges.iter().collect();
    by_prio.sort_by_key(|b| badge_priority(&b.kind));
    let extra = by_prio.len().saturating_sub(MAX_BADGES);
    let shown = &by_prio[..by_prio.len().min(MAX_BADGES)];

    let mut inner = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_start()
        .gap_1()
        .overflow_hidden();

    // Badges in priority order: primary (HEAD/branch) leftmost.
    for (i, badge) in shown.iter().enumerate() {
        let color = match badge.kind {
            BadgeKind::HeadBranch => theme().color_head,
            BadgeKind::Branch => theme().color_branch,
            BadgeKind::Remote => theme().color_remote,
            BadgeKind::Tag => theme().color_tag,
        };
        // Char-truncate long labels.
        let label: SharedString = if badge.label.chars().count() > MAX_BADGE_CHARS {
            let s: String = badge.label.chars().take(MAX_BADGE_CHARS - 1).collect();
            SharedString::from(format!("{}\u{2026}", s))
        } else {
            badge.label.clone()
        };
        let is_primary = i == 0;
        let (badge_bg, badge_border, badge_text) = theme::badge_style(color);
        let chip = div()
            .px_1()
            .rounded_sm()
            .bg(gpui::rgba(badge_bg))
            .border_1()
            .border_color(gpui::rgba(badge_border))
            .text_color(rgb(badge_text))
            .text_sm()
            .when(is_primary, |c| c.flex_shrink_0())
            // Secondary chips may shrink to fit; their text ellipsizes.
            .when(!is_primary, |c| c.min_w(px(20.)).truncate())
            .child(label);
        inner = inner.child(chip);

        // "+N" chip directly after the primary chip (never clipped).
        if is_primary && extra > 0 {
            inner = inner.child(
                div()
                    .px_1()
                    .rounded_sm()
                    .bg(rgb(theme().surface))
                    .text_color(rgb(theme().text_sub))
                    .text_sm()
                    .flex_shrink_0()
                    .child(SharedString::from(format!("+{extra}"))),
            );
        }
    }

    // User-resizable container (T030), overflow clipped so long badge lists don't push graph.
    div()
        .w(px(badge_col_w))
        .flex_shrink_0()
        .overflow_hidden()
        .flex()
        .flex_row()
        .items_center()
        .justify_start()
        .child(inner)
}

// ──────────────────────────────────────────────────────────────
// W3-NOTIFY: blocking cores for pull / push
//
// Everything that may take seconds (repo open → preflight → execute →
// verify snapshot) lives here, free of `&mut KagiApp`, so the UI path can
// run it via `cx.background_spawn` while the headless path calls it inline.
// ──────────────────────────────────────────────────────────────

/// Blocking part of pull. Returns (human summary, after-state) or an error
/// message suitable for the oplog / modal.
fn pull_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
) -> Result<(String, StateSummary), String> {
    let repo = git2::Repository::open(repo_path)
        .map_err(|e| format!("Repo open error: {}", e.message()))?;
    preflight_check(&repo, plan).map_err(|e| format!("Preflight failed: {}", e))?;

    let outcome = execute_pull(&repo, repo_path).map_err(|e| format!("Pull failed: {}", e))?;
    let summary = match &outcome {
        PullOutcome::UpToDate => "already up to date".to_string(),
        PullOutcome::FastForward { to } => format!("fast-forward to {}", to.short()),
        PullOutcome::Merged { commit } => format!("merge commit {}", commit.short()),
    };
    eprintln!("[kagi] executed: pull — {}", summary);

    // Verify: re-snapshot for the after-state.
    let after_summary = verify_after_snapshot(repo_path, plan);
    eprintln!("[kagi] verified: pull after = {}", after_summary.head);
    Ok((summary, after_summary))
}

/// Blocking part of push. Returns (human summary, after-state) or an error
/// message suitable for the oplog / modal.
fn push_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
) -> Result<(String, StateSummary), String> {
    let repo = git2::Repository::open(repo_path)
        .map_err(|e| format!("Repo open error: {}", e.message()))?;
    preflight_check(&repo, plan).map_err(|e| format!("Preflight failed: {}", e))?;

    let outcome = execute_push(&repo, repo_path).map_err(|e| format!("Push failed: {}", e))?;
    let summary = if outcome.set_upstream {
        format!("pushed {} commit(s), set upstream", outcome.pushed)
    } else {
        format!("pushed {} commit(s)", outcome.pushed)
    };
    eprintln!("[kagi] executed: push — {}", summary);

    let after_summary = verify_after_snapshot(repo_path, plan);
    eprintln!("[kagi] verified: push after = {}", after_summary.head);
    Ok((summary, after_summary))
}

/// Re-snapshot the repo for the verified after-state; falls back to the
/// plan's prediction when the snapshot fails (non-fatal).
fn verify_after_snapshot(repo_path: &std::path::Path, plan: &OperationPlan) -> StateSummary {
    match git2::Repository::open(repo_path) {
        Ok(mut repo2) => match kagi::git::snapshot(&mut repo2, 10_000) {
            Ok(snap) => StateSummary {
                head: snap.head.display(),
                dirty: if snap.status.is_dirty() {
                    "dirty".to_string()
                } else {
                    "clean".to_string()
                },
            },
            Err(_) => plan.predicted.clone(),
        },
        Err(_) => plan.predicted.clone(),
    }
}

// ──────────────────────────────────────────────────────────────
// Status footer renderer (T017)
// ──────────────────────────────────────────────────────────────

/// Render the 22px status footer bar at the bottom of the window.
///
/// - [`FooterStatus::Success`] — green text on dark background.
/// - [`FooterStatus::Failed`] — red text on dark background.
/// - [`FooterStatus::Idle`] — muted text (default: "Ready").
#[allow(dead_code)]
fn render_status_footer(status: FooterStatus) -> impl IntoElement {
    let (text_color, text) = match &status {
        FooterStatus::Success(msg) => (theme().color_success, msg.clone()),
        FooterStatus::Failed(msg) => (theme().color_blocker, msg.clone()),
        FooterStatus::Idle(msg) => (theme().text_muted, msg.clone()),
        FooterStatus::Busy(msg) => (
            theme().color_branch,
            SharedString::from(format!("\u{27f3} {}", msg)), // ⟳ msg
        ),
    };

    div()
        .id("status-footer")
        .flex()
        .flex_row()
        .items_center()
        .w_full()
        .h(px(22.))
        .flex_shrink_0()
        .px_3()
        .bg(rgb(theme().panel))
        .text_xs()
        .text_color(rgb(text_color))
        .overflow_hidden()
        .child(text)
}

// ──────────────────────────────────────────────────────────────
// Plan modal renderer (T013)
// ──────────────────────────────────────────────────────────────

/// Render the plan confirmation overlay.
///
/// Layout (absolute, full-screen):
/// - Semi-transparent dark backdrop
/// - Centred modal card:
///   - Title
///   - Current → Predicted state
///   - Warnings (yellow) if any
///   - Blockers (red) if any
///   - Recovery text
///   - Error message (if preflight/execute failed)
///   - `[Cancel]` always present; `[Checkout]` only when no blockers
fn render_plan_modal(modal: CheckoutPlanModal, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let create_branch_target = match &modal.target {
        CheckoutPlanTarget::Commit(commit_id) => Some(commit_id.clone()),
        CheckoutPlanTarget::Branch(_) => None,
    };
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.confirm_checkout();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        "Checkout",
        cancel_handler,
        confirm_handler,
        create_branch_target,
        cx,
    )
        .into_any_element()
}

/// Pull plan confirmation overlay (T-HT-003) — same card as the checkout
/// plan modal, wired to `confirm_pull`.
fn render_pull_modal(modal: PullPlanModal, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_pull_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        // W3-NOTIFY: run on a background thread (start/finish toasts).
        this.start_pull(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(modal.plan, modal.error, "Pull", cancel_handler, confirm_handler, None, cx)
        .into_any_element()
}

/// Undo-commit confirmation overlay (T-HT-009).
fn render_undo_modal(modal: UndoPlanModal, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_undo_modal();
        if let Some(fh) = this.root_focus.clone() { window.focus(&fh); }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.confirm_undo();
        if let Some(fh) = this.root_focus.clone() { window.focus(&fh); }
        cx.notify();
    });
    render_plan_modal_card(modal.plan, modal.error, "Undo", cancel_handler, confirm_handler, None, cx)
        .into_any_element()
}

/// Stash-pop confirmation overlay (T-HT-007).
fn render_pop_modal(modal: PopPlanModal, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_pop_modal();
        if let Some(fh) = this.root_focus.clone() { window.focus(&fh); }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.confirm_pop();
        if let Some(fh) = this.root_focus.clone() { window.focus(&fh); }
        cx.notify();
    });
    render_plan_modal_card(modal.plan, modal.error, "Pop", cancel_handler, confirm_handler, None, cx)
        .into_any_element()
}

/// Push plan confirmation overlay (T-HT-004) — same card as the pull
/// plan modal, wired to `confirm_push`.
fn render_push_modal(modal: PushPlanModal, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_push_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        // W3-NOTIFY: run on a background thread (start/finish toasts).
        this.start_push(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(modal.plan, modal.error, "Push", cancel_handler, confirm_handler, None, cx)
        .into_any_element()
}

/// Delete-branch confirmation overlay (W2-DELETE).
fn render_delete_branch_modal(
    modal: DeleteBranchModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_delete_branch_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.confirm_delete_branch();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        "Delete",
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
}

/// Revert confirmation overlay (T-CM-034).
fn render_revert_modal(
    modal: RevertModal,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let cancel_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.cancel_revert_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.confirm_revert();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(
        modal.plan,
        modal.error,
        "Revert",
        cancel_handler,
        confirm_handler,
        None,
        cx,
    )
    .into_any_element()
}

/// Shared plan-confirmation card: title / current→predicted / warnings /
/// blockers / recovery / error / Cancel + confirm buttons.  The confirm
/// button is hidden whenever the plan has blockers.
fn render_plan_modal_card(
    plan: std::sync::Arc<OperationPlan>,
    error: Option<SharedString>,
    confirm_label: &'static str,
    cancel_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    confirm_handler: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
    create_branch_target: Option<CommitId>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let has_blockers = !plan.blockers.is_empty();

    // ── Build modal card ────────────────────────────────────
    let mut card = div()
        .w(px(480.))
        .bg(rgb(theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        // ── Title ─────────────────────────────────────────
        .child(
            div()
                .text_color(rgb(theme().text_main))
                .text_xl()
                .child(SharedString::from(plan.title.clone())),
        )
        // ── Current → Predicted ───────────────────────────
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("Current")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(plan.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(theme().text_sub))
                                .child(SharedString::from(format!("[{}]", plan.current.dirty))),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("\u{2192} Predicted")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(plan.predicted.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(theme().text_sub))
                                .child(SharedString::from(format!("[{}]", plan.predicted.dirty))),
                        ),
                ),
        );

    // ── Warnings ─────────────────────────────────────────
    if !plan.warnings.is_empty() {
        let mut warn_col = div().flex().flex_col().gap_1();
        for w in &plan.warnings {
            warn_col = warn_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().color_warning))
                    .overflow_hidden()
                    .child(SharedString::from(format!("\u{26a0} {}", w))),
            );
        }
        card = card.child(warn_col);
    }

    // ── Commits to push (T-HT-004) ────────────────────────
    // Shown only when preview_commits is non-empty (push plans).
    if !plan.preview_commits.is_empty() {
        let total = plan.preview_commits.len();
        let show_count = total.min(10);
        let label = format!("Commits to push ({})", total);
        let mut commit_col = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_label))
                    .child(SharedString::from(label)),
            );
        for entry in plan.preview_commits.iter().take(show_count) {
            let line: String = entry.chars().take(72).collect();
            commit_col = commit_col.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_sub))
                    .overflow_hidden()
                    .child(SharedString::from(line)),
            );
        }
        if total > 10 {
            commit_col = commit_col.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(format!("\u{2026} and {} more", total - 10))),
            );
        }
        card = card.child(commit_col);
    }

    // ── Blockers ──────────────────────────────────────────
    if !plan.blockers.is_empty() {
        let mut block_col = div().flex().flex_col().gap_1();
        for b in &plan.blockers {
            block_col = block_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().color_blocker))
                    .overflow_hidden()
                    .child(SharedString::from(format!("\u{2717} {}", b))),
            );
        }
        card = card.child(block_col);
    }

    // ── Recovery ──────────────────────────────────────────
    card = card.child(
        div()
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .overflow_hidden()
            .child(SharedString::from(plan.recovery.clone())),
    );

    // ── Error message (preflight / execute failure) ───────
    if let Some(err) = &error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    // ── Buttons ───────────────────────────────────────────
    let mut button_row = div()
        .flex()
        .flex_row()
        .gap_2()
        .justify_end()
        // Cancel button (always present — safe default)
        .child(
            div()
                .id("plan-cancel")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().surface))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .on_click(cancel_handler)
                .hover(|style| style.bg(rgb(theme().selected)))
                .child(SharedString::from("Cancel")),
        );

    if let Some(commit_id) = create_branch_target {
        let create_handler = cx.listener(move |this, _event: &gpui::ClickEvent, window, cx| {
            this.cancel_modal();
            this.open_create_branch_modal(commit_id.clone(), cx);
            if let Some(fh) = this.root_focus.clone() {
                window.focus(&fh);
            }
            cx.notify();
        });
        button_row = button_row.child(
            div()
                .id("plan-create-branch")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().surface))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .on_click(create_handler)
                .hover(|style| style.bg(rgb(theme().selected)))
                .child(SharedString::from("Create branch here...")),
        );
    }

    // Checkout button: only shown when there are no blockers.
    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("plan-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().color_branch))
                .text_sm()
                .text_color(rgb(theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from(confirm_label)),
        );
    }

    card = card.child(button_row);

    // ── Full-screen overlay wrapper ─────────────────────────────────────
    // Two layers: backdrop (semi-transparent) + centred card.
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        // Backdrop (dark, semi-transparent).
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                // Block mouse events from reaching the UI beneath the modal
                // (user-reported click-through on the create-branch dialog).
                .occlude()
                .bg(rgb(theme().modal_overlay))
                .opacity(0.65),
        )
        // Card centred on top of the backdrop.
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(card),
        )

}

// ──────────────────────────────────────────────────────────────
// Create-branch modal renderer (T014)
// ──────────────────────────────────────────────────────────────

/// W12-GCADOPT (§2.10): wrap a virtualized list in a relative flex column and
/// overlay a `gpui_component::scroll::Scrollbar` driven by the list's existing
/// `UniformListScrollHandle`.  The Scrollbar paints itself absolutely-positioned
/// over the container (relative(1.) size), so this is layout-non-destructive —
/// the inner `uniform_list` keeps its own `flex_1().min_h(0)` sizing.  Colours
/// follow the gpui-component scrollbar theme fields, which
/// `sync_gpui_component_theme` keeps in step with kagi's palette.
fn with_vertical_scrollbar(
    id: &'static str,
    handle: &UniformListScrollHandle,
    list: impl IntoElement,
) -> impl IntoElement {
    div()
        .id(id)
        .relative()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .child(list)
        .child(Scrollbar::vertical(handle))
}

/// Render the create-branch confirmation overlay.
///
/// Layout (absolute, full-screen):
/// - Semi-transparent dark backdrop
/// - Centred modal card:
///   - Title
///   - Branch name text input (live KeyDown handler)
///   - Live plan: Current → Predicted state
///   - Blockers (red) if any
///   - Error message (if preflight/execute failed)
///   - `[Cancel]` always; `[Create]` only when no blockers and name is non-empty
fn render_create_branch_modal(
    modal: CreateBranchModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let plan = modal.plan.clone();
    let has_blockers = plan.as_ref().map(|p| !p.blockers.is_empty()).unwrap_or(true);

    // ── Cancel handler ──────────────────────────────────────
    // T-BP-003: return focus to root_focus so cmd-j keeps working.
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_create_branch_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    // ── Confirm handler (only created when no blockers) ─────
    // T-BP-003: return focus to root_focus after confirm.
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.confirm_create_branch();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    // W12-GCADOPT (§2.7): replace the old `[ ]`/`[x]` pseudo-checkbox text with a
    // real `gpui_component::checkbox::Checkbox`.  Its `on_click` hands us the new
    // checked state; we route it through the same toggle + replan logic via the
    // KagiApp entity (Checkbox callbacks take `&mut App`, not `&mut Context`).
    let app_entity = cx.entity();
    let toggle_checkout = move |new_checked: &bool, _window: &mut Window, cx: &mut App| {
        let new_checked = *new_checked;
        app_entity.update(cx, |this, cx| {
            if let Some(ref mut modal) = this.create_branch_modal {
                modal.checkout_after = new_checked;
                modal.error = None;
            }
            this.replan_create_branch();
            cx.notify();
        });
    };

    // ── Build modal card ────────────────────────────────────
    let mut card = div()
        .w(px(480.))
        .bg(rgb(theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        // ── Title ─────────────────────────────────────────
        .child(
            div()
                .text_color(rgb(theme().text_main))
                .text_xl()
                .child(SharedString::from(format!(
                    "Create branch @ {}  {}",
                    modal.at.short(),
                    modal.start_title
                ))),
        )
        // ── Name input ────────────────────────────────────
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("Branch name")),
                )
                .children(modal.input_state.as_ref().map(|st| Input::new(st).small())),
        )
        .child(
            div()
                .px_2()
                .py_1()
                .child(
                    Checkbox::new("create-branch-checkout-after")
                        .label("Checkout after create")
                        .checked(modal.checkout_after)
                        .on_click(toggle_checkout),
                ),
        );

    // ── Plan state (current → predicted) ─────────────────
    if let Some(ref p) = plan {
        card = card.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("Current")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(p.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(theme().text_sub))
                                .child(SharedString::from(format!("[{}]", p.current.dirty))),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("\u{2192} Predicted")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_muted))
                        .child(SharedString::from(p.title.clone())),
                ),
        );

        // ── Blockers ──────────────────────────────────────
        if !p.blockers.is_empty() {
            let mut block_col = div().flex().flex_col().gap_1();
            for b in &p.blockers {
                block_col = block_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().color_blocker))
                        .overflow_hidden()
                        .child(SharedString::from(format!("\u{2717} {}", b))),
                );
            }
            card = card.child(block_col);
        }

        // ── Recovery ──────────────────────────────────────
        card = card.child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .overflow_hidden()
                .child(SharedString::from(p.recovery.clone())),
        );
    }

    // ── Error message (preflight / execute failure) ───────
    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    // ── Buttons ───────────────────────────────────────────
    let mut button_row = div()
        .flex()
        .flex_row()
        .gap_2()
        .justify_end()
        .child(
            div()
                .id("create-branch-cancel")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().surface))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .on_click(cancel_handler)
                .hover(|style| style.bg(rgb(theme().selected)))
                .child(SharedString::from("Cancel")),
        );

    // Create button: only shown when there are no blockers.
    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("create-branch-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().color_success))
                .text_sm()
                .text_color(rgb(theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Create")),
        );
    }

    card = card.child(button_row);

    // Real text inputs handle their own focus/keys now; the wrapper stays
    // only to keep the optional legacy focus handle alive.
    let focusable_card = if let Some(ref fh) = focus_handle {
        div().track_focus(fh).child(card)
    } else {
        div().child(card)
    };

    // ── Full-screen overlay wrapper ─────────────────────────
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                // Block mouse events from reaching the UI beneath the modal
                // (user-reported click-through on the create-branch dialog).
                .occlude()
                .bg(rgb(theme().modal_overlay))
                .opacity(0.65),
        )
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(focusable_card),
        )
}

fn render_create_worktree_modal(
    modal: CreateWorktreeModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let plan = modal.plan.clone();
    let has_blockers = plan.as_ref().map(|p| !p.blockers.is_empty()).unwrap_or(true);

    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_create_worktree_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.confirm_create_worktree();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    let mut card = div()
        .w(px(540.))
        .bg(rgb(theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_color(rgb(theme().text_main))
                .text_xl()
                .child(SharedString::from(format!(
                    "Create worktree @ {}  {}",
                    modal.at.short(),
                    modal.start_title
                ))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().text_color(rgb(theme().text_label)).child(SharedString::from("Branch name")))
                .children(modal.branch_state.as_ref().map(|st| Input::new(st).small())),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().text_color(rgb(theme().text_label)).child(SharedString::from("Path")))
                .children(modal.path_state.as_ref().map(|st| Input::new(st).small())),
        );

    if let Some(ref p) = plan {
        card = card.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().text_color(rgb(theme().text_label)).child(SharedString::from("Current")))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(div().text_color(rgb(theme().text_main)).child(SharedString::from(p.current.head.clone())))
                        .child(div().text_color(rgb(theme().text_sub)).child(SharedString::from(format!("[{}]", p.current.dirty)))),
                )
                .child(div().text_sm().text_color(rgb(theme().text_label)).child(SharedString::from("\u{2192} Predicted")))
                .child(div().text_sm().text_color(rgb(theme().text_muted)).child(SharedString::from(p.title.clone()))),
        );

        if !p.warnings.is_empty() {
            let mut warn_col = div().flex().flex_col().gap_1();
            for w in &p.warnings {
                warn_col = warn_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().color_warning))
                        .overflow_hidden()
                        .child(SharedString::from(format!("! {}", w))),
                );
            }
            card = card.child(warn_col);
        }

        if !p.blockers.is_empty() {
            let mut block_col = div().flex().flex_col().gap_1();
            for b in &p.blockers {
                block_col = block_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().color_blocker))
                        .overflow_hidden()
                        .child(SharedString::from(format!("\u{2717} {}", b))),
                );
            }
            card = card.child(block_col);
        }

        card = card.child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .overflow_hidden()
                .child(SharedString::from(p.recovery.clone())),
        );
    }

    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    let mut button_row = div()
        .flex()
        .flex_row()
        .gap_2()
        .justify_end()
        .child(
            div()
                .id("create-worktree-cancel")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().surface))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .on_click(cancel_handler)
                .hover(|style| style.bg(rgb(theme().selected)))
                .child(SharedString::from("Cancel")),
        );
    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("create-worktree-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().color_success))
                .text_sm()
                .text_color(rgb(theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Create")),
        );
    }
    card = card.child(button_row);

    let focusable_card = if let Some(ref fh) = focus_handle {
        div().track_focus(fh).child(card)
    } else {
        div().child(card)
    };

    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                // Block mouse events from reaching the UI beneath the modal
                // (user-reported click-through on the create-branch dialog).
                .occlude()
                .bg(rgb(theme().modal_overlay))
                .opacity(0.65),
        )
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(focusable_card),
        )
}

// ──────────────────────────────────────────────────────────────
// Stash push modal renderer (T015)
// ──────────────────────────────────────────────────────────────

/// Render the stash push confirmation overlay.
///
/// Layout (absolute, full-screen):
/// - Semi-transparent dark backdrop
/// - Centred modal card:
///   - Title
///   - Optional message text input (reuses T014 key-input pattern)
///   - Live plan: Current → Predicted state
///   - Warnings (yellow) if any
///   - Blockers (red) if any
///   - Error message (if execute failed)
///   - `[Cancel]` always; `[Stash]` only when no blockers
fn render_stash_push_modal(
    modal: StashPushModal,
    focus_handle: Option<FocusHandle>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let plan = modal.plan.clone();
    let has_blockers = plan.as_ref().map(|p| !p.blockers.is_empty()).unwrap_or(true);

    // T-BP-003: return focus to root_focus on cancel/confirm.
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_stash_push_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.confirm_stash_push();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });


    let mut card = div()
        .w(px(480.))
        .bg(rgb(theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_color(rgb(theme().text_main))
                .text_xl()
                .child(SharedString::from("Stash push — save local modifications")),
        )
        // ── Message input ──────────────────────────────────
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("Message (optional)")),
                )
                .children(modal.input_state.as_ref().map(|st| Input::new(st).small())),
        );

    // ── Plan state (current → predicted) ─────────────────
    if let Some(ref p) = plan {
        card = card.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("Current")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(p.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(theme().text_sub))
                                .child(SharedString::from(format!("[{}]", p.current.dirty))),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("\u{2192} Predicted")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(p.predicted.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(theme().text_sub))
                                .child(SharedString::from(format!("[{}]", p.predicted.dirty))),
                        ),
                ),
        );

        // ── Warnings ──────────────────────────────────────
        if !p.warnings.is_empty() {
            let mut warn_col = div().flex().flex_col().gap_1();
            for w in &p.warnings {
                warn_col = warn_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().color_warning))
                        .overflow_hidden()
                        .child(SharedString::from(format!("\u{26a0} {}", w))),
                );
            }
            card = card.child(warn_col);
        }

        // ── Blockers ──────────────────────────────────────
        if !p.blockers.is_empty() {
            let mut block_col = div().flex().flex_col().gap_1();
            for b in &p.blockers {
                block_col = block_col.child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().color_blocker))
                        .overflow_hidden()
                        .child(SharedString::from(format!("\u{2717} {}", b))),
                );
            }
            card = card.child(block_col);
        }

        // ── Recovery ──────────────────────────────────────
        card = card.child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .overflow_hidden()
                .child(SharedString::from(p.recovery.clone())),
        );
    }

    // ── Error message ──────────────────────────────────
    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    // ── Buttons ───────────────────────────────────────────
    let mut button_row = div()
        .flex()
        .flex_row()
        .gap_2()
        .justify_end()
        .child(
            div()
                .id("stash-push-cancel")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().surface))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .on_click(cancel_handler)
                .hover(|style| style.bg(rgb(theme().selected)))
                .child(SharedString::from("Cancel")),
        );

    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("stash-push-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().color_warning))
                .text_sm()
                .text_color(rgb(theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Stash")),
        );
    }

    card = card.child(button_row);

    let focusable_card = if let Some(ref fh) = focus_handle {
        div().track_focus(fh).child(card)
    } else {
        div().child(card)
    };

    // ── Full-screen overlay wrapper ─────────────────────────
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                // Block mouse events from reaching the UI beneath the modal
                // (user-reported click-through on the create-branch dialog).
                .occlude()
                .bg(rgb(theme().modal_overlay))
                .opacity(0.65),
        )
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(focusable_card),
        )
}

// ──────────────────────────────────────────────────────────────
// Stash apply modal renderer (T015)
// ──────────────────────────────────────────────────────────────

/// Render the stash apply confirmation overlay.
///
/// Layout (absolute, full-screen):
/// - Semi-transparent dark backdrop
/// - Centred modal card:
///   - Title (showing stash index)
///   - Current → Predicted state
///   - Blockers (red) if any
///   - Recovery text
///   - Error message (if execute failed)
///   - `[Cancel]` always; `[Apply]` only when no blockers
fn render_stash_apply_modal(
    modal: StashApplyModal,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let plan = modal.plan.clone();
    let has_blockers = !plan.blockers.is_empty();

    // T-BP-003: return focus to root_focus on cancel/confirm.
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_stash_apply_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.confirm_stash_apply();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    let mut card = div()
        .w(px(480.))
        .bg(rgb(theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_color(rgb(theme().text_main))
                .text_xl()
                .child(SharedString::from(plan.title.clone())),
        )
        // ── Current → Predicted ─────────────────────────────
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("Current")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(plan.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(theme().text_sub))
                                .child(SharedString::from(format!("[{}]", plan.current.dirty))),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("\u{2192} Predicted")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(plan.predicted.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(theme().text_sub))
                                .child(SharedString::from(format!("[{}]", plan.predicted.dirty))),
                        ),
                ),
        );

    // ── Blockers ──────────────────────────────────────────
    if !plan.blockers.is_empty() {
        let mut block_col = div().flex().flex_col().gap_1();
        for b in &plan.blockers {
            block_col = block_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().color_blocker))
                    .overflow_hidden()
                    .child(SharedString::from(format!("\u{2717} {}", b))),
            );
        }
        card = card.child(block_col);
    }

    // ── Recovery ──────────────────────────────────────────
    card = card.child(
        div()
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .overflow_hidden()
            .child(SharedString::from(plan.recovery.clone())),
    );

    // ── Error message ────────────────────────────────────
    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    // ── Buttons ───────────────────────────────────────────
    let mut button_row = div()
        .flex()
        .flex_row()
        .gap_2()
        .justify_end()
        .child(
            div()
                .id("stash-apply-cancel")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().surface))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .on_click(cancel_handler)
                .hover(|style| style.bg(rgb(theme().selected)))
                .child(SharedString::from("Cancel")),
        );

    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("stash-apply-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().color_success))
                .text_sm()
                .text_color(rgb(theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Apply")),
        );
    }

    card = card.child(button_row);

    // ── Full-screen overlay wrapper ─────────────────────────
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                // Block mouse events from reaching the UI beneath the modal
                // (user-reported click-through on the create-branch dialog).
                .occlude()
                .bg(rgb(theme().modal_overlay))
                .opacity(0.65),
        )
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(card),
        )
}

// ──────────────────────────────────────────────────────────────
// Cherry-pick modal renderer (T016)
// ──────────────────────────────────────────────────────────────

/// Render the cherry-pick plan confirmation overlay.
///
/// Layout (absolute, full-screen):
/// - Semi-transparent dark backdrop
/// - Centred modal card:
///   - Title (commit short sha + summary onto HEAD branch)
///   - Current → Predicted state
///   - Preview files section (file tree, reusing T018 build_file_tree)
///   - Blockers (red) if any — includes conflict file names
///   - Recovery text
///   - Error message (if preflight/execute failed)
///   - `[Cancel]` always; `[Cherry-pick]` only when no blockers
fn render_cherry_pick_modal(
    modal: CherryPickModal,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let plan = modal.plan.clone();
    let has_blockers = !plan.blockers.is_empty();

    // T-BP-003: return focus to root_focus on cancel/confirm.
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_cherry_pick_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.confirm_cherry_pick();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    // Change-kind colours come from the active theme (W9-THEME).

    // ── Build preview file tree rows ────────────────────────
    let tree_rows = file_tree::build_file_tree(&plan.preview_files);
    let tree_element_rows: Vec<_> = tree_rows.iter().map(|row| {
        match row {
            file_tree::TreeRow::Dir { depth, name } => {
                let indent = (*depth as f32) * 12.0;
                div()
                    .id(SharedString::from(format!("cpk-dir-{}", name.as_ref())))
                    .flex()
                    .flex_row()
                    .items_center()
                    .pl(px(indent))
                    .mb_px()
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme().change_dir))
                            .child(name.clone()),
                    )
                    .into_any()
            }
            file_tree::TreeRow::File { depth, name, file_index, change } => {
                let indent = (*depth as f32) * 12.0;
                let (badge_char, badge_color) = match change {
                    ChangeKind::Added      => ("A", theme().change_added),
                    ChangeKind::Modified   => ("M", theme().change_modified),
                    ChangeKind::Deleted    => ("D", theme().change_deleted),
                    ChangeKind::Renamed { .. } => ("R", theme().change_renamed),
                    ChangeKind::TypeChange => ("T", theme().change_typechange),
                };
                let _ = file_index; // not clickable in preview
                div()
                    .id(("cpk-file", *file_index))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .pl(px(indent))
                    .mb_px()
                    .child(
                        div()
                            .w(px(14.))
                            .flex_shrink_0()
                            .text_sm()
                            .text_color(rgb(badge_color))
                            .child(SharedString::from(badge_char)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(rgb(theme().text_main))
                            .overflow_hidden()
                            .child(name.clone()),
                    )
                    .into_any()
            }
        }
    }).collect();

    // ── Build modal card ────────────────────────────────────
    let mut card = div()
        .w(px(520.))
        .bg(rgb(theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        // ── Title ─────────────────────────────────────────
        .child(
            div()
                .text_color(rgb(theme().text_main))
                .text_xl()
                .child(SharedString::from(plan.title.clone())),
        )
        // ── Current → Predicted ───────────────────────────
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("Current")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(plan.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(theme().text_sub))
                                .child(SharedString::from(format!("[{}]", plan.current.dirty))),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("\u{2192} Predicted")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(plan.predicted.head.clone())),
                        ),
                ),
        );

    // ── Preview files section ─────────────────────────────
    if !plan.preview_files.is_empty() {
        let mut preview_col = div()
            .flex()
            .flex_col()
            .gap_px()
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().text_label))
                    .mb_1()
                    .child(SharedString::from(format!(
                        "Preview ({} file{})",
                        plan.preview_files.len(),
                        if plan.preview_files.len() == 1 { "" } else { "s" }
                    ))),
            );
        for row in tree_element_rows {
            preview_col = preview_col.child(row);
        }
        card = card.child(preview_col);
    }

    // ── Warnings ──────────────────────────────────────────
    if !plan.warnings.is_empty() {
        let mut warn_col = div().flex().flex_col().gap_1();
        for w in &plan.warnings {
            warn_col = warn_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().color_warning))
                    .overflow_hidden()
                    .child(SharedString::from(format!("\u{26a0} {}", w))),
            );
        }
        card = card.child(warn_col);
    }

    // ── Blockers ──────────────────────────────────────────
    if !plan.blockers.is_empty() {
        let mut block_col = div().flex().flex_col().gap_1();
        for b in &plan.blockers {
            block_col = block_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().color_blocker))
                    .overflow_hidden()
                    .child(SharedString::from(format!("\u{2717} {}", b))),
            );
        }
        card = card.child(block_col);
    }

    // ── Recovery ──────────────────────────────────────────
    card = card.child(
        div()
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .overflow_hidden()
            .child(SharedString::from(plan.recovery.clone())),
    );

    // ── Error message (preflight / execute failure) ───────
    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    // ── Buttons ───────────────────────────────────────────
    let mut button_row = div()
        .flex()
        .flex_row()
        .gap_2()
        .justify_end()
        .child(
            div()
                .id("cherry-pick-cancel")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().surface))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .on_click(cancel_handler)
                .hover(|style| style.bg(rgb(theme().selected)))
                .child(SharedString::from("Cancel")),
        );

    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("cherry-pick-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().accent)) // mauve accent
                .text_sm()
                .text_color(rgb(theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Cherry-pick")),
        );
    }

    card = card.child(button_row);

    // ── Full-screen overlay wrapper ─────────────────────────
    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                // Block mouse events from reaching the UI beneath the modal
                // (user-reported click-through on the create-branch dialog).
                .occlude()
                .bg(rgb(theme().modal_overlay))
                .opacity(0.65),
        )
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(card),
        )
}

// ──────────────────────────────────────────────────────────────
// Commit Panel renderer (T025)
// ──────────────────────────────────────────────────────────────

/// Render the Commit Panel: unstaged/staged sections + diff viewer + message input + commit button.
///
/// Layout (top to bottom in right panel):
/// 1. Unstaged (N)  [flat|tree] toggle
/// 2. Staged (M)
/// 3. Diff viewer (flex_1)
/// 4. Message input (T014 pattern — simple key handler)
/// 5. Warning (if unstaged remain)
/// 6. Commit button (disabled when staged=0 or message empty)
fn render_commit_panel(
    panel: CommitPanelState,
    panel_width: f32,
    commit_input: Option<Entity<InputState>>,
    active_wip: Option<(bool, PathBuf)>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // theme().change_dir now sourced from theme().change_dir (W9-THEME).

    let tree_view = panel.tree_view;
    let unstaged_count = panel.unstaged.len();
    let staged_count = panel.staged.len();
    // T026: can_commit uses InputState value if available, else commit_msg (headless).
    let input_msg_nonempty = commit_input
        .as_ref()
        .map(|e| !e.read(cx).value().trim().is_empty())
        .unwrap_or(!panel.commit_msg.trim().is_empty());
    let can_commit = !panel.staged.is_empty() && input_msg_nonempty;
    let has_unstaged_warning = !panel.unstaged.is_empty() && staged_count > 0;
    // T-UI-003: selected_file tracks which row is highlighted in the panel.
    let selected_file = panel.selected_file.clone();

    // ── View switch: segmented [List | Tree] (T-UI-002) ──────
    let list_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
        if let Some(panel) = this.commit_panel.as_mut() { panel.tree_view = false; }
        cx.notify();
    });
    let tree_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
        if let Some(panel) = this.commit_panel.as_mut() { panel.tree_view = true; }
        cx.notify();
    });
    let seg = |id: &'static str, label: &'static str, active: bool| {
        div()
            .id(id)
            .px_1p5()
            .py_px()
            .text_xs()
            .bg(rgb(if active { theme().selected } else { theme().surface }))
            .text_color(rgb(if active { theme().text_main } else { theme().text_muted }))
            .hover(|st| st.text_color(rgb(theme().text_main)).cursor_pointer())
            .child(SharedString::from(label))
    };
    let toggle_btn = div()
        .flex()
        .flex_row()
        .rounded_sm()
        .overflow_hidden()
        .border_1()
        .border_color(rgb(theme().surface))
        .child(seg("cp-view-list", "List", !tree_view).on_click(list_click))
        .child(seg("cp-view-tree", "Tree", tree_view).on_click(tree_click));

    // ── Helper: build file rows for a section ────────────────
    // Returns a Vec of (element, depth, name, is_conflicted) as IntoElement.
    // We render inline to avoid capture issues.

    // ── Unstaged section ─────────────────────────────────────
    // T027: ヘッダ行は箱の外に固定し、ファイル行のみをスクロールボックス内に入れる

    // Unstaged ヘッダ行 (固定 — flex_shrink_0 で高さを保持)
    let unstaged_header = div()
        .flex()
        .flex_row()
        .items_center()
        .px_2()
        .py_1()
        .flex_shrink_0()
        .child(
            div()
                .flex_1()
                .text_sm()
                .text_color(rgb(theme().text_label))
                .child(SharedString::from(format!("Unstaged ({})", unstaged_count))),
        )
        .when(unstaged_count > 0, |el| {
            let stage_all_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
                this.do_stage_all();
                cx.notify();
            });
            el.child(
                div()
                    .id("cp-stage-all")
                    .mr_2()
                    .px_1p5()
                    .py_px()
                    .rounded_sm()
                    .bg(rgb(theme().surface))
                    .text_xs()
                    .text_color(rgb(theme().color_success))
                    .hover(|st| st.bg(rgb(theme().selected)).cursor_pointer())
                    .on_click(stage_all_click)
                    .child(SharedString::from("Stage all")),
            )
        })
        .child(toggle_btn);

    // Unstaged ファイル行コンテナ (スクロールボックス内に入る)
    let mut unstaged_files = div()
        .flex()
        .flex_col();

    if tree_view {
        // Tree view: use build_file_tree
        let tree_rows = file_tree::build_file_tree(&panel.unstaged);
        for row in &tree_rows {
            match row {
                file_tree::TreeRow::Dir { depth, name } => {
                    let indent = (*depth as f32) * 12.0;
                    unstaged_files = unstaged_files.child(
                        div()
                            .id(SharedString::from(format!("cp-us-dir-{}", name.as_ref())))
                            .pl(px(8.0 + indent))
                            .text_xs()
                            .text_color(rgb(theme().change_dir))
                            .child(name.clone()),
                    );
                }
                file_tree::TreeRow::File { depth, name, file_index, change } => {
                    let indent = (*depth as f32) * 12.0;
                    let fi = *file_index;
                    // Look up the original path to check if conflicted
                    let is_conflicted_file = panel.unstaged.get(fi)
                        .map(|f| panel.is_conflicted(&f.path))
                        .unwrap_or(false);
                    let (badge, badge_color, _) = status_badge(change, is_conflicted_file);
                    let is_sel = selected_file == Some(CommitPanelFileRef::Unstaged { index: fi });
                    let file_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                        this.select_commit_panel_file(CommitPanelFileRef::Unstaged { index: fi });
                        cx.notify();
                    });
                    let stage_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                        this.do_stage_file(fi);
                        cx.notify();
                    });
                    let row_bg = if is_conflicted_file { theme().diff_removed_bg } else if is_sel { theme().selected } else { theme().panel };
                    let mut file_row = div()
                        .id(("cp-us-file", fi))
                        .when(
                            active_wip.as_ref().map_or(false, |(st, p)| {
                                *st == false && panel.unstaged.get(fi).map_or(false, |f| &f.path == p)
                            }),
                            |el| el.bg(rgb(theme().selected)),
                        )
                        .flex()
                        .flex_row()
                        .items_center()
                        .pl(px(8.0 + indent))
                        .pr(px(2.0))
                        .py_px()
                        .bg(rgb(row_bg))
                        .hover(|s| s.bg(rgb(theme().surface)))
                        .on_click(file_click)
                        .child(
                            div()
                                .w(px(12.))
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(rgb(badge_color))
                                .child(SharedString::from(badge)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_xs()
                                .text_color(rgb(theme().text_main))
                                .overflow_hidden()
                                .truncate()
                                .child(name.clone()),
                        );
                    if !is_conflicted_file {
                        file_row = file_row.child(
                            div()
                                .id(("cp-us-stage-btn", fi))
                                .px_1()
                                .py_px()
                                .rounded_sm()
                                .flex_shrink_0()
                                .bg(rgb(theme().color_success))
                                .text_xs()
                                .text_color(rgb(theme().bg_base))
                                .on_click(stage_click)
                                .hover(|s| s.opacity(0.8))
                                .child(SharedString::from("Stage")),
                        );
                    } else {
                        file_row = file_row.child(
                            div()
                                .id(("cp-us-conflict-badge", fi))
                                .px_1()
                                .py_px()
                                .rounded_sm()
                                .flex_shrink_0()
                                .bg(rgb(theme().color_blocker))
                                .text_xs()
                                .text_color(rgb(theme().bg_base))
                                .child(SharedString::from("Conflict")),
                        );
                    }
                    unstaged_files = unstaged_files.child(file_row);
                }
            }
        }
    } else {
        // Flat view
        for (fi, f) in panel.unstaged.iter().enumerate() {
            let name = f.path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| f.path.to_string_lossy().into_owned());
            let is_conflicted_file = panel.is_conflicted(&f.path);
            let (badge, badge_color, _) = status_badge(&f.change, is_conflicted_file);
            let is_sel = selected_file == Some(CommitPanelFileRef::Unstaged { index: fi });
            let file_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.select_commit_panel_file(CommitPanelFileRef::Unstaged { index: fi });
                cx.notify();
            });
            let stage_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.do_stage_file(fi);
                cx.notify();
            });
            // Row background: conflicted files get red tint
            let row_bg = if is_conflicted_file { theme().diff_removed_bg } else if is_sel { theme().selected } else { theme().panel };
            let mut file_row = div()
                .id(("cp-us-flat-file", fi))
                        .when(
                            active_wip.as_ref().map_or(false, |(st, p)| {
                                *st == false && panel.unstaged.get(fi).map_or(false, |f| &f.path == p)
                            }),
                            |el| el.bg(rgb(theme().selected)),
                        )
                .flex()
                .flex_row()
                .items_center()
                .px_2()
                .py_px()
                .bg(rgb(row_bg))
                .hover(|s| s.bg(rgb(theme().surface)))
                .on_click(file_click)
                .child(
                    div()
                        .w(px(12.))
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(rgb(badge_color))
                        .child(SharedString::from(badge)),
                )
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .text_color(rgb(theme().text_main))
                        .overflow_hidden()
                        .truncate()
                        .child(SharedString::from(name)),
                );
            // Stage button only for non-conflicted files
            if !is_conflicted_file {
                file_row = file_row.child(
                    div()
                        .id(("cp-us-flat-stage-btn", fi))
                        .px_1()
                        .py_px()
                        .rounded_sm()
                        .flex_shrink_0()
                        .bg(rgb(theme().color_success))
                        .text_xs()
                        .text_color(rgb(theme().bg_base))
                        .on_click(stage_click)
                        .hover(|s| s.opacity(0.8))
                        .child(SharedString::from("Stage")),
                );
            } else {
                file_row = file_row.child(
                    div()
                        .id(("cp-us-flat-conflict-badge", fi))
                        .px_1()
                        .py_px()
                        .rounded_sm()
                        .flex_shrink_0()
                        .bg(rgb(theme().color_blocker)) // red
                        .text_xs()
                        .text_color(rgb(theme().bg_base))
                        .child(SharedString::from("Conflict")),
                );
            }
            unstaged_files = unstaged_files.child(file_row);
        }
    }

    // ── Staged section ───────────────────────────────────────
    // T027: ヘッダ行は箱の外に固定し、ファイル行のみをスクロールボックス内に入れる

    // Staged ヘッダ行 (固定)
    let staged_header = div()
        .flex()
        .flex_row()
        .items_center()
        .px_2()
        .py_1()
        .flex_shrink_0()
        .child(
            div()
                .flex_1()
                .text_sm()
                .text_color(rgb(theme().text_label))
                .child(SharedString::from(format!("Staged ({})", staged_count))),
        )
        .when(staged_count > 0, |el| {
            let unstage_all_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
                this.do_unstage_all();
                cx.notify();
            });
            el.child(
                div()
                    .id("cp-unstage-all")
                    .px_1p5()
                    .py_px()
                    .rounded_sm()
                    .bg(rgb(theme().surface))
                    .text_xs()
                    .text_color(rgb(theme().color_warning))
                    .hover(|st| st.bg(rgb(theme().selected)).cursor_pointer())
                    .on_click(unstage_all_click)
                    .child(SharedString::from("Unstage all")),
            )
        });

    // Staged ファイル行コンテナ (スクロールボックス内に入る)
    let mut staged_files = div()
        .flex()
        .flex_col();

    if tree_view {
        let tree_rows = file_tree::build_file_tree(&panel.staged);
        for row in &tree_rows {
            match row {
                file_tree::TreeRow::Dir { depth, name } => {
                    let indent = (*depth as f32) * 12.0;
                    staged_files = staged_files.child(
                        div()
                            .id(SharedString::from(format!("cp-st-dir-{}", name.as_ref())))
                            .pl(px(8.0 + indent))
                            .text_xs()
                            .text_color(rgb(theme().change_dir))
                            .child(name.clone()),
                    );
                }
                file_tree::TreeRow::File { depth, name, file_index, change } => {
                    let indent = (*depth as f32) * 12.0;
                    let fi = *file_index;
                    let (badge, badge_color, _conflicted) = status_badge(change, false);
                    let is_sel = selected_file == Some(CommitPanelFileRef::Staged { index: fi });
                    let file_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                        this.select_commit_panel_file(CommitPanelFileRef::Staged { index: fi });
                        cx.notify();
                    });
                    let unstage_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                        this.do_unstage_file(fi);
                        cx.notify();
                    });
                    staged_files = staged_files.child(
                        div()
                            .id(("cp-st-file", fi))
                        .when(
                            active_wip.as_ref().map_or(false, |(st, p)| {
                                *st == true && panel.staged.get(fi).map_or(false, |f| &f.path == p)
                            }),
                            |el| el.bg(rgb(theme().selected)),
                        )
                            .flex()
                            .flex_row()
                            .items_center()
                            .pl(px(8.0 + indent))
                            .pr(px(2.0))
                            .py_px()
                            .bg(rgb(if is_sel { theme().selected } else { theme().panel }))
                            .hover(|s| s.bg(rgb(theme().surface)))
                            .on_click(file_click)
                            .child(
                                div()
                                    .w(px(12.))
                                    .flex_shrink_0()
                                    .text_xs()
                                    .text_color(rgb(badge_color))
                                    .child(SharedString::from(badge)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(rgb(theme().text_main))
                                    .overflow_hidden()
                                    .truncate()
                                    .child(name.clone()),
                            )
                            .child(
                                div()
                                    .id(("cp-st-unstage-btn", fi))
                                    .px_1()
                                    .py_px()
                                    .rounded_sm()
                                    .flex_shrink_0()
                                    .bg(rgb(theme().color_warning))
                                    .text_xs()
                                    .text_color(rgb(theme().bg_base))
                                    .on_click(unstage_click)
                                    .hover(|s| s.opacity(0.8))
                                    .child(SharedString::from("Unstage")),
                            ),
                    );
                }
            }
        }
    } else {
        for (fi, f) in panel.staged.iter().enumerate() {
            let name = f.path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| f.path.to_string_lossy().into_owned());
            let (badge, badge_color, _conflicted) = status_badge(&f.change, false);
            let is_sel = selected_file == Some(CommitPanelFileRef::Staged { index: fi });
            let file_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.select_commit_panel_file(CommitPanelFileRef::Staged { index: fi });
                cx.notify();
            });
            let unstage_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.do_unstage_file(fi);
                cx.notify();
            });
            staged_files = staged_files.child(
                div()
                    .id(("cp-st-flat-file", fi))
                        .when(
                            active_wip.as_ref().map_or(false, |(st, p)| {
                                *st == true && panel.staged.get(fi).map_or(false, |f| &f.path == p)
                            }),
                            |el| el.bg(rgb(theme().selected)),
                        )
                    .flex()
                    .flex_row()
                    .items_center()
                    .px_2()
                    .py_px()
                    .bg(rgb(if is_sel { theme().selected } else { theme().panel }))
                    .hover(|s| s.bg(rgb(theme().surface)))
                    .on_click(file_click)
                    .child(
                        div()
                            .w(px(12.))
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(rgb(badge_color))
                            .child(SharedString::from(badge)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(rgb(theme().text_main))
                            .overflow_hidden()
                            .truncate()
                            .child(SharedString::from(name)),
                    )
                    .child(
                        div()
                            .id(("cp-st-flat-unstage-btn", fi))
                            .px_1()
                            .py_px()
                            .rounded_sm()
                            .flex_shrink_0()
                            .bg(rgb(theme().color_warning))
                            .text_xs()
                            .text_color(rgb(theme().bg_base))
                            .on_click(unstage_click)
                            .hover(|s| s.opacity(0.8))
                            .child(SharedString::from("Unstage")),
                    ),
            );
        }
    }

    // ── Commit message input (T026: gpui-component Input with IME support) ────────────
    let msg_input_wrapper: gpui::AnyElement = if let Some(ref input_entity) = commit_input {
        // Use gpui-component Input element — handles IME, clipboard, arrow keys, etc.
        Input::new(input_entity)
            .appearance(true)
            .bordered(true)
            .into_any_element()
    } else {
        // Fallback for headless / no-window case (should not occur in normal UI flow).
        div()
            .px_2()
            .py_1()
            .bg(rgb(theme().bg_base))
            .rounded_sm()
            .text_xs()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from("(commit message input unavailable)"))
            .into_any_element()
    };

    // ── Commit button ─────────────────────────────────────────
    let commit_btn = if can_commit {
        let commit_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
            this.open_commit_plan_modal(cx);
            cx.notify();
        });
        div()
            .id("cp-commit-btn")
            .mt_1()
            .w_full()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(rgb(theme().color_branch))
            .text_sm()
            .text_color(rgb(theme().bg_base))
            .on_click(commit_click)
            .hover(|s| s.opacity(0.85))
            .child(SharedString::from(format!("Commit ({} file{})",
                staged_count,
                if staged_count == 1 { "" } else { "s" }
            )))
            .into_any_element()
    } else {
        // Tell the user exactly why the button is disabled.
        let reason = if staged_count == 0 && !input_msg_nonempty {
            "Commit — stage a file and enter a message first"
        } else if staged_count == 0 {
            "Commit — stage at least one file first"
        } else {
            "Commit — enter a commit message first"
        };
        div()
            .id("cp-commit-btn-disabled")
            .mt_1()
            .w_full()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(rgb(theme().surface))
            .text_sm()
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from(reason))
            .into_any_element()
    };

    // ── Assemble panel ───────────────────────────────────────
    // T-UI-003: diff ボックス廃止。Unstaged/Staged 箱が flex_1 で全体を占める(1:1)。
    div()
        .w(px(panel_width))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(theme().panel))
        // Header
        .child(
            div()
                .flex_shrink_0()
                .px_2()
                .py_1()
                .bg(rgb(theme().surface))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .child(SharedString::from("Commit Panel")),
        )
        // T-UI-003: ファイル領域コンテナ (flex_1 + min_h(0)) — diff 廃止でフル高さ
        .child(
            div()
                .id("cp-files-container")
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_col()
                // Unstaged ヘッダ (固定)
                .child(unstaged_header)
                // Unstaged スクロールボックス (flex_1 + min_h(0) + 薄枠)
                .child(
                    div()
                        .id("cp-unstaged-scroll")
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .mx_1()
                        .mb_px()
                        .border_1()
                        .border_color(rgb(theme().surface))
                        .rounded_sm()
                        .child(unstaged_files),
                )
                // Staged ヘッダ (固定)
                .child(staged_header)
                // Staged スクロールボックス (flex_1 + min_h(0) + 薄枠)
                .child(
                    div()
                        .id("cp-staged-scroll")
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .mx_1()
                        .mb_px()
                        .border_1()
                        .border_color(rgb(theme().surface))
                        .rounded_sm()
                        .child(staged_files),
                ),
        )
        // Commit footer: message input + warning + button
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_col()
                .px_2()
                .py_1()
                .gap_1()
                .bg(rgb(theme().surface))
                // Message label + input
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("Commit message")),
                )
                .child(msg_input_wrapper)
                // Unstaged warning
                .when(has_unstaged_warning, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().color_warning))
                            .child(SharedString::from(format!(
                                "⚠ {} unstaged change(s) not included",
                                unstaged_count
                            ))),
                    )
                })
                // Commit button
                .child(commit_btn),
        )
}

// ──────────────────────────────────────────────────────────────
// Commit Plan modal renderer (T025)
// ──────────────────────────────────────────────────────────────

/// Render the commit plan confirmation overlay.
///
/// Layout (absolute, full-screen):
/// - Semi-transparent dark backdrop
/// - Centred modal card:
///   - Title
///   - Preview files (staged files)
///   - Warnings (unstaged remain)
///   - Error message (if execute failed)
///   - `[Cancel]` always; `[Commit]` when no blockers
fn render_commit_plan_modal(
    modal: CommitPlanModal,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    let plan = modal.plan.clone();
    let has_blockers = !plan.blockers.is_empty();

    // T-BP-003: return focus to root_focus on cancel/confirm.
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.cancel_commit_plan_modal();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, window, cx| {
        this.confirm_commit(cx);
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });

    // ── Preview file tree ────────────────────────────────────
    let tree_rows = file_tree::build_file_tree(&plan.preview_files);
    let mut preview_col = div().flex().flex_col().gap_px()
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme().text_label))
                .mb_1()
                .child(SharedString::from(format!(
                    "Staging ({} file{})",
                    plan.preview_files.len(),
                    if plan.preview_files.len() == 1 { "" } else { "s" }
                ))),
        );

    for row in &tree_rows {
        match row {
            file_tree::TreeRow::Dir { depth, name } => {
                let indent = (*depth as f32) * 12.0;
                preview_col = preview_col.child(
                    div()
                        .id(SharedString::from(format!("cpk-dir-{}", name.as_ref())))
                        .pl(px(indent))
                        .text_xs()
                        .text_color(rgb(theme().change_dir))
                        .child(name.clone()),
                );
            }
            file_tree::TreeRow::File { depth, name, file_index, change } => {
                let indent = (*depth as f32) * 12.0;
                let (badge, badge_color, _) = status_badge(change, false);
                let _ = file_index;
                preview_col = preview_col.child(
                    div()
                        .id(("cpk-file", *file_index))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .pl(px(indent))
                        .child(
                            div()
                                .w(px(14.))
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(rgb(badge_color))
                                .child(SharedString::from(badge)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_xs()
                                .text_color(rgb(theme().text_main))
                                .overflow_hidden()
                                .child(name.clone()),
                        ),
                );
            }
        }
    }

    let mut card = div()
        .w(px(480.))
        .bg(rgb(theme().modal))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_color(rgb(theme().text_main))
                .text_xl()
                .child(SharedString::from(plan.title.clone())),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("Current")),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(theme().text_main))
                                .child(SharedString::from(plan.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(theme().text_sub))
                                .child(SharedString::from(format!("[{}]", plan.current.dirty))),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_label))
                        .child(SharedString::from("\u{2192} Predicted")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme().text_main))
                        .child(SharedString::from(plan.predicted.head.clone())),
                ),
        )
        // Preview files
        .child(preview_col);

    // Warnings
    if !plan.warnings.is_empty() {
        let mut warn_col = div().flex().flex_col().gap_1();
        for w in &plan.warnings {
            warn_col = warn_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().color_warning))
                    .overflow_hidden()
                    .child(SharedString::from(format!("\u{26a0} {}", w))),
            );
        }
        card = card.child(warn_col);
    }

    // Blockers
    if !plan.blockers.is_empty() {
        let mut block_col = div().flex().flex_col().gap_1();
        for b in &plan.blockers {
            block_col = block_col.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme().color_blocker))
                    .overflow_hidden()
                    .child(SharedString::from(format!("\u{2717} {}", b))),
            );
        }
        card = card.child(block_col);
    }

    // Error
    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(theme().color_blocker))
                .overflow_hidden()
                .child(err.clone()),
        );
    }

    let mut button_row = div()
        .flex()
        .flex_row()
        .gap_2()
        .justify_end()
        .child(
            div()
                .id("commit-plan-cancel")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().surface))
                .text_sm()
                .text_color(rgb(theme().text_main))
                .on_click(cancel_handler)
                .hover(|style| style.bg(rgb(theme().selected)))
                .child(SharedString::from("Cancel")),
        );

    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("commit-plan-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(theme().color_branch))
                .text_sm()
                .text_color(rgb(theme().bg_base))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Commit")),
        );
    }

    card = card.child(button_row);

    div()
        .size_full()
        .absolute()
        .top_0()
        .left_0()
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                // Block mouse events from reaching the UI beneath the modal
                // (user-reported click-through on the create-branch dialog).
                .occlude()
                .bg(rgb(theme().modal_overlay))
                .opacity(0.65),
        )
        .child(
            div()
                .size_full()
                .absolute()
                .top_0()
                .left_0()
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(card),
        )
}

// ──────────────────────────────────────────────────────────────
// Application entry point helper
// ──────────────────────────────────────────────────────────────

/// Open the GPUI window and start the event loop.
pub fn run_app(mut app_state: KagiApp) {
    use gpui::{Application, Bounds, WindowBounds, WindowOptions, size};

    // W4-TABS / ADR-0027: the watcher is armed from inside the window context
    // via `arm_watcher` (generation scheme), replacing the fixed spawn that
    // used to live here.  No pre-window watcher is created.

    Application::new()
        .with_assets(assets::KagiAssets)
        .run(move |cx: &mut App| {
        // T025: initialize gpui-component (registers key bindings, themes, etc.)
        gpui_component::init(cx);

        // W12-GCADOPT: gpui_component::init runs `sync_system_appearance`, which
        // seeds the gpui-component palette from the OS light/dark setting.  Push
        // kagi's active theme (already resolved by `theme::init_active` in main)
        // on top so adopted components (Input, Tooltip, Scrollbar, Checkbox…)
        // render in kagi's colours rather than the system default.
        theme::sync_gpui_component_theme(cx);

        // T-BP-002: register cmd-j as the toggle key for the bottom panel.
        // context = None means the binding fires regardless of focus context.
        cx.bind_keys([KeyBinding::new("cmd-j", ToggleBottomPanel, None)]);
        // T-UI-003: Esc closes the main diff view (no-op when main_diff is None).
        cx.bind_keys([KeyBinding::new("escape", CloseMainDiff, None)]);
        // Arrow keys step through files while the main diff is open
        // (no-ops otherwise; see main_diff_step).
        cx.bind_keys([
            KeyBinding::new("up", DiffPrevFile, None),
            KeyBinding::new("down", DiffNextFile, None),
        ]);

        // W5-MENU / ADR-0029: register the command-registry keystrokes and the
        // native menu bar.  Keystrokes are passed into `set_menus` via the live
        // keymap, so they render next to each menu item automatically.
        commands::register_keybindings(cx);
        cx.set_menus(commands::build_menus());

        // KAGI_WINDOW=WxH (dev/testing only): override the initial window size
        // so layout behaviour at small sizes can be verified headlessly.
        let (win_w, win_h) = std::env::var("KAGI_WINDOW")
            .ok()
            .and_then(|s| {
                let (w, h) = s.split_once('x')?;
                Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
            })
            .unwrap_or((1440.0, 920.0));
        let bounds = Bounds::centered(None, size(px(win_w), px(win_h)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                // gpui-component widgets (Input etc.) require the window's
                // first layer to be a `gpui_component::Root`; rendering
                // KagiApp directly panics inside Root::read (user-reported
                // crash when opening the commit panel).
                let kagi: Entity<KagiApp> = cx.new(|cx| {
                    // Root focus handle: without a focused element gpui never
                    // dispatches key events, so cmd-j (and future shortcuts)
                    // would silently do nothing.
                    app_state.root_focus = Some(cx.focus_handle());
                    app_state
                });
                if let Some(fh) = kagi.read(cx).root_focus.clone() {
                    window.focus(&fh);
                }
                // Regression coverage for the Root::read crash: with
                // KAGI_COMMIT_PANEL=1, open the panel through the real
                // window-context path so the InputState + Input element
                // actually render during headless verification (the
                // pre-window env path in main.rs cannot create them).
                if std::env::var("KAGI_COMMIT_PANEL").as_deref() == Ok("1") {
                    kagi.update(cx, |app, cx| app.open_commit_panel(window, cx));
                }

                // T-BP-007: KAGI_TERMINAL=1 (requires KAGI_BOTTOM_PANEL=1) starts
                // the terminal session now that a Window context is available.
                if std::env::var("KAGI_TERMINAL").as_deref() == Ok("1") {
                    kagi.update(cx, |app, cx| app.ensure_terminal(window, cx));
                }

                // W4-TABS / ADR-0027: arm the .git watcher for the initial tab
                // (if any) using the generation scheme.  Subsequent switch/open/
                // close re-arm it from within the entity context.
                kagi.update(cx, |app, cx| {
                    if app.repo_path.is_some() {
                        app.arm_watcher(cx);
                    }
                });

                cx.new(|cx| gpui_component::Root::new(kagi, window, cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
