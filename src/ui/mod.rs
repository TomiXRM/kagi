//! UI module — T008: GPUI commit list / T009: commit graph lane / T010: commit selection + detail panel / T011: changed files list / T012: file diff viewer / T013: checkout plan modal + sidebar / T023: pane resize / T-BP-002: bottom panel open/close + resize / T-BP-007: terminal
//!
//! This module lives in the binary crate (`main.rs` does `mod ui;`).
//! It must not be added to `src/lib.rs` so that domain tests stay
//! independent of GPUI.

pub mod avatar;
pub mod commit_list;
pub mod commit_panel;
pub mod detail_panel;
pub mod file_tree;
pub mod graph_view;
pub mod terminal;
pub mod watcher;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use gpui::{
    App, Context, Entity, FocusHandle, KeyDownEvent, KeyBinding, SharedString, Window,
    UniformListScrollHandle, ScrollStrategy,
    actions, div, prelude::*, px, rgb, uniform_list,
};
use gpui_component::input::{Input, InputState};
use gpui_component::Sizable as _;

// ──────────────────────────────────────────────────────────────
// T-BP-002: Bottom Panel — action + tab enum
// ──────────────────────────────────────────────────────────────

// cmd-j toggle action for the bottom panel.
actions!(kagi, [ToggleBottomPanel]);

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
const BOTTOM_PANEL_DEFAULT_H: f32 = 220.0;
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

// Width of the inner divider handles (badge|graph and graph|message).
const INNER_DIV_W: f32 = 4.0;

use kagi::git::{
    ChangeKind, CommitId, FileDiff, DiffLineKind, FileStatus, Head, RepoSnapshot, Stash,
    ops::{
        OperationPlan, StateSummary,
        execute_checkout, execute_create_branch, plan_checkout, plan_create_branch, preflight_check,
        plan_stash_push, execute_stash_push,
        plan_stash_apply, execute_stash_apply,
        plan_pull, execute_pull, PullOutcome,
        plan_undo_commit, execute_undo_commit,
        plan_stash_pop, execute_stash_pop,
        plan_push, execute_push,
        preflight_check_stash,
        plan_cherry_pick, execute_cherry_pick,
    },
    oplog::{OpLogEntry, OpOutcome, append_oplog, read_oplog_tail},
    stage_file, unstage_file, plan_commit, execute_commit,
};
use commit_panel::{CommitPanelState, CommitPanelFileRef, CommitPlanModal, status_badge};
use commit_list::{BadgeKind, CommitRow, build_commit_rows};
use detail_panel::{CommitDetail, build_commit_details};
use graph_view::graph_canvas;

// ──────────────────────────────────────────────────────────────
// Catppuccin Mocha palette (subset)
// ──────────────────────────────────────────────────────────────
const BG_BASE: u32 = 0x1e1e2e;
const BG_SURFACE: u32 = 0x313244;
const BG_SELECTED: u32 = 0x45475a; // surface1 — selected row highlight
const BG_PANEL: u32 = 0x181825;    // mantle — detail panel background
const TEXT_MAIN: u32 = 0xcdd6f4;
const TEXT_SUB: u32 = 0xa6adc8;
const TEXT_MUTED: u32 = 0x585b70;
const TEXT_LABEL: u32 = 0x6c7086; // overlay0 — field labels in detail panel
const COLOR_HEAD: u32 = 0xf38ba8; // red  — HEAD / attached branch
const COLOR_BRANCH: u32 = 0x89b4fa; // blue — local branch
const COLOR_REMOTE: u32 = 0xa6e3a1; // green — remote branch
const COLOR_TAG: u32 = 0xfab387; // peach — tag

// Diff display colours
const BG_DIFF_ADDED: u32 = 0x1c3a2a;   // dark green background for added lines
const BG_DIFF_REMOVED: u32 = 0x3a1c1c; // dark red background for removed lines
const COLOR_DIFF_HUNK: u32 = 0x89b4fa; // blue — hunk header

// Sidebar / modal colours (T013)
const BG_SIDEBAR: u32 = 0x11111b;       // crust — sidebar background
const COLOR_WARNING: u32 = 0xf9e2af;    // yellow — warning text
const COLOR_BLOCKER: u32 = 0xf38ba8;    // red — blocker text
const COLOR_SUCCESS: u32 = 0xa6e3a1;    // green — success / checked-out mark
const BG_MODAL_OVERLAY: u32 = 0x000000; // semi-transparent overlay (set opacity in render)
const BG_MODAL: u32 = 0x313244;         // surface0 — modal background

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
}

impl StatusBarSummary {
    /// Build from a [`RepoSnapshot`] at the current wall clock time.
    pub fn from_snapshot(snap: &kagi::git::RepoSnapshot) -> Self {
        use kagi::git::Head;
        use commit_list::now_unix_secs;

        let (branch, ahead, behind, no_upstream, is_detached, is_unborn) = match &snap.head {
            Head::Attached { branch, .. } => {
                // Look up upstream info for this branch.
                let upstream = snap
                    .branches
                    .iter()
                    .find(|b| &b.name == branch)
                    .and_then(|b| b.upstream.as_ref());
                match upstream {
                    Some(u) => (branch.clone(), Some(u.ahead), Some(u.behind), false, false, false),
                    None => (branch.clone(), None, None, true, false, false),
                }
            }
            Head::Detached { target } => {
                let short = target.get(..8).unwrap_or(target).to_string();
                (format!("detached HEAD ({})", short), None, None, false, true, false)
            }
            Head::Unborn { branch } => {
                (format!("no commits yet ({})", branch), None, None, false, false, true)
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
        }
    }

    /// Emit the headless verification log line required by T-BP-003.
    ///
    /// Format: `[kagi] statusbar: <branch> ↑A ↓B staged=N unstaged=M`
    pub fn log_headless(&self) {
        let ahead = self.ahead.unwrap_or(0);
        let behind = self.behind.unwrap_or(0);
        eprintln!(
            "[kagi] statusbar: {} \u{2191}{} \u{2193}{} staged={} unstaged={}",
            self.branch, ahead, behind, self.staged, self.unstaged
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

        let pull_on = !no_upstream; // Pull: disabled if no upstream (incl. detached/unborn)
        // Push: enabled when (upstream && ahead>0) OR (no-upstream && attached && remote exists).
        // Dirty WT is irrelevant — push never changes local state.
        let push_on = (!no_upstream && self.ahead.unwrap_or(0) > 0)
            || (self.no_upstream && !not_attached && self.has_remote);
        let stash_on = self.is_dirty; // Stash: disabled if working tree is clean
        let pop_on = self.stash_count > 0; // Pop: disabled if no stashes
        let undo_on = !not_attached && self.ahead.unwrap_or(0) > 0; // disabled if detached/unborn or ahead=0

        ToolbarState {
            pull_on,
            push_on,
            stash_on,
            pop_on,
            undo_on,
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
}

impl ToolbarState {
    /// Emit the headless toolbar log line required by T-HT-001.
    ///
    /// Format: `[kagi] toolbar: pull=on/off push=on/off stash=on/off pop=on/off undo=on/off`
    pub fn log_headless(&self) {
        eprintln!(
            "[kagi] toolbar: pull={} push={} stash={} pop={} undo={}",
            if self.pull_on { "on" } else { "off" },
            if self.push_on { "on" } else { "off" },
            if self.stash_on { "on" } else { "off" },
            if self.pop_on { "on" } else { "off" },
            if self.undo_on { "on" } else { "off" },
        );
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
}

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
// CheckoutPlanModal — state for the plan confirmation overlay (T013)
// ──────────────────────────────────────────────────────────────

/// State for an in-progress checkout plan confirmation.
#[derive(Clone)]
pub struct CheckoutPlanModal {
    /// The computed plan (title, current, predicted, warnings, blockers, recovery).
    pub plan: std::sync::Arc<OperationPlan>,
    /// Error message to show if execute or preflight failed (replaces normal buttons).
    pub error: Option<SharedString>,
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
    /// Current text in the branch-name input field.
    pub input: String,
    /// Live plan (re-generated each keystroke from `input` and `at`).
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
    pub input: String,
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
    /// When `Some`, the detail panel shows the diff for this file instead of
    /// the commit metadata + changed-files list.  Cleared whenever
    /// `selected` changes.
    pub file_diff_view: Option<FileDiffView>,
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
    // ── T-BP-007: Terminal session ───────────────────────────────
    /// Lazy terminal session.  `None` until `repo_path` is known and the
    /// Terminal tab is first displayed (or KAGI_TERMINAL=1 at startup).
    pub terminal_session: Option<terminal::KagiTerminalSession>,
}

impl KagiApp {
    /// Construct from a successful [`RepoSnapshot`].
    pub fn from_snapshot(repo_name: &str, snap: &RepoSnapshot) -> Self {
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

        // T-BP-003: build StatusBarSummary and emit the headless log.
        let mut status_summary = StatusBarSummary::from_snapshot(snap);
        // T-HT-001: fill repo_name for toolbar display.
        status_summary.repo_name = repo_name.to_string();
        status_summary.log_headless();

        // T-HT-001: derive toolbar state and emit headless log.
        let toolbar_state = status_summary.toolbar_state();
        toolbar_state.log_headless();

        // T-BP-004: load up to 100 entries from the oplog file at startup.
        let op_entries: VecDeque<OpLogEntry> = read_oplog_tail(OP_ENTRIES_LOAD).into();

        KagiApp {
            root_focus: None,
            header,
            rows,
            details,
            selected: None,
            error: None,
            repo_path: None,
            diff_cache: HashMap::new(),
            file_diff_view: None,
            branches,
            plan_modal: None,
            pull_modal: None,
            undo_modal: None,
            pop_modal: None,
            push_modal: None,
            create_branch_modal: None,
            modal_focus: None,
            stashes,
            is_dirty,
            stash_push_modal: None,
            stash_apply_modal: None,
            stash_push_focus: None,
            cherry_pick_modal: None,
            status_footer: FooterStatus::Idle(SharedString::from("Ready")),
            sidebar_width: SIDEBAR_DEFAULT,
            panel_width: PANEL_DEFAULT,
            badge_col_w: BADGE_COL_DEFAULT,
            graph_col_w: GRAPH_COL_DEFAULT,
            bottom_panel_open: false,
            bottom_panel_height: BOTTOM_PANEL_DEFAULT_H,
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
            terminal_session: None,
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
            file_diff_view: None,
            branches: Vec::new(),
            plan_modal: None,
            pull_modal: None,
            undo_modal: None,
            pop_modal: None,
            push_modal: None,
            create_branch_modal: None,
            modal_focus: None,
            stashes: Vec::new(),
            is_dirty: false,
            stash_push_modal: None,
            stash_apply_modal: None,
            stash_push_focus: None,
            cherry_pick_modal: None,
            status_footer: FooterStatus::Idle(SharedString::from("Ready")),
            sidebar_width: SIDEBAR_DEFAULT,
            panel_width: PANEL_DEFAULT,
            badge_col_w: BADGE_COL_DEFAULT,
            graph_col_w: GRAPH_COL_DEFAULT,
            bottom_panel_open: false,
            bottom_panel_height: BOTTOM_PANEL_DEFAULT_H,
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
            terminal_session: None,
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

        // Rebuild display data in-place.
        let fresh = KagiApp::from_snapshot(&repo_name, &snap);
        self.header = fresh.header;
        self.rows = fresh.rows;
        self.details = fresh.details;
        self.branches = fresh.branches;
        self.selected = None;
        self.diff_cache = HashMap::new();
        self.file_diff_view = None;
        self.plan_modal = None;
        self.pull_modal = None;
        self.undo_modal = None;
        self.pop_modal = None;
        self.create_branch_modal = None;
        self.modal_focus = None;
        self.stashes = fresh.stashes;
        self.is_dirty = fresh.is_dirty;
        self.stash_push_modal = None;
        self.stash_apply_modal = None;
        self.stash_push_focus = None;
        self.cherry_pick_modal = None;
        // T025/T026: reset commit panel and input so it reflects fresh status after reload.
        self.commit_panel_open = false;
        self.commit_panel = None;
        self.commit_input = None;
        // T028: refresh branch/commit lookup maps so jump works after checkout.
        self.branch_targets = fresh.branch_targets;
        self.commit_row_index = fresh.commit_row_index;
        // T-BP-003: update StatusBarSummary (already logged by from_snapshot above).
        self.status_summary = fresh.status_summary;
        // T-HT-001: update ToolbarState (already logged by from_snapshot above).
        self.toolbar_state = fresh.toolbar_state;
        // commit_scroll_handle is preserved so the existing Rc<RefCell<...>> reference
        // wired into the uniform_list continues to work after reload.
        // status_footer is intentionally preserved across reloads so the last
        // operation result remains visible after the commit list refreshes.
        // sidebar_width / panel_width are also preserved so the user's resize
        // is not lost on checkout/reload (T023).
        // T-BP-004: op_entries, oplog_scroll_handle, oplog_expanded are preserved
        // so the Operation Log keeps its contents across repository reloads.
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
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                eprintln!("[kagi] plan: error: {}", e);
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
        self.create_branch_modal = Some(CreateBranchModal {
            at,
            input: String::new(),
            plan: None,
            error: None,
        });
        // Re-plan immediately (empty name → blocker).
        self.replan_create_branch();
    }

    /// Close the create-branch modal without making any changes.
    pub fn cancel_create_branch_modal(&mut self) {
        self.create_branch_modal = None;
    }

    /// Handle a key-down event for the create-branch name input.
    ///
    /// Accepted characters: ASCII alphanumeric, `-`, `_`, `/`, `.`.
    /// `backspace` removes the last character.
    /// All other keys (including modifier combos) are ignored.
    pub fn handle_create_branch_key(&mut self, event: &KeyDownEvent) {
        let modal = match self.create_branch_modal.as_mut() {
            Some(m) => m,
            None => return,
        };
        let key = &event.keystroke.key;
        let modifiers = &event.keystroke.modifiers;

        // Ignore any modifier combos (cmd/ctrl/alt).
        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }

        if key == "backspace" {
            modal.input.pop();
        } else if key.len() == 1 {
            let ch = key.chars().next().unwrap();
            // Allow: a-z A-Z 0-9 - _ / .
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '/' || ch == '.' {
                modal.input.push(ch);
            }
        }
        modal.error = None;
        self.replan_create_branch();
    }

    /// Re-generate the live plan from the current modal input.
    fn replan_create_branch(&mut self) {
        let (at, name) = match self.create_branch_modal.as_ref() {
            Some(m) => (m.at.clone(), m.input.clone()),
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
        match plan_create_branch(&repo, &name, &at) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: create-branch '{}' blockers={} warnings={}",
                    name,
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

        // Record success to oplog + update footer.
        self.record_op(
            "create-branch",
            plan.current.clone(),
            OpOutcome::Success { after: plan.predicted.clone() },
            &repo_path,
        );

        // Reload display data (new branch badge should appear).
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
            plan: None,
            error: None,
        });
        self.replan_stash_push();
    }

    /// Close the stash push modal without making any changes.
    pub fn cancel_stash_push_modal(&mut self) {
        self.stash_push_modal = None;
    }

    /// Handle a key-down event for the stash push message input.
    pub fn handle_stash_push_key(&mut self, event: &KeyDownEvent) {
        let modal = match self.stash_push_modal.as_mut() {
            Some(m) => m,
            None => return,
        };
        let key = &event.keystroke.key;
        let modifiers = &event.keystroke.modifiers;

        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }

        if key == "backspace" {
            modal.input.pop();
        } else if key == "space" {
            modal.input.push(' ');
        } else if key.len() == 1 {
            let ch = key.chars().next().unwrap();
            if !ch.is_control() {
                modal.input.push(ch);
            }
        }
        modal.error = None;
        self.replan_stash_push();
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

    // ── Oplog + footer helper (T017) ────────────────────────

    /// Record an operation to the oplog and update the status footer.
    ///
    /// Write failures are non-fatal: they emit a stderr warning only.
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
        // Initialise the session container if we haven't yet.
        if self.terminal_session.is_none() {
            if let Some(ref rp) = self.repo_path.clone() {
                self.terminal_session =
                    Some(terminal::KagiTerminalSession::new(rp.clone()));
            } else {
                eprintln!("[kagi] terminal: no repo_path — cannot start terminal");
                return;
            }
        }

        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        // Mutably borrow session; we need to split the borrow to call record_op.
        // Collect the failure message first (if any) then record it after.
        let session = self.terminal_session.as_mut().expect("just set above");

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
        let branch = match modal.plan.predicted.head.strip_prefix("branch: ") {
            Some(b) => b.to_string(),
            None => {
                self.plan_modal = Some(CheckoutPlanModal {
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from("Internal error: could not determine target branch.")),
                });
                return;
            }
        };

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e.message());
                self.record_op(
                    "checkout",
                    modal.plan.current.clone(),
                    OpOutcome::Failed { error: err_msg.clone() },
                    &repo_path,
                );
                self.plan_modal = Some(CheckoutPlanModal {
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
                "checkout",
                modal.plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            self.plan_modal = Some(CheckoutPlanModal {
                plan: modal.plan.clone(),
                error: Some(SharedString::from(err_msg)),
            });
            return;
        }

        // Execute checkout (safe mode only).
        if let Err(e) = execute_checkout(&repo, &branch) {
            let err_msg = format!("Checkout failed: {}", e);
            self.record_op(
                "checkout",
                modal.plan.current.clone(),
                OpOutcome::Failed { error: err_msg.clone() },
                &repo_path,
            );
            self.plan_modal = Some(CheckoutPlanModal {
                plan: modal.plan.clone(),
                error: Some(SharedString::from(err_msg)),
            });
            return;
        }

        eprintln!("[kagi] executed: checkout {}", branch);

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
                match &snap.head {
                    Head::Attached { branch: actual_branch, .. } if actual_branch == &branch => {
                        eprintln!("[kagi] verified: HEAD={}", actual_branch);
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
            "checkout",
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

    /// Confirm the pull plan: preflight, fetch via CLI, then FF / in-memory
    /// merge (see `execute_pull`).  Mirrors `confirm_checkout`'s pipeline.
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

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e.message());
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
                return;
            }
        };

        if let Err(e) = preflight_check(&repo, &modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
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
            return;
        }

        match execute_pull(&repo, &repo_path) {
            Ok(outcome) => {
                let summary = match &outcome {
                    PullOutcome::UpToDate => "already up to date".to_string(),
                    PullOutcome::FastForward { to } => format!("fast-forward to {}", to.short()),
                    PullOutcome::Merged { commit } => format!("merge commit {}", commit.short()),
                };
                eprintln!("[kagi] executed: pull — {}", summary);
                self.pull_modal = None;

                // Verify: re-snapshot for the after-state.
                let after_summary = match git2::Repository::open(&repo_path) {
                    Ok(mut repo2) => match kagi::git::snapshot(&mut repo2, 10_000) {
                        Ok(snap) => StateSummary {
                            head: snap.head.display(),
                            dirty: if snap.status.is_dirty() {
                                "dirty".to_string()
                            } else {
                                "clean".to_string()
                            },
                        },
                        Err(_) => modal.plan.predicted.clone(),
                    },
                    Err(_) => modal.plan.predicted.clone(),
                };
                eprintln!("[kagi] verified: pull after = {}", after_summary.head);

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
            Err(e) => {
                let err_msg = format!("Pull failed: {}", e);
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

    // ── T-HT-004: Push ────────────────────────────────────────

    /// Build a push plan and open the confirmation modal.
    pub fn open_push_modal(&mut self) {
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

    /// Confirm the push plan: preflight, execute push via CLI.
    /// Mirrors `confirm_pull`'s pipeline.
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

        let repo = match git2::Repository::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e.message());
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
                return;
            }
        };

        if let Err(e) = preflight_check(&repo, &modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
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
            return;
        }

        match execute_push(&repo, &repo_path) {
            Ok(outcome) => {
                let summary = if outcome.set_upstream {
                    format!("pushed {} commit(s), set upstream", outcome.pushed)
                } else {
                    format!("pushed {} commit(s)", outcome.pushed)
                };
                eprintln!("[kagi] executed: push — {}", summary);
                self.push_modal = None;

                // Verify: re-snapshot for the after-state.
                let after_summary = match git2::Repository::open(&repo_path) {
                    Ok(mut repo2) => match kagi::git::snapshot(&mut repo2, 10_000) {
                        Ok(snap) => StateSummary {
                            head: snap.head.display(),
                            dirty: if snap.status.is_dirty() {
                                "dirty".to_string()
                            } else {
                                "clean".to_string()
                            },
                        },
                        Err(_) => modal.plan.predicted.clone(),
                    },
                    Err(_) => modal.plan.predicted.clone(),
                };
                eprintln!("[kagi] verified: push after = {}", after_summary.head);

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
            Err(e) => {
                let err_msg = format!("Push failed: {}", e);
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
        self.file_diff_view = None;

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

    /// Select a file in the commit panel and load its diff.
    pub fn select_commit_panel_file(&mut self, file_ref: CommitPanelFileRef) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if let Some(ref mut panel) = self.commit_panel {
            panel.load_diff(file_ref, &repo_path);
        }
    }

    /// Toggle tree view in the commit panel.
    pub fn toggle_commit_panel_tree_view(&mut self) {
        if let Some(ref mut panel) = self.commit_panel {
            panel.tree_view = !panel.tree_view;
        }
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
            self.file_diff_view = None;
            return;
        }
        self.selected = Some(index);
        // Clear any open file diff when the commit selection changes.
        self.file_diff_view = None;

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

    /// Open the diff for the file at `file_index` in the currently selected commit.
    ///
    /// Fetches the diff via [`commit_file_diff`] and stores a pre-rendered
    /// [`FileDiffView`] in `self.file_diff_view`.  No-op if no commit is selected.
    pub fn open_file_diff(&mut self, file_index: usize) {
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

                eprintln!(
                    "[kagi] diff: {} hunks={} (+{} -{})",
                    path.display(),
                    hunks,
                    added,
                    removed,
                );

                self.file_diff_view = Some(FileDiffView::from_file_diff(&file_diff, file_index));
            }
            Err(e) => {
                eprintln!("[kagi] diff error: {}", e);
            }
        }
    }

    /// Close the current file diff view and return to the changed-files list.
    pub fn close_file_diff(&mut self) {
        self.file_diff_view = None;
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
        self.select(row_ix);
    }
}

impl Render for KagiApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row_count = self.rows.len();
        let selected = self.selected;

        if let Some(err) = &self.error {
            // ── Error / usage state ──────────────────────────
            let err = err.clone();
            return div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .size_full()
                .bg(rgb(BG_BASE))
                .child(
                    div()
                        .text_xl()
                        .text_color(rgb(TEXT_MAIN))
                        .child(err),
                )
                .into_any();
        }

        // ── Pre-fetch detail for panel (if any row is selected) ─
        let detail = selected.and_then(|i| self.details.get(i)).cloned();
        // Clone cached changed-files list for the render closure.
        // `None` outer = no selection; `Some(None)` = diff unavailable; `Some(Some(v))` = files.
        let changed_files: Option<Option<Vec<FileStatus>>> = selected
            .map(|i| self.diff_cache.get(&i).cloned().unwrap_or(None));

        // Clone the file diff view if present.
        let file_diff_view = self.file_diff_view.clone();

        // Clone branch list and modal state for render.
        let branches = self.branches.clone();
        let stashes = self.stashes.clone();
        let is_dirty = self.is_dirty;
        let plan_modal = self.plan_modal.clone();
        let pull_modal = self.pull_modal.clone();
        let undo_modal = self.undo_modal.clone();
        let pop_modal = self.pop_modal.clone();
        let push_modal = self.push_modal.clone();
        let create_branch_modal = self.create_branch_modal.clone();
        let modal_focus = self.modal_focus.clone();
        let stash_push_modal = self.stash_push_modal.clone();
        let stash_push_focus = self.stash_push_focus.clone();
        let stash_apply_modal = self.stash_apply_modal.clone();
        let cherry_pick_modal = self.cherry_pick_modal.clone();
        let status_footer = self.status_footer.clone();
        // T-HT-001: clone toolbar/summary state for header render.
        let toolbar_state = self.toolbar_state.clone();
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

        // ── Normal state: header + body + bottom panel slot + status bar ─────
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG_BASE))
            // Key events only dispatch along the focus path, so the root must
            // own (and initially hold) focus for window-wide actions to work.
            .when_some(self.root_focus.clone(), |el, fh| el.track_focus(&fh))
            // T023: capture drag-move for both dividers on the root element.
            .on_drag_move::<DividerDrag>(divider_drag_move)
            // T-BP-002: cmd-j toggle action (window-wide via on_action on root div).
            .on_action(toggle_bottom_panel)
            // ── Header slot ──────────────────────────────────
            .child(self.render_header_slot(toolbar_state, status_summary, cx))
            // ── Body slot: sidebar | list | optional panel ───
            .child(self.render_body(
                row_count, selected, detail, changed_files, file_diff_view,
                branches, stashes, is_dirty, sidebar_width, panel_width,
                badge_col_w, graph_col_w, commit_scroll_handle,
                commit_panel_open, commit_panel.clone(), commit_input.clone(),
                cx,
            ))
            // ── Bottom panel slot (T-BP-002) ─────────────────
            .children(self.render_bottom_panel_slot(bottom_panel_open, bottom_panel_height, bottom_tab, cx))
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
                el.child(render_create_branch_modal(modal, modal_focus, cx))
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
            .into_any()
    }

}

// ── AppShell layout slots ────────────────────────────────────────────────────
// ADR-0007 / T-BP-001: KagiApp::render is decomposed into four vertical
// flex slots.  Each slot is a plain method so that later tickets
// (T-BP-002, T-HT-001, …) can extend their signatures without
// touching the caller site.
impl KagiApp {
    /// Header slot — the Toolbar bar (T-HT-001).
    ///
    /// Layout (34 px):  repo-name | Pull Push | Branch Stash Pop | Undo | Refresh  [right→] branch ↑A ↓B
    fn render_header_slot(
        &mut self,
        toolbar: ToolbarState,
        summary: StatusBarSummary,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // ── Click handlers ──────────────────────────────────────────────────
        // Pull (not implemented yet — footer notice only).
        let pull_on = toolbar.pull_on;
        let pull_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if pull_on {
                this.open_pull_modal();
            } else {
                let reason = if this.status_summary.is_detached {
                    "Pull: detached HEAD — branch に切り替えてください"
                } else if this.status_summary.is_unborn {
                    "Pull: no commits yet — upstream がありません"
                } else {
                    "Pull: upstream が設定されていません (no upstream)"
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
                let reason = if this.status_summary.is_detached {
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
            cx.notify();
        });

        // ── Helper: build a single toolbar button ───────────────────────────
        // `id` must be a unique string for GPUI element tracking.
        // `enabled` controls opacity; `label` is the button text.
        let make_btn = |id: &'static str,
                        label: &'static str,
                        icon: gpui_component::IconName,
                        enabled: bool| {
            let text_color = if enabled { TEXT_MAIN } else { TEXT_MUTED };
            let bg_color = if enabled { BG_SELECTED } else { BG_SURFACE };
            div()
                .id(id)
                .px_2()
                .py_px()
                .rounded_sm()
                .bg(rgb(bg_color))
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .text_sm()
                .text_color(rgb(text_color))
                .hover(|style| style.opacity(if enabled { 0.85 } else { 0.6 }))
                .cursor(if enabled { gpui::CursorStyle::PointingHand } else { gpui::CursorStyle::Arrow })
                .child(
                    gpui_component::Icon::new(icon)
                        .with_size(gpui_component::Size::XSmall)
                        .text_color(rgb(text_color)),
                )
                .child(SharedString::from(label))
        };

        // ── Ahead/behind display (right side) ──────────────────────────────
        let ab_label = if summary.is_detached {
            "detached HEAD".to_string()
        } else if summary.is_unborn {
            "no commits yet".to_string()
        } else if summary.no_upstream {
            "no upstream".to_string()
        } else {
            let ahead = summary.ahead.unwrap_or(0);
            let behind = summary.behind.unwrap_or(0);
            format!("{} \u{2191}{} \u{2193}{}", summary.branch, ahead, behind)
        };

        // ── Vertical separator ──────────────────────────────────────────────
        let sep = || {
            div()
                .w(px(1.0))
                .h(px(16.0))
                .bg(rgb(TEXT_MUTED))
                .mx_1()
                .flex_shrink_0()
        };

        // ── Toolbar bar (34 px) ─────────────────────────────────────────────
        div()
            .id("toolbar-bar")
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .px_3()
            .h(px(34.0))
            .flex_shrink_0()
            .bg(rgb(BG_SURFACE))
            .text_color(rgb(TEXT_SUB))
            // Repo name (left anchor)
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(TEXT_MAIN))
                    .font_weight(gpui::FontWeight::BOLD)
                    .mr_2()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .child(SharedString::from(summary.repo_name.clone())),
            )
            .child(sep())
            // Pull
            .child(
                make_btn("tb-pull", "Pull", gpui_component::IconName::ArrowDown, toolbar.pull_on)
                    .on_click(pull_click),
            )
            .child(div().w(px(4.0)))
            // Push
            .child(
                make_btn("tb-push", "Push", gpui_component::IconName::ArrowUp, toolbar.push_on)
                    .on_click(push_click),
            )
            .child(sep())
            // Branch
            .child(
                make_btn("tb-branch", "Branch", gpui_component::IconName::Plus, true)
                    .on_click(branch_click),
            )
            .child(div().w(px(4.0)))
            // Stash
            .child(
                make_btn("tb-stash", "Stash", gpui_component::IconName::Inbox, toolbar.stash_on)
                    .on_click(stash_click),
            )
            .child(div().w(px(4.0)))
            // Pop
            .child(
                make_btn("tb-pop", "Pop", gpui_component::IconName::FolderOpen, toolbar.pop_on)
                    .on_click(pop_click),
            )
            .child(sep())
            // Undo
            .child(
                make_btn("tb-undo", "Undo", gpui_component::IconName::Undo2, toolbar.undo_on)
                    .on_click(undo_click),
            )
            .child(sep())
            // Refresh
            .child(
                make_btn("tb-refresh", "Refresh", gpui_component::IconName::LoaderCircle, true)
                    .on_click(refresh_click),
            )
            // Spacer — pushes ahead/behind to the right
            .child(div().flex_1())
            // Branch + ahead/behind (right anchor)
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(TEXT_SUB))
                    .flex_shrink_0()
                    .child(SharedString::from(ab_label)),
            )
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
        file_diff_view: Option<FileDiffView>,
        branches: Vec<(String, bool)>,
        stashes: Vec<kagi::git::Stash>,
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
        // Build divider 1: sidebar | main.
        let divider1 = div()
            .id("divider-sidebar")
            .w(px(4.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(BG_SURFACE))
            .hover(|style| style.bg(rgb(COLOR_BRANCH)).cursor_col_resize())
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
        let wip_bg = if commit_panel_open { BG_SELECTED } else { 0x2a2a3a };

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
            .bg(rgb(BG_SURFACE))
            // Badge column label
            .child(
                div()
                    .w(px(badge_col_w))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .text_xs()
                    .text_color(rgb(TEXT_MUTED))
                    .child(SharedString::from("BRANCH / TAG")),
            )
            // Handle between badge and graph columns
            .child(
                div()
                    .id("divider-badge-col")
                    .w(px(INNER_DIV_W))
                    .flex_shrink_0()
                    .h_full()
                    .bg(rgb(BG_SURFACE))
                    .hover(|style| style.bg(rgb(COLOR_BRANCH)).cursor_col_resize())
                    .cursor_col_resize()
                    .on_drag(
                        DividerDrag { kind: DividerKind::BadgeCol },
                        |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
                    ),
            )
            // Graph column label
            .child(
                div()
                    .w(px(graph_col_w))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(rgb(TEXT_MUTED))
                    .child(SharedString::from("GRAPH")),
            )
            // Handle between graph and message columns
            .child(
                div()
                    .id("divider-graph-col")
                    .w(px(INNER_DIV_W))
                    .flex_shrink_0()
                    .h_full()
                    .bg(rgb(BG_SURFACE))
                    .hover(|style| style.bg(rgb(COLOR_BRANCH)).cursor_col_resize())
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
                    .text_color(rgb(TEXT_MUTED))
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
                        .h(px(graph_view::ROW_H))
                        .bg(rgb(wip_bg))
                        .on_click(wip_click)
                        .hover(|s| s.bg(rgb(BG_SELECTED)))
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
                                        .bg(rgb(COLOR_WARNING))
                                        .text_color(rgb(BG_BASE))
                                        .text_sm()
                                        .flex_shrink_0()
                                        .child(SharedString::from("WIP")),
                                ),
                        )
                        // Inner divider spacer (badge|graph handle width)
                        .child(div().w(px(INNER_DIV_W)).flex_shrink_0())
                        // Graph column placeholder (empty for WIP row)
                        .child(
                            div()
                                .w(px(graph_col_w))
                                .flex_shrink_0(),
                        )
                        // Inner divider spacer (graph|message handle width)
                        .child(div().w(px(INNER_DIV_W)).flex_shrink_0())
                        // Summary area: "// WIP — N changes"
                        .child(
                            div()
                                .flex_1()
                                .text_color(rgb(TEXT_MUTED))
                                .overflow_hidden()
                                .child(SharedString::from("// WIP")),
                        ),
                )
            })
            // ── Virtualized commit list ──────────────
            .child(
                uniform_list(
                    "commit-list",
                    row_count,
                    cx.processor(move |this, range, _window, cx| {
                        render_rows(&this.rows, range, selected, this.badge_col_w, this.graph_col_w, cx)
                    }),
                )
                // T028: wire scroll handle so jump_to_branch can scroll the list.
                .track_scroll(commit_scroll_handle)
                .flex_1()
                .min_h(px(0.)),
            );

        let mut body_row = div()
            .flex()
            .flex_row()
            .flex_1()
            // min_h(0) — NOT h_full: the body must be able to shrink below its
            // natural content height, otherwise it pushes the bottom panel and
            // status bar out of the window on small window sizes (user report).
            .min_h(px(0.))
            // ── Left sidebar ──────────────────────────
            .child(render_sidebar(&branches, &stashes, sidebar_width, cx))
            // ── Sidebar divider ───────────────────────
            .child(divider1)
            // ── Commit list column (WIP row + virtualized list) ──
            .child(commit_list_col);

        // ── Right panel: commit panel OR detail panel ───────────
        // Build divider 2 (shared between both panel modes).
        let divider2 = div()
            .id("divider-panel")
            .w(px(4.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(BG_SURFACE))
            .hover(|style| style.bg(rgb(COLOR_BRANCH)).cursor_col_resize())
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
                    .child(render_commit_panel(panel_state, panel_width, commit_input.clone(), cx));
            }
        } else {
            // ── Normal commit detail panel (existing behaviour) ──
            body_row = body_row.when_some(detail, |el, d| {
                if let Some(diff_view) = file_diff_view {
                    // ── Diff view mode ──────────────────
                    el.child(divider2)
                        .child(render_diff_panel(diff_view, panel_width, cx))
                } else {
                    // ── Commit metadata + changed files ─
                    let at = CommitId(d.full_sha.as_ref().to_string());
                    let files = changed_files.clone();
                    let files_for_click = changed_files.clone();
                    el.child(divider2)
                        .child(render_detail_panel(d, at, files.unwrap_or(None), files_for_click.unwrap_or(None), panel_width, cx))
                }
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
            .bg(rgb(BG_SURFACE))
            .hover(|style| style.bg(rgb(COLOR_BRANCH)).cursor_row_resize())
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
                let text_color = if is_active { TEXT_MAIN } else { TEXT_MUTED };
                let bg_color = if is_active { BG_SELECTED } else { BG_PANEL };
                div()
                    .px_3()
                    .h(px(BOTTOM_PANEL_TAB_H))
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .bg(rgb(bg_color))
                    .text_sm()
                    .text_color(rgb(text_color))
                    .hover(|s| s.bg(rgb(BG_SURFACE)))
                    .child(SharedString::from(label))
            };

            div()
                .id("bottom-panel-tab-bar")
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .flex_shrink_0()
                .bg(rgb(BG_PANEL))
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
                .bg(rgb(BG_PANEL))
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(rgb(TEXT_MUTED))
                .child(SharedString::from("No operations yet"))
                .into_any();
        }

        let scroll_handle = self.oplog_scroll_handle.clone();

        uniform_list(
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
                                COLOR_SUCCESS,
                            ),
                            OpOutcome::Failed { error } => (
                                SharedString::from(format!("Failed: {}", error)),
                                COLOR_BLOCKER,
                            ),
                            OpOutcome::Refused { blockers } => (
                                SharedString::from(format!(
                                    "Refused ({} blocker{})",
                                    blockers.len(),
                                    if blockers.len() == 1 { "" } else { "s" }
                                )),
                                COLOR_WARNING,
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

                        let row_bg = if i % 2 == 0 { BG_PANEL } else { BG_BASE };

                        // Summary row.
                        let mut row_div = div()
                            .id(("oplog-row", i))
                            .flex()
                            .flex_col()
                            .w_full()
                            .bg(rgb(row_bg))
                            .hover(|s| s.bg(rgb(BG_SURFACE)).cursor_pointer())
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
                                            .text_color(rgb(TEXT_MUTED))
                                            .child(time_label),
                                    )
                                    .child(
                                        div()
                                            .w(px(100.))
                                            .flex_shrink_0()
                                            .ml(px(6.))
                                            .text_xs()
                                            .text_color(rgb(TEXT_SUB))
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
                                .bg(rgb(BG_SELECTED))
                                .text_xs()
                                .text_color(rgb(TEXT_SUB))
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
        .bg(rgb(BG_PANEL))
        .into_any()
    }

    /// Render the Terminal tab body (T-BP-007).
    ///
    /// Three possible states:
    /// 1. Session running → render `TerminalView` entity directly (flex_1 + min_h).
    /// 2. Session failed to start → show the error message.
    /// 3. Not yet started (session is None, or view is None with no error) →
    ///    show a "starting…" placeholder.  The Terminal tab click listener has
    ///    already called `ensure_terminal`; the view will appear on next repaint.
    fn render_terminal_body(&mut self, _cx: &mut Context<Self>) -> gpui::AnyElement {
        // Case 1: running terminal view.
        if let Some(ref session) = self.terminal_session {
            if let Some(ref view_entity) = session.view {
                return div()
                    .flex_1()
                    .min_h(px(0.))
                    .w_full()
                    .child(view_entity.clone())
                    .into_any();
            }

            // Case 2: start failed — show error.
            if let Some(ref err) = session.start_error {
                let msg = SharedString::from(format!("terminal error: {}", err));
                return div()
                    .flex_1()
                    .min_h(px(0.))
                    .bg(rgb(BG_PANEL))
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(rgb(COLOR_BLOCKER))
                    .child(msg)
                    .into_any();
            }
        }

        // Case 3: placeholder (no session yet / shell exited, will restart).
        div()
            .flex_1()
            .min_h(px(0.))
            .bg(rgb(BG_PANEL))
            .px_3()
            .py_2()
            .text_sm()
            .text_color(rgb(TEXT_MUTED))
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
            FooterStatus::Success(msg) => (COLOR_SUCCESS, msg.clone()),
            FooterStatus::Failed(msg) => (COLOR_BLOCKER, msg.clone()),
            FooterStatus::Idle(msg) => (TEXT_MUTED, msg.clone()),
        };

        // ── Branch label ───────────────────────────────────────
        let branch_text = SharedString::from(summary.branch.clone());

        // ── Dirty bullet ──────────────────────────────────────
        let dirty_chip = if summary.is_dirty {
            Some(
                div()
                    .ml(px(4.))
                    .text_color(rgb(COLOR_WARNING))
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
                    .text_color(rgb(COLOR_SUCCESS))
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
                    .text_color(rgb(COLOR_WARNING))
                    .flex_shrink_0()
                    .child(SharedString::from(format!("~{}", summary.unstaged))),
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
                        .text_color(rgb(TEXT_SUB))
                        .flex_shrink_0()
                        .child(SharedString::from(label)),
                )
            }
            _ if summary.no_upstream => Some(
                div()
                    .ml(px(6.))
                    .text_color(rgb(TEXT_MUTED))
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
                    .text_color(rgb(TEXT_MUTED))
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

        let icon_terminal_color = if terminal_active { TEXT_MAIN } else { TEXT_MUTED };
        let icon_oplog_color = if oplog_active { TEXT_MAIN } else { TEXT_MUTED };

        let icon_terminal = div()
            .id("status-icon-terminal")
            .ml(px(4.))
            .px_1()
            .flex_shrink_0()
            .text_color(rgb(icon_terminal_color))
            .hover(|s| s.text_color(rgb(TEXT_MAIN)).cursor_pointer())
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
            .hover(|s| s.text_color(rgb(TEXT_MAIN)).cursor_pointer())
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
            .bg(rgb(BG_PANEL))
            .text_xs()
            .text_color(rgb(TEXT_MUTED))
            .overflow_hidden()
            // Branch label
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(rgb(TEXT_MAIN))
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
        // Upstream ahead/behind
        if let Some(chip) = upstream_chip {
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
/// `cx` — the `Context<KagiApp>` from the `cx.processor` closure;
///         used to build `cx.listener(...)` for the on_click handler.
fn render_rows(
    rows: &[CommitRow],
    range: std::ops::Range<usize>,
    selected: Option<usize>,
    badge_col_w: f32,
    graph_col_w: f32,
    cx: &mut Context<KagiApp>,
) -> Vec<impl IntoElement> {
    range
        .filter_map(|i| rows.get(i).map(|row| (i, row)))
        .map(|(ix, row)| {
            let row = row.clone();

            // Selected row gets a prominent surface highlight;
            // even/odd stripes apply otherwise.
            let row_bg = if selected == Some(ix) {
                BG_SELECTED
            } else if ix % 2 == 0 {
                BG_BASE
            } else {
                0x1a1a2a
            };

            // ── Graph lane area (T030) ────────────────────────
            // visible_lanes = how many lanes fit in the current graph column width.
            // This replaces the old MAX_LANES-based clipping.
            let visible_lanes = graph_view::lanes_for_width(graph_col_w);

            // on_click handler: update KagiApp.selected via cx.listener.
            let click_handler = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.select(ix);
                cx.notify();
            });

            // ── Avatar (T020) ─────────────────────────────────
            let avatar_color = avatar::avatar_color(&row.author_email);
            let avatar_init = SharedString::from(avatar::avatar_initial(&row.author));
            // Convert Hsla to the rgb u32 that gpui's `bg()` accepts via hsla().
            let av_bg = avatar_color;

            div()
                .id(ix)
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .px_3()
                .h(px(graph_view::ROW_H))
                .bg(rgb(row_bg))
                .on_click(click_handler)
                // ── Badges column: user-resizable width (T030) ──
                .child(render_badges_column(&row.badges, badge_col_w))
                // ── Inner divider spacer (badge|graph handle width) ──
                .child(div().w(px(INNER_DIV_W)).flex_shrink_0())
                // ── Graph lane area (T030) ────────────────────────
                // Always render the graph column at graph_col_w width.
                // Clip by visible_lanes to prevent bleed into message column.
                .child(
                    div()
                        .w(px(graph_col_w))
                        .h_full()
                        .flex_shrink_0()
                        .overflow_hidden()
                        .when(visible_lanes > 0, |el| {
                            el.child(
                                graph_canvas(row.lane, row.edges.clone(), visible_lanes)
                                    .size_full(),
                            )
                        }),
                )
                // ── Inner divider spacer (graph|message handle width) ──
                .child(div().w(px(INNER_DIV_W)).flex_shrink_0())
                // ── Author avatar: 18px circle after graph ────────
                .child(
                    div()
                        .w(px(18.))
                        .h(px(18.))
                        .flex_shrink_0()
                        .mr(px(4.))
                        .rounded_full()
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
                )
                .child(
                    div()
                        .flex_1()
                        .text_color(rgb(TEXT_MAIN))
                        // Single line, no wrapping: long summaries ellipsize
                        // (truncate = overflow_hidden + nowrap + ellipsis).
                        .truncate()
                        .child(row.summary.clone()),
                )
                .child(
                    div()
                        .w(px(130.))
                        .flex_shrink_0()
                        .text_color(rgb(TEXT_SUB))
                        .truncate()
                        .child(row.author.clone()),
                )
                .child(
                    div()
                        .w(px(72.))
                        .flex_shrink_0()
                        .text_color(rgb(TEXT_MUTED))
                        .child(row.date.clone()),
                )
        })
        .collect()
}

// ──────────────────────────────────────────────────────────────
// Detail panel renderer
// ──────────────────────────────────────────────────────────────

/// Render the right-side detail panel showing commit metadata + changed files.
///
/// T022: The metadata area is now vertically scrollable (`overflow_y_scroll()`
/// via `.id("detail-scroll")`).  All text fields use `truncate()` (single-line
/// + ellipsis) except the commit message, which is split on `'\n'` so that
/// each original line is truncated independently (no artificial soft-wrap).
/// Empty message lines are preserved as full-height spacer rows.
///
/// Each changed-file row is clickable: clicking opens the file diff view.
/// A `+ Create branch here` button at the top opens the create-branch modal.
fn render_detail_panel(
    d: CommitDetail,
    at: CommitId,
    changed_files: Option<Vec<FileStatus>>,
    changed_files_for_click: Option<Vec<FileStatus>>,
    panel_width: f32,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // Helper: one labelled field row.  Value is single-line + truncate.
    let field = |label: &'static str, value: SharedString| {
        div()
            .flex()
            .flex_col()
            .mb_2()
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(TEXT_LABEL))
                    .child(SharedString::from(label)),
            )
            .child(
                div()
                    .text_color(rgb(TEXT_MAIN))
                    .truncate()
                    .child(value),
            )
    };

    // Parents section: "none" for root commits, short ids otherwise.
    let parents_value = if d.parent_ids.is_empty() {
        SharedString::from("(root commit)")
    } else {
        SharedString::from(d.parent_ids.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join("  "))
    };

    // Colour constants for change-kind badges (A/M/D/R/T).
    const COLOR_ADDED:   u32 = 0xa6e3a1; // green
    const COLOR_MODIFIED: u32 = 0xf9e2af; // yellow
    const COLOR_DELETED: u32 = 0xf38ba8; // red
    const COLOR_RENAMED: u32 = 0x89b4fa; // blue
    const COLOR_TYPECHANGE: u32 = 0x585b70; // gray (muted)
    const COLOR_DIR: u32 = 0x6c7086; // overlay0 — muted directory label

    const MAX_FILES: usize = 100;

    // Suppress unused warning for changed_files_for_click (kept for symmetry / future use).
    let _ = changed_files_for_click;

    // ── Truncate input files before building the tree (T018 policy) ──────
    let truncated_files: Option<Vec<FileStatus>> = changed_files.as_ref().map(|files| {
        files.iter().take(MAX_FILES).cloned().collect()
    });
    let total_files = changed_files.as_ref().map(|f| f.len()).unwrap_or(0);
    let truncated_count = if total_files > MAX_FILES { Some(total_files - MAX_FILES) } else { None };

    // ── Build tree rows from (truncated) file list ────────────────────────
    let tree_rows = truncated_files.as_ref().map(|files| {
        file_tree::build_file_tree(files)
    });

    // ── Build GPUI element rows for the tree ─────────────────────────────
    let tree_element_rows: Vec<_> = match &tree_rows {
        None => vec![],
        Some(rows) => rows.iter().map(|row| {
            match row {
                file_tree::TreeRow::Dir { depth, name } => {
                    let indent = (*depth as f32) * 12.0;
                    div()
                        .id(SharedString::from(format!("tree-dir-{}", name.as_ref())))
                        .flex()
                        .flex_row()
                        .items_center()
                        .pl(px(indent))
                        .mb_px()
                        .overflow_hidden()
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(COLOR_DIR))
                                .truncate()
                                .child(name.clone()),
                        )
                        .into_any()
                }
                file_tree::TreeRow::File { depth, name, file_index, change } => {
                    let indent = (*depth as f32) * 12.0;
                    let (badge_char, badge_color) = match change {
                        ChangeKind::Added      => ("A", COLOR_ADDED),
                        ChangeKind::Modified   => ("M", COLOR_MODIFIED),
                        ChangeKind::Deleted    => ("D", COLOR_DELETED),
                        ChangeKind::Renamed { .. } => ("R", COLOR_RENAMED),
                        ChangeKind::TypeChange => ("T", COLOR_TYPECHANGE),
                    };
                    let fi = *file_index;
                    let click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                        this.open_file_diff(fi);
                        cx.notify();
                    });
                    div()
                        .id(("file-row", fi))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .pl(px(indent))
                        .mb_px()
                        .on_click(click)
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
                                .text_color(rgb(TEXT_MAIN))
                                .truncate()
                                .child(name.clone()),
                        )
                        .into_any()
                }
            }
        }).collect(),
    };

    // ── "Create branch here" button ──────────────────────────
    let at_for_cherry = at.clone();
    let create_branch_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.open_create_branch_modal(at.clone(), cx);
        cx.notify();
    });

    let create_branch_button = div()
        .id("create-branch-btn")
        .mb_1()
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(rgb(BG_SURFACE))
        .text_sm()
        .text_color(rgb(COLOR_BRANCH))
        .on_click(create_branch_click)
        .hover(|style| style.bg(rgb(BG_SELECTED)))
        .child(SharedString::from("+ Create branch here"));

    // ── "Cherry-pick onto HEAD" button (T016) ────────────────
    let cherry_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.open_cherry_pick_modal(at_for_cherry.clone());
        cx.notify();
    });

    let cherry_pick_button = div()
        .id("cherry-pick-btn")
        .mb_2()
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(rgb(BG_SURFACE))
        .text_sm()
        .text_color(rgb(0xcba6f7)) // Catppuccin mauve — cherry-pick distinct from branch color
        .on_click(cherry_click)
        .hover(|style| style.bg(rgb(BG_SELECTED)))
        .child(SharedString::from("\u{1f352} Cherry-pick onto HEAD branch"));

    // ── Message: split on '\n', each line truncated independently ────────
    // Empty lines are rendered as a full-height spacer (non-breaking space).
    let message_lines: Vec<_> = d.full_message
        .as_ref()
        .split('\n')
        .map(|line| {
            let text = if line.is_empty() {
                // Preserve empty lines as visible spacers.
                SharedString::from("\u{00A0}") // NBSP — gives the row its line height
            } else {
                SharedString::from(line.to_string())
            };
            div()
                .flex()
                .flex_row()
                .w_full()
                .text_color(rgb(TEXT_MAIN))
                .text_sm()
                .truncate()
                .child(text)
                .into_any()
        })
        .collect();

    let files_section = {
        let section_label = match &changed_files {
            None => SharedString::from("Changed files"),
            Some(files) => SharedString::from(format!("Changed files ({})", files.len())),
        };

        let mut section = div()
            .flex()
            .flex_col()
            .mt_2()
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(TEXT_LABEL))
                    .mb_1()
                    .child(section_label),
            );

        if changed_files.is_none() {
            section = section.child(
                div()
                    .text_sm()
                    .text_color(rgb(TEXT_MUTED))
                    .child(SharedString::from("(diff unavailable)")),
            );
        } else {
            for row in tree_element_rows {
                section = section.child(row);
            }
            if let Some(remaining) = truncated_count {
                section = section.child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT_MUTED))
                        .child(SharedString::from(format!("\u{2026} and {} more", remaining))),
                );
            }
        }

        section
    };

    // ── Build the scrollable content block ───────────────────────────────
    // All metadata + file tree goes inside a single scrollable div with `.id()`.
    // The outer panel is `flex_col` + `h_full`; the inner scroll area is `flex_1`
    // with `min_h(px(0.))` so it can shrink below its natural height.
    let mut scroll_content = div()
        .flex()
        .flex_col()
        .px_3()
        .py_2()
        // ── Create branch here button ────────────────────────
        .child(create_branch_button)
        // ── Cherry-pick onto HEAD button (T016) ─────────────
        .child(cherry_pick_button)
        // ── Full SHA — single-line + truncate ────────────────
        .child(field("SHA", d.full_sha))
        // ── Author — single-line + truncate ──────────────────
        .child(field("Author", d.author_line))
        // ── Committer (only when different from author) ──────
        .when_some(d.committer_line, |el, c| el.child(field("Committer", c)))
        // ── Parents — single-line + truncate ─────────────────
        .child(field("Parents", parents_value))
        // ── Message — per-line truncate, no soft-wrap ────────
        .child(
            div()
                .flex()
                .flex_col()
                .mb_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT_LABEL))
                        .mb_1()
                        .child(SharedString::from("Message")),
                ),
        );

    for line_el in message_lines {
        scroll_content = scroll_content.child(line_el);
    }

    scroll_content = scroll_content
        // ── Changed files ─────────────────────────────────
        .child(files_section);

    // ── Outer panel: user-resizable width, full height, flex_col ─────────
    div()
        .w(px(panel_width))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(BG_PANEL))
        // ── Scrollable area (flex_1 + min_h(0) so it can shrink) ─────────
        .child(
            div()
                .id("detail-scroll")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .child(scroll_content),
        )
}

// ──────────────────────────────────────────────────────────────
// Diff panel renderer
// ──────────────────────────────────────────────────────────────

/// Render the diff view panel for a single file.
///
/// Layout:
/// - `← back` row (click to return to the changed-files list)
/// - File name
/// - Virtualized diff line list (`uniform_list` with id `"diff-list"`)
/// T023: `panel_width` replaces the hard-coded 560px diff-view special case.
fn render_diff_panel(view: FileDiffView, panel_width: f32, cx: &mut Context<KagiApp>) -> impl IntoElement {
    let row_count = view.rows.len();
    let rows = std::sync::Arc::new(view.rows);
    let rows_for_list = rows.clone();

    // "← back" click handler: close the diff view.
    let back_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.close_file_diff();
        cx.notify();
    });

    // T022: `min_h(px(0.))` on the uniform_list wrapper is the fix for
    // "file diff not visible" — without it, the flex child does not shrink
    // below its natural height, so the uniform_list overflows the panel and
    // the diff rows are pushed outside the visible area.
    div()
        .w(px(panel_width))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(BG_PANEL))
        .px_0()
        .py_0()
        // ── Back row (fixed height — does NOT participate in flex shrinking) ──
        .child(
            div()
                .id("diff-back")
                .flex()
                .flex_row()
                .items_center()
                .flex_shrink_0()
                .px_3()
                .py_1()
                .bg(rgb(BG_SURFACE))
                .on_click(back_click)
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT_SUB))
                        .child(SharedString::from("\u{2190} back")),
                )
                .child(
                    div()
                        .ml_2()
                        .flex_1()
                        .text_sm()
                        .text_color(rgb(TEXT_MAIN))
                        .truncate()
                        .child(view.file_name),
                ),
        )
        // ── Diff body: flex_1 + min_h(0) ensures it fills remaining space ──
        .child(
            uniform_list(
                "diff-list",
                row_count,
                cx.processor(move |_this, range, _window, _cx| {
                    render_diff_rows(&rows_for_list, range)
                }),
            )
            .flex_1()
            .min_h(px(0.)),
        )
}

/// Render a range of diff rows for the `"diff-list"` uniform_list.
fn render_diff_rows(
    rows: &[DiffRow],
    range: std::ops::Range<usize>,
) -> Vec<impl IntoElement> {
    range
        .filter_map(|i| rows.get(i).map(|row| (i, row)))
        .map(|(i, row)| match row {
            DiffRow::HunkHeader(header) => {
                div()
                    .id(("diff-hunk", i))
                    .w_full()
                    .px_2()
                    .py_px()
                    .bg(rgb(BG_SURFACE))
                    .text_sm()
                    .text_color(rgb(COLOR_DIFF_HUNK))
                    .overflow_hidden()
                    .child(header.clone())
                    .into_any()
            }
            DiffRow::Line { kind, text } => {
                let bg = match kind {
                    DiffLineKind::Added   => BG_DIFF_ADDED,
                    DiffLineKind::Removed => BG_DIFF_REMOVED,
                    DiffLineKind::Context => BG_BASE,
                };
                let text_color = match kind {
                    DiffLineKind::Added   => 0xa6e3a1u32, // green
                    DiffLineKind::Removed => 0xf38ba8u32, // red
                    DiffLineKind::Context => TEXT_MAIN,
                };
                div()
                    .id(("diff-line", i))
                    .w_full()
                    .px_2()
                    .py_px()
                    .bg(rgb(bg))
                    .text_sm()
                    .text_color(rgb(text_color))
                    .overflow_hidden()
                    .child(text.clone())
                    .into_any()
            }
            DiffRow::Binary => {
                div()
                    .id(("diff-binary", i))
                    .w_full()
                    .px_2()
                    .py_1()
                    .text_sm()
                    .text_color(rgb(TEXT_MUTED))
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
            BadgeKind::HeadBranch => COLOR_HEAD,
            BadgeKind::Branch => COLOR_BRANCH,
            BadgeKind::Remote => COLOR_REMOTE,
            BadgeKind::Tag => COLOR_TAG,
        };
        // Char-truncate long labels.
        let label: SharedString = if badge.label.chars().count() > MAX_BADGE_CHARS {
            let s: String = badge.label.chars().take(MAX_BADGE_CHARS - 1).collect();
            SharedString::from(format!("{}\u{2026}", s))
        } else {
            badge.label.clone()
        };
        let is_primary = i == 0;
        let chip = div()
            .px_1()
            .rounded_sm()
            .bg(rgb(color))
            .text_color(rgb(BG_BASE))
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
                    .bg(rgb(BG_SURFACE))
                    .text_color(rgb(TEXT_SUB))
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
        FooterStatus::Success(msg) => (COLOR_SUCCESS, msg.clone()),
        FooterStatus::Failed(msg) => (COLOR_BLOCKER, msg.clone()),
        FooterStatus::Idle(msg) => (TEXT_MUTED, msg.clone()),
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
        .bg(rgb(BG_PANEL))
        .text_xs()
        .text_color(rgb(text_color))
        .overflow_hidden()
        .child(text)
}

// ──────────────────────────────────────────────────────────────
// Sidebar renderer (T013)
// ──────────────────────────────────────────────────────────────

/// Render the left sidebar showing local branches and stash entries.
///
/// - Local branches: clicking the HEAD branch does nothing (already checked out).
///   Clicking any other branch opens the checkout plan modal.
/// - Stash entries: clicking any stash entry opens the stash apply modal.
/// - `width` — the current sidebar width in pixels (T023: user-resizable).
fn render_sidebar(
    branches: &[(String, bool)],
    stashes: &[Stash],
    width: f32,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // Scrollable inner column: every row keeps its natural height
    // (flex_shrink_0) and the column scrolls when content exceeds the
    // sidebar height.  Without this, flex squeezed rows together on small
    // windows (overlapping text) instead of scrolling (user report).
    let mut col = div()
        .id("sidebar-scroll")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .py_2()
        // ── LOCAL BRANCHES label ──────────────────────────
        .child(
            div()
                .px_3()
                .py_1()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(TEXT_MUTED))
                .child(SharedString::from("LOCAL BRANCHES")),
        );

    for (branch_name, is_head) in branches {
        let label = if *is_head {
            SharedString::from(format!("\u{2713} {}", branch_name))
        } else {
            SharedString::from(branch_name.clone())
        };
        let text_color = if *is_head { COLOR_SUCCESS } else { TEXT_MAIN };
        let branch_for_click = branch_name.clone();
        let is_head = *is_head;

        let row = if is_head {
            // HEAD branch: not clickable.
            div()
                .flex()
                .flex_row()
                .items_center()
                .flex_shrink_0()
                .px_3()
                .py_1()
                .text_sm()
                .text_color(rgb(text_color))
                .overflow_hidden()
                .child(label)
                .into_any()
        } else {
            // T028: single-click = jump to branch tip commit in graph;
            //        double-click = open checkout plan modal.
            // ClickEvent::click_count() returns the OS-level click count:
            //   1 = single click, 2 = double-click.
            // Note: for a double-click, gpui fires the on_click handler TWICE:
            // once with click_count=1 and once with click_count=2.  This means
            // the first click always performs the jump first — which is the
            // natural / intended behaviour (same as GitKraken).
            let click_handler = cx.listener(move |this, event: &gpui::ClickEvent, _window, cx| {
                if event.click_count() >= 2 {
                    // Double-click: open the checkout plan modal.
                    this.open_plan_modal(branch_for_click.clone());
                } else {
                    // Single-click: jump (scroll + select) to the branch tip.
                    this.jump_to_branch(&branch_for_click);
                }
                cx.notify();
            });
            div()
                .id(SharedString::from(format!("sidebar-branch-{}", branch_name)))
                .flex()
                .flex_row()
                .items_center()
                .flex_shrink_0()
                .px_3()
                .py_1()
                .text_sm()
                .text_color(rgb(text_color))
                .overflow_hidden()
                .on_click(click_handler)
                .hover(|style| style.bg(rgb(BG_SURFACE)))
                .child(label)
                .into_any()
        };

        col = col.child(row);
    }

    // ── STASHES section ──────────────────────────────────
    if !stashes.is_empty() {
        col = col.child(
            div()
                .px_3()
                .pt_3()
                .pb_1()
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(TEXT_MUTED))
                .child(SharedString::from("STASHES")),
        );

        for stash in stashes {
            let idx = stash.index;
            // Display as "stash@{N}: <message>", truncated.
            let raw_label = format!("stash@{{{}}}: {}", idx, stash.message);
            const MAX_STASH_CHARS: usize = 28;
            let display_label = if raw_label.chars().count() > MAX_STASH_CHARS {
                let tail: String = raw_label.chars().take(MAX_STASH_CHARS - 1).collect();
                format!("{}\u{2026}", tail)
            } else {
                raw_label
            };

            let click_handler = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.open_stash_apply_modal(idx);
                cx.notify();
            });

            col = col.child(
                div()
                    .id(("sidebar-stash", idx))
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_shrink_0()
                    .px_3()
                    .py_1()
                    .text_sm()
                    .text_color(rgb(COLOR_WARNING))
                    .on_click(click_handler)
                    .hover(|style| style.bg(rgb(BG_SURFACE)))
                    .child(SharedString::from(display_label)),
            );
        }
    }

    // Fixed-width outer shell; the inner column scrolls.
    div()
        .w(px(width))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(BG_SIDEBAR))
        .child(col)
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
    render_plan_modal_card(modal.plan, modal.error, "Checkout", cancel_handler, confirm_handler)
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
        this.confirm_pull();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(modal.plan, modal.error, "Pull", cancel_handler, confirm_handler)
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
    render_plan_modal_card(modal.plan, modal.error, "Undo", cancel_handler, confirm_handler)
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
    render_plan_modal_card(modal.plan, modal.error, "Pop", cancel_handler, confirm_handler)
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
        this.confirm_push();
        if let Some(fh) = this.root_focus.clone() {
            window.focus(&fh);
        }
        cx.notify();
    });
    render_plan_modal_card(modal.plan, modal.error, "Push", cancel_handler, confirm_handler)
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
) -> impl IntoElement {
    let has_blockers = !plan.blockers.is_empty();

    // ── Build modal card ────────────────────────────────────
    let mut card = div()
        .w(px(480.))
        .bg(rgb(BG_MODAL))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        // ── Title ─────────────────────────────────────────
        .child(
            div()
                .text_color(rgb(TEXT_MAIN))
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
                        .text_color(rgb(TEXT_LABEL))
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
                                .text_color(rgb(TEXT_MAIN))
                                .child(SharedString::from(plan.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(TEXT_SUB))
                                .child(SharedString::from(format!("[{}]", plan.current.dirty))),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT_LABEL))
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
                                .text_color(rgb(TEXT_MAIN))
                                .child(SharedString::from(plan.predicted.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(TEXT_SUB))
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
                    .text_color(rgb(COLOR_WARNING))
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
                    .text_color(rgb(TEXT_LABEL))
                    .child(SharedString::from(label)),
            );
        for entry in plan.preview_commits.iter().take(show_count) {
            let line: String = entry.chars().take(72).collect();
            commit_col = commit_col.child(
                div()
                    .text_xs()
                    .text_color(rgb(TEXT_SUB))
                    .overflow_hidden()
                    .child(SharedString::from(line)),
            );
        }
        if total > 10 {
            commit_col = commit_col.child(
                div()
                    .text_xs()
                    .text_color(rgb(TEXT_MUTED))
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
                    .text_color(rgb(COLOR_BLOCKER))
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
            .text_color(rgb(TEXT_MUTED))
            .overflow_hidden()
            .child(SharedString::from(plan.recovery.clone())),
    );

    // ── Error message (preflight / execute failure) ───────
    if let Some(err) = &error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(COLOR_BLOCKER))
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
                .bg(rgb(BG_SURFACE))
                .text_sm()
                .text_color(rgb(TEXT_MAIN))
                .on_click(cancel_handler)
                .hover(|style| style.bg(rgb(BG_SELECTED)))
                .child(SharedString::from("Cancel")),
        );

    // Checkout button: only shown when there are no blockers.
    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("plan-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(COLOR_BRANCH))
                .text_sm()
                .text_color(rgb(BG_BASE))
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
                .bg(rgb(BG_MODAL_OVERLAY))
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
    let input_display = SharedString::from(format!("{}_", modal.input)); // cursor indicator

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

    // ── Key handler for the input ─────────────────────────────
    let key_handler = cx.listener(|this, event: &KeyDownEvent, _window, cx| {
        this.handle_create_branch_key(event);
        cx.notify();
    });

    // ── Build modal card ────────────────────────────────────
    let mut card = div()
        .w(px(480.))
        .bg(rgb(BG_MODAL))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        // ── Title ─────────────────────────────────────────
        .child(
            div()
                .text_color(rgb(TEXT_MAIN))
                .text_xl()
                .child(SharedString::from(format!(
                    "Create branch @ {}",
                    modal.at.short()
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
                        .text_color(rgb(TEXT_LABEL))
                        .child(SharedString::from("Branch name")),
                )
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .bg(rgb(BG_BASE))
                        .rounded_sm()
                        .text_color(rgb(TEXT_MAIN))
                        .child(input_display),
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
                        .text_color(rgb(TEXT_LABEL))
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
                                .text_color(rgb(TEXT_MAIN))
                                .child(SharedString::from(p.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(TEXT_SUB))
                                .child(SharedString::from(format!("[{}]", p.current.dirty))),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT_LABEL))
                        .child(SharedString::from("\u{2192} Predicted")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT_MUTED))
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
                        .text_color(rgb(COLOR_BLOCKER))
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
                .text_color(rgb(TEXT_MUTED))
                .overflow_hidden()
                .child(SharedString::from(p.recovery.clone())),
        );
    }

    // ── Error message (preflight / execute failure) ───────
    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(COLOR_BLOCKER))
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
                .bg(rgb(BG_SURFACE))
                .text_sm()
                .text_color(rgb(TEXT_MAIN))
                .on_click(cancel_handler)
                .hover(|style| style.bg(rgb(BG_SELECTED)))
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
                .bg(rgb(COLOR_SUCCESS))
                .text_sm()
                .text_color(rgb(BG_BASE))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Create")),
        );
    }

    card = card.child(button_row);

    // ── Key-capture wrapper ─────────────────────────────────
    // We wrap the card in a focusable container that captures key-down events.
    let focusable_card = if let Some(ref fh) = focus_handle {
        div()
            .track_focus(fh)
            .on_key_down(key_handler)
            .child(card)
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
                .bg(rgb(BG_MODAL_OVERLAY))
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
    let input_display = SharedString::from(format!("{}_", modal.input));

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

    let key_handler = cx.listener(|this, event: &KeyDownEvent, _window, cx| {
        this.handle_stash_push_key(event);
        cx.notify();
    });

    let mut card = div()
        .w(px(480.))
        .bg(rgb(BG_MODAL))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_color(rgb(TEXT_MAIN))
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
                        .text_color(rgb(TEXT_LABEL))
                        .child(SharedString::from("Message (optional)")),
                )
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .bg(rgb(BG_BASE))
                        .rounded_sm()
                        .text_color(rgb(TEXT_MAIN))
                        .child(input_display),
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
                        .text_color(rgb(TEXT_LABEL))
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
                                .text_color(rgb(TEXT_MAIN))
                                .child(SharedString::from(p.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(TEXT_SUB))
                                .child(SharedString::from(format!("[{}]", p.current.dirty))),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT_LABEL))
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
                                .text_color(rgb(TEXT_MAIN))
                                .child(SharedString::from(p.predicted.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(TEXT_SUB))
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
                        .text_color(rgb(COLOR_WARNING))
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
                        .text_color(rgb(COLOR_BLOCKER))
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
                .text_color(rgb(TEXT_MUTED))
                .overflow_hidden()
                .child(SharedString::from(p.recovery.clone())),
        );
    }

    // ── Error message ──────────────────────────────────
    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(COLOR_BLOCKER))
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
                .bg(rgb(BG_SURFACE))
                .text_sm()
                .text_color(rgb(TEXT_MAIN))
                .on_click(cancel_handler)
                .hover(|style| style.bg(rgb(BG_SELECTED)))
                .child(SharedString::from("Cancel")),
        );

    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("stash-push-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(COLOR_WARNING))
                .text_sm()
                .text_color(rgb(BG_BASE))
                .on_click(confirm_handler)
                .hover(|style| style.opacity(0.85))
                .child(SharedString::from("Stash")),
        );
    }

    card = card.child(button_row);

    // ── Key-capture wrapper ─────────────────────────────────
    let focusable_card = if let Some(ref fh) = focus_handle {
        div()
            .track_focus(fh)
            .on_key_down(key_handler)
            .child(card)
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
                .bg(rgb(BG_MODAL_OVERLAY))
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
        .bg(rgb(BG_MODAL))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_color(rgb(TEXT_MAIN))
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
                        .text_color(rgb(TEXT_LABEL))
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
                                .text_color(rgb(TEXT_MAIN))
                                .child(SharedString::from(plan.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(TEXT_SUB))
                                .child(SharedString::from(format!("[{}]", plan.current.dirty))),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT_LABEL))
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
                                .text_color(rgb(TEXT_MAIN))
                                .child(SharedString::from(plan.predicted.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(TEXT_SUB))
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
                    .text_color(rgb(COLOR_BLOCKER))
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
            .text_color(rgb(TEXT_MUTED))
            .overflow_hidden()
            .child(SharedString::from(plan.recovery.clone())),
    );

    // ── Error message ────────────────────────────────────
    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(COLOR_BLOCKER))
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
                .bg(rgb(BG_SURFACE))
                .text_sm()
                .text_color(rgb(TEXT_MAIN))
                .on_click(cancel_handler)
                .hover(|style| style.bg(rgb(BG_SELECTED)))
                .child(SharedString::from("Cancel")),
        );

    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("stash-apply-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(COLOR_SUCCESS))
                .text_sm()
                .text_color(rgb(BG_BASE))
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
                .bg(rgb(BG_MODAL_OVERLAY))
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

    // Colour constants mirroring the detail panel.
    const COLOR_ADDED:    u32 = 0xa6e3a1;
    const COLOR_MODIFIED: u32 = 0xf9e2af;
    const COLOR_DELETED:  u32 = 0xf38ba8;
    const COLOR_RENAMED:  u32 = 0x89b4fa;
    const COLOR_TYPECHANGE: u32 = 0x585b70;
    const COLOR_DIR:      u32 = 0x6c7086;

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
                            .text_color(rgb(COLOR_DIR))
                            .child(name.clone()),
                    )
                    .into_any()
            }
            file_tree::TreeRow::File { depth, name, file_index, change } => {
                let indent = (*depth as f32) * 12.0;
                let (badge_char, badge_color) = match change {
                    ChangeKind::Added      => ("A", COLOR_ADDED),
                    ChangeKind::Modified   => ("M", COLOR_MODIFIED),
                    ChangeKind::Deleted    => ("D", COLOR_DELETED),
                    ChangeKind::Renamed { .. } => ("R", COLOR_RENAMED),
                    ChangeKind::TypeChange => ("T", COLOR_TYPECHANGE),
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
                            .text_color(rgb(TEXT_MAIN))
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
        .bg(rgb(BG_MODAL))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        // ── Title ─────────────────────────────────────────
        .child(
            div()
                .text_color(rgb(TEXT_MAIN))
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
                        .text_color(rgb(TEXT_LABEL))
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
                                .text_color(rgb(TEXT_MAIN))
                                .child(SharedString::from(plan.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(TEXT_SUB))
                                .child(SharedString::from(format!("[{}]", plan.current.dirty))),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT_LABEL))
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
                                .text_color(rgb(TEXT_MAIN))
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
                    .text_color(rgb(TEXT_LABEL))
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
                    .text_color(rgb(COLOR_WARNING))
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
                    .text_color(rgb(COLOR_BLOCKER))
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
            .text_color(rgb(TEXT_MUTED))
            .overflow_hidden()
            .child(SharedString::from(plan.recovery.clone())),
    );

    // ── Error message (preflight / execute failure) ───────
    if let Some(ref err) = modal.error {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(COLOR_BLOCKER))
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
                .bg(rgb(BG_SURFACE))
                .text_sm()
                .text_color(rgb(TEXT_MAIN))
                .on_click(cancel_handler)
                .hover(|style| style.bg(rgb(BG_SELECTED)))
                .child(SharedString::from("Cancel")),
        );

    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("cherry-pick-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(0xcba6f7)) // mauve
                .text_sm()
                .text_color(rgb(BG_BASE))
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
                .bg(rgb(BG_MODAL_OVERLAY))
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
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    const COLOR_DIR: u32      = 0x6c7086;

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
    let diff_view = panel.diff_view.clone();
    let selected_file = panel.selected_file.clone();

    // ── Tree view toggle ─────────────────────────────────────
    let toggle_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.toggle_commit_panel_tree_view();
        cx.notify();
    });

    let toggle_btn = div()
        .id("cp-tree-toggle")
        .px_1()
        .py_px()
        .rounded_sm()
        .bg(rgb(BG_SURFACE))
        .text_xs()
        .text_color(rgb(if tree_view { COLOR_BRANCH } else { TEXT_MUTED }))
        .on_click(toggle_click)
        .hover(|s| s.bg(rgb(BG_SELECTED)))
        .child(SharedString::from(if tree_view { "tree" } else { "flat" }));

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
                .text_color(rgb(TEXT_LABEL))
                .child(SharedString::from(format!("Unstaged ({})", unstaged_count))),
        )
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
                            .text_color(rgb(COLOR_DIR))
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
                    let row_bg = if is_conflicted_file { 0x3a1c1c } else if is_sel { BG_SELECTED } else { BG_PANEL };
                    let mut file_row = div()
                        .id(("cp-us-file", fi))
                        .flex()
                        .flex_row()
                        .items_center()
                        .pl(px(8.0 + indent))
                        .pr(px(2.0))
                        .py_px()
                        .bg(rgb(row_bg))
                        .hover(|s| s.bg(rgb(BG_SURFACE)))
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
                                .text_color(rgb(TEXT_MAIN))
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
                                .bg(rgb(COLOR_SUCCESS))
                                .text_xs()
                                .text_color(rgb(BG_BASE))
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
                                .bg(rgb(0xf38ba8))
                                .text_xs()
                                .text_color(rgb(BG_BASE))
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
            let row_bg = if is_conflicted_file { 0x3a1c1c } else if is_sel { BG_SELECTED } else { BG_PANEL };
            let mut file_row = div()
                .id(("cp-us-flat-file", fi))
                .flex()
                .flex_row()
                .items_center()
                .px_2()
                .py_px()
                .bg(rgb(row_bg))
                .hover(|s| s.bg(rgb(BG_SURFACE)))
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
                        .text_color(rgb(TEXT_MAIN))
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
                        .bg(rgb(COLOR_SUCCESS))
                        .text_xs()
                        .text_color(rgb(BG_BASE))
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
                        .bg(rgb(0xf38ba8)) // red
                        .text_xs()
                        .text_color(rgb(BG_BASE))
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
                .text_color(rgb(TEXT_LABEL))
                .child(SharedString::from(format!("Staged ({})", staged_count))),
        );

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
                            .text_color(rgb(COLOR_DIR))
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
                            .flex()
                            .flex_row()
                            .items_center()
                            .pl(px(8.0 + indent))
                            .pr(px(2.0))
                            .py_px()
                            .bg(rgb(if is_sel { BG_SELECTED } else { BG_PANEL }))
                            .hover(|s| s.bg(rgb(BG_SURFACE)))
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
                                    .text_color(rgb(TEXT_MAIN))
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
                                    .bg(rgb(COLOR_WARNING))
                                    .text_xs()
                                    .text_color(rgb(BG_BASE))
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
                    .flex()
                    .flex_row()
                    .items_center()
                    .px_2()
                    .py_px()
                    .bg(rgb(if is_sel { BG_SELECTED } else { BG_PANEL }))
                    .hover(|s| s.bg(rgb(BG_SURFACE)))
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
                            .text_color(rgb(TEXT_MAIN))
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
                            .bg(rgb(COLOR_WARNING))
                            .text_xs()
                            .text_color(rgb(BG_BASE))
                            .on_click(unstage_click)
                            .hover(|s| s.opacity(0.8))
                            .child(SharedString::from("Unstage")),
                    ),
            );
        }
    }

    // ── Diff viewer ──────────────────────────────────────────
    let diff_area: gpui::AnyElement = if let Some(dv) = diff_view {
        let diff_row_count = dv.rows.len();
        let rows_arc = std::sync::Arc::new(dv.rows);
        let rows_for_list = rows_arc.clone();
        uniform_list(
            "cp-diff-list",
            diff_row_count,
            cx.processor(move |_this, range, _window, _cx| {
                render_diff_rows(&rows_for_list, range)
            }),
        )
        .flex_1()
        .min_h(px(0.))
        .into_any_element()
    } else {
        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(rgb(TEXT_MUTED))
            .child(SharedString::from("Select a file to view diff"))
            .into_any_element()
    };

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
            .bg(rgb(BG_BASE))
            .rounded_sm()
            .text_xs()
            .text_color(rgb(TEXT_MUTED))
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
            .bg(rgb(COLOR_BRANCH))
            .text_sm()
            .text_color(rgb(BG_BASE))
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
            .bg(rgb(BG_SURFACE))
            .text_sm()
            .text_color(rgb(TEXT_MUTED))
            .child(SharedString::from(reason))
            .into_any_element()
    };

    // ── Assemble panel ───────────────────────────────────────
    // T027: ファイル領域(Unstaged箱 + Staged箱)とdiff領域を flex_1 で 1:1 に分割する。
    // 高さ配分: ファイル領域(flex_1) : diff領域(flex_1) = 1:1
    // 各箱はさらに 1:1 に分割(各 flex_1 + min_h(px(0.)) + overflow_y_scroll)。
    // ヘッダは各箱の外で flex_shrink_0 に固定し、スクロール対象はファイル行のみ。
    div()
        .w(px(panel_width))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(BG_PANEL))
        // Header
        .child(
            div()
                .flex_shrink_0()
                .px_2()
                .py_1()
                .bg(rgb(BG_SURFACE))
                .text_sm()
                .text_color(rgb(TEXT_MAIN))
                .child(SharedString::from("Commit Panel")),
        )
        // T027: ファイル領域コンテナ (flex_1 + min_h(0)) — Unstaged箱 + Staged箱を 1:1 で収める
        .child(
            div()
                .id("cp-files-container")
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_col()
                // Unstaged ヘッダ (固定)
                .child(unstaged_header)
                // T027: Unstaged スクロールボックス (flex_1 + min_h(0) + 薄枠)
                .child(
                    div()
                        .id("cp-unstaged-scroll")
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .mx_1()
                        .mb_px()
                        .border_1()
                        .border_color(rgb(BG_SURFACE))
                        .rounded_sm()
                        .child(unstaged_files),
                )
                // Staged ヘッダ (固定)
                .child(staged_header)
                // T027: Staged スクロールボックス (flex_1 + min_h(0) + 薄枠)
                .child(
                    div()
                        .id("cp-staged-scroll")
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .mx_1()
                        .mb_px()
                        .border_1()
                        .border_color(rgb(BG_SURFACE))
                        .rounded_sm()
                        .child(staged_files),
                ),
        )
        // Diff area (flex_1 — takes remaining space equal to files container)
        .child(
            div()
                .id("cp-diff-area")
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_shrink_0()
                        .px_2()
                        .py_px()
                        .bg(rgb(BG_SURFACE))
                        .text_xs()
                        .text_color(rgb(TEXT_MUTED))
                        .child(SharedString::from("diff")),
                )
                .child(diff_area),
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
                .bg(rgb(BG_SURFACE))
                // Message label + input
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(TEXT_LABEL))
                        .child(SharedString::from("Commit message")),
                )
                .child(msg_input_wrapper)
                // Unstaged warning
                .when(has_unstaged_warning, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(rgb(COLOR_WARNING))
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
                .text_color(rgb(TEXT_LABEL))
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
                        .text_color(rgb(0x6c7086u32))
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
                                .text_color(rgb(TEXT_MAIN))
                                .overflow_hidden()
                                .child(name.clone()),
                        ),
                );
            }
        }
    }

    let mut card = div()
        .w(px(480.))
        .bg(rgb(BG_MODAL))
        .rounded_lg()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .text_color(rgb(TEXT_MAIN))
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
                        .text_color(rgb(TEXT_LABEL))
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
                                .text_color(rgb(TEXT_MAIN))
                                .child(SharedString::from(plan.current.head.clone())),
                        )
                        .child(
                            div()
                                .text_color(rgb(TEXT_SUB))
                                .child(SharedString::from(format!("[{}]", plan.current.dirty))),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT_LABEL))
                        .child(SharedString::from("\u{2192} Predicted")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(TEXT_MAIN))
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
                    .text_color(rgb(COLOR_WARNING))
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
                    .text_color(rgb(COLOR_BLOCKER))
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
                .text_color(rgb(COLOR_BLOCKER))
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
                .bg(rgb(BG_SURFACE))
                .text_sm()
                .text_color(rgb(TEXT_MAIN))
                .on_click(cancel_handler)
                .hover(|style| style.bg(rgb(BG_SELECTED)))
                .child(SharedString::from("Cancel")),
        );

    if !has_blockers {
        button_row = button_row.child(
            div()
                .id("commit-plan-confirm")
                .px_3()
                .py_1()
                .rounded_sm()
                .bg(rgb(COLOR_BRANCH))
                .text_sm()
                .text_color(rgb(BG_BASE))
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
                .bg(rgb(BG_MODAL_OVERLAY))
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
    use gpui::{Application, Bounds, Timer, WindowBounds, WindowOptions, size};
    use std::sync::mpsc;
    use std::time::Duration;

    // T029: start the .git watcher before entering the gpui event loop.
    // Extract repo_path first; if absent (error-state app) skip the watcher.
    let maybe_watcher: Option<(mpsc::Receiver<()>, notify::RecommendedWatcher)> =
        app_state.repo_path.as_ref().and_then(|p| watcher::start_git_watcher(p));

    Application::new().run(move |cx: &mut App| {
        // T025: initialize gpui-component (registers key bindings, themes, etc.)
        gpui_component::init(cx);

        // T-BP-002: register cmd-j as the toggle key for the bottom panel.
        // context = None means the binding fires regardless of focus context.
        cx.bind_keys([KeyBinding::new("cmd-j", ToggleBottomPanel, None)]);

        // KAGI_WINDOW=WxH (dev/testing only): override the initial window size
        // so layout behaviour at small sizes can be verified headlessly.
        let (win_w, win_h) = std::env::var("KAGI_WINDOW")
            .ok()
            .and_then(|s| {
                let (w, h) = s.split_once('x')?;
                Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
            })
            .unwrap_or((1024.0, 768.0));
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

                // T029: if the watcher started successfully, spawn a background
                // loop that waits for events and triggers reload_external().
                if let Some((rx, _watcher_handle)) = maybe_watcher {
                    // Keep the watcher alive by moving it into a long-lived task.
                    // We move `_watcher_handle` into the async block so it is
                    // dropped only when the task finishes (i.e. the app quits).
                    let weak = kagi.downgrade();
                    cx.spawn(async move |async_cx| {
                        // Suppress unused-variable warning: watcher lifetime is
                        // the point — we hold it so notify doesn't stop.
                        let _watcher = _watcher_handle;

                        loop {
                            // Sleep briefly before checking the channel to avoid
                            // a busy loop.  100 ms granularity is fine here;
                            // debounce happens AFTER the first signal arrives.
                            Timer::after(Duration::from_millis(100)).await;

                            // Check if a signal arrived.
                            let got_signal = rx.try_recv().is_ok();
                            if !got_signal {
                                continue;
                            }

                            // Signal received — wait the debounce window then
                            // drain any additional signals that arrived during it.
                            Timer::after(watcher::DEBOUNCE).await;
                            while rx.try_recv().is_ok() {}

                            // Upgrade WeakEntity and call reload_external.
                            // If the entity has been dropped (window closed), exit.
                            let result = async_cx.update(|cx| {
                                weak.update(cx, |app, cx| {
                                    app.reload_external(cx);
                                })
                            });
                            if result.is_err() {
                                // App is gone; stop the loop.
                                break;
                            }
                        }
                    })
                    .detach();
                }

                cx.new(|cx| gpui_component::Root::new(kagi, window, cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
