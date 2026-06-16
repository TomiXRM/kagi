//! UI module — T008: GPUI commit list / T009: commit graph lane / T010: commit selection + detail panel / T011: changed files list / T012: file diff viewer / T013: checkout plan modal + sidebar / T023: pane resize / T-BP-002: bottom panel open/close + resize / T-BP-007: terminal
//!
//! This module lives in the binary crate (`main.rs` does `mod ui;`).
//! It must not be added to `src/lib.rs` so that domain tests stay
//! independent of GPUI.

pub mod assets;
pub mod avatar;
pub mod avatar_fetch;
pub mod branch_menu;
pub mod commands;
pub mod commit_list;
pub mod commit_panel;
pub mod conflict_editor;
pub mod conflict_view;
pub mod context_menu;
pub mod detail_panel;
pub mod diff_view;
pub mod diffstat_bar;
pub mod file_tree;
pub mod graph_view;
pub mod i18n;
pub mod inspector;
pub mod modals;
pub mod settings_view;
pub mod sidebar;
pub mod smart_commit;
pub mod stash_menu;
pub mod tabs;
pub mod terminal;
pub mod theme;
pub mod watcher;

pub use diff_view::*;
use i18n::Msg;
pub use modals::*;
use theme::theme;

use kagi::git::message_gen;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{
    actions, div, prelude::*, px, rgb, uniform_list, App, ClipboardItem, Context, Entity,
    FocusHandle, KeyBinding, KeyDownEvent, MouseButton, ScrollStrategy, SharedString,
    UniformListScrollHandle, Window,
};
use gpui_component::input::{Input, InputState};
use gpui_component::scroll::Scrollbar;
use gpui_component::tooltip::Tooltip;
use gpui_component::Sizable as _;

// ──────────────────────────────────────────────────────────────
// T-BP-002: Bottom Panel — action + tab enum
// ──────────────────────────────────────────────────────────────

// cmd-j toggle action for the bottom panel.
// escape to close main diff view.
actions!(
    kagi,
    [
        ToggleBottomPanel,
        CloseMainDiff,
        DiffPrevFile,
        DiffNextFile,
        CheckoutSelected
    ]
);

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
    /// T-CONFLICT-UI-003: vertical divider between the A (Current) and B
    /// (Incoming) panes in the Conflict Editor (adjusts the A|B width ratio).
    ConflictAB,
    /// T-CONFLICT-UI-003: horizontal divider between the A·B row (top) and the
    /// Result pane (bottom) in the Conflict Editor (adjusts that split ratio).
    ConflictResult,
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
pub struct DividerGhost;
impl gpui::Render for DividerGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        div()
    }
}

// ── T-DNDMERGE-001 / ADR-0079: branch drag-and-drop merge ─────
//
// Dragging a LOCAL branch label in the sidebar and dropping it on the current
// (checked-out) branch row starts a "merge dragged into current" preview.  The
// drop is a TRIGGER ONLY: it dispatches to `KagiApp::start_merge_from_drag`,
// which validates and delegates to the existing `open_merge_modal` pipeline.
// No git is executed on drop (ADR-0079).

/// Drag payload carrying the dragged local branch name.  Layer 1 (the view)
/// emits this on `on_drag`; the current-branch drop zone consumes it via
/// `on_drop::<BranchDrag>` and dispatches it to the action layer.
#[derive(Clone, Debug)]
pub struct BranchDrag {
    /// The dragged local branch name (= merge *source*).
    pub name: String,
}

/// Ghost chip rendered next to the cursor while a branch is being dragged, so
/// the user can see which branch they are dragging (ADR-0079 acceptance: the
/// dragged branch name is visible during the drag).
pub struct BranchDragGhost {
    pub name: SharedString,
}
impl gpui::Render for BranchDragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        // T-DNDMERGE-001: the ghost must look like the LIFTED branch badge so the
        // chip "sticks" to the cursor (user request). Mirror the real branch
        // badge chip rendered in `render_badges_column` for `BadgeKind::Branch`:
        // same `badge_style(color_branch)` tint, `rounded_sm`, `px_1`, `text_sm`.
        // gpui renders this ghost entity at the cursor automatically, so we only
        // need it to LOOK like the lifted badge. Used by BOTH the graph-badge
        // drag and the sidebar drag (kept consistent).
        let (badge_bg, badge_border, badge_text) = theme::badge_style(theme().color_branch);
        div()
            .px_1()
            .rounded_sm()
            .bg(gpui::rgba(badge_bg))
            .border_1()
            .border_color(gpui::rgba(badge_border))
            .text_sm()
            .text_color(rgb(badge_text))
            .child(self.name.clone())
    }
}

/// T-DNDMERGE-001 / ADR-0079: pure helper that extracts the draggable branch
/// name from a graph ref-badge for the `BranchDrag { name }` payload.
///
/// `BadgeKind::Branch` (label IS the plain local-branch name) and
/// `BadgeKind::Remote` (label IS the full `remote/name` ref) are both draggable
/// merge sources — a remote chip lets an upstream-only branch be merged
/// directly via its remote-tracking ref (resolved by the merge backend), with
/// no local branch required. `HeadBranch` (label = `"<name> ✓"`, and it is the
/// drop *target*, not a source) and `Tag` chips are NOT draggable → `None`.
fn draggable_branch_name(badge: &commit_list::RefBadge) -> Option<String> {
    match badge.kind {
        BadgeKind::Branch | BadgeKind::Remote => Some(badge.label.to_string()),
        BadgeKind::HeadBranch | BadgeKind::Tag => None,
    }
}

/// Pure validation for [`KagiApp::start_merge_from_drag`] (ADR-0079 layer 2),
/// extracted so the rejection rules can be unit-tested without a `Window`/`cx`.
///
/// `branches` is the local-branch list as held in `KagiApp::branches`
/// (`(name, is_head)`); `remotes` is the list of remote-tracking ref names
/// (`"origin/feature"`, from `KagiApp::remote_branches`).  Returns `Ok(())` when
/// the drag may proceed to `open_merge_modal(source)`, or `Err(reason)`
/// describing why it is rejected (same-branch / unknown branch / busy).
/// `plan_merge_branch` remains the authoritative guard for dirty-WT / ff /
/// conflict prediction.
fn validate_merge_from_drag(
    source: &str,
    branches: &[(String, bool)],
    remotes: &[String],
    busy: bool,
) -> Result<(), String> {
    if busy {
        return Err(Msg::OpInProgress.t().to_string());
    }
    // A local branch (but not the current one — that's a no-op merge).
    if let Some((_, is_head)) = branches.iter().find(|(n, _)| n == source) {
        if *is_head {
            return Err(format!(
                "Branch '{}' is already the current branch.",
                source
            ));
        }
        return Ok(());
    }
    // Or a remote-tracking branch: an upstream-only branch is merged directly
    // via its remote ref (no local branch needed). It can never be HEAD.
    if remotes.iter().any(|n| n == source) {
        return Ok(());
    }
    Err(format!("Branch '{}' is not a branch.", source))
}

/// Bundled UI sans family (OFL Inter), loaded at startup via `add_fonts`, so the
/// UI looks identical on every OS instead of relying on the platform default.
pub const UI_FONT: &str = "Inter";
/// Bundled monospace family (OFL JetBrains Mono) for the terminal / conflict
/// editor / code — replaces the macOS-only "Menlo" fallback.
pub const MONO_FONT: &str = "JetBrains Mono";

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
const INSPECTOR_SPLIT_DEFAULT: f32 = 0.35; // message 35% : files 65% (user request: files +30%)
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

// ── T-CONFLICT-UI-003: Conflict Editor split ratios ──────────
/// A|B horizontal split ratio (fraction of the A·B row width given to A).
const CONFLICT_AB_DEFAULT: f32 = 0.5;
const CONFLICT_AB_MIN: f32 = 0.2;
const CONFLICT_AB_MAX: f32 = 0.8;
/// A·B / Result vertical split ratio (fraction of the editor height given to
/// the A·B row; the remainder is the Result pane).
const CONFLICT_RESULT_DEFAULT: f32 = 0.55;
const CONFLICT_RESULT_MIN: f32 = 0.25;
const CONFLICT_RESULT_MAX: f32 = 0.8;
/// Height of the Conflict Editor split divider handles.
const CONFLICT_SPLIT_DIVIDER: f32 = 4.0;

// ── W2-GRAPH: compact mode ────────────────────────────────────
/// Row height for normal (full) mode.
const ROW_H_FULL: f32 = graph_view::ROW_H; // 29.0 (= 24 * 1.2)
/// Row height for compact mode.
const ROW_H_COMPACT: f32 = 22.0; // 18.0 * 1.2 (keeps compact:full ratio)

/// Return the row height for the current compact mode setting.
///
/// W28: the result is **zoom-scaled** (`base * theme::zoom()`) so the commit-row
/// container height grows/shrinks in lock-step with the rem-scaled row text and
/// the graph canvas drawn inside it. The graph canvas reads its *measured*
/// height (`bounds.size.height`) for vertical anchoring, so returning the
/// scaled height here is what keeps the ● node centred and edges connecting
/// row-to-row with zero drift at any zoom. Compact mode scales by the same
/// factor (`ROW_H_COMPACT * zoom()`), preserving the compact:full ratio.
#[inline]
fn row_height(compact: bool) -> f32 {
    theme::scaled(if compact { ROW_H_COMPACT } else { ROW_H_FULL })
}

/// Friendly present-progressive label for the busy snackbar, keyed by the
/// `busy_op` tag set when an async op starts.
fn busy_label(op: &str) -> String {
    let s = match op {
        "merge-plan" => "Planning merge…",
        "merge" => "Merging…",
        "pull" => "Pulling…",
        "push" => "Pushing…",
        "fetch" => "Fetching…",
        "commit" => "Committing…",
        "amend" => "Amending commit…",
        "checkout" => "Checking out…",
        "cherry-pick" => "Cherry-picking…",
        "revert" => "Reverting…",
        "discard" => "Discarding…",
        "stash" => "Stashing…",
        "stash-pop" => "Applying stash…",
        "stash-drop" => "Dropping stash…",
        "create-worktree" => "Creating worktree…",
        "delete-branch" => "Deleting branch…",
        "rename-branch" => "Renaming branch…",
        "set-upstream" => "Setting upstream…",
        other => return format!("{other}…"),
    };
    s.to_string()
}

use branch_menu::{
    BranchAction, BranchConflictMode, BranchKind, BranchMenuContext, BranchMenuState,
};
use commit_list::{BadgeKind, CommitRow};
use commit_panel::{status_badge, CommitPanelFileRef, CommitPanelState, CommitPlanModal};
use context_menu::{CommitAction, CommitMenuState, MenuContext};
use detail_panel::{build_commit_details, CommitDetail};
use graph_view::graph_canvas;
use kagi::git::{
    oplog::{append_oplog, read_oplog_tail, OpLogEntry, OpOutcome},
    ops::{
        default_tracking_branch_name, validate_branch_rename, AmendMode, MergeKind, OperationPlan,
        PullOutcome, StateSummary,
    },
    CommitId, DiffLineKind, FileDiffStat, FileStatus, Head, RemoteBranch, RepoSnapshot, Stash, Tag,
    UpstreamInfo, Worktree,
};

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
        use commit_list::now_unix_secs;
        use kagi::git::Head;

        let (branch, ahead, behind, no_upstream, is_detached, is_unborn, upstream_name) =
            match &snap.head {
                Head::Attached { branch, .. } => {
                    // Look up upstream info for this branch.
                    let upstream = snap
                        .branches
                        .iter()
                        .find(|b| &b.name == branch)
                        .and_then(|b| b.upstream.as_ref());
                    match upstream {
                        Some(u) => (
                            branch.clone(),
                            Some(u.ahead),
                            Some(u.behind),
                            false,
                            false,
                            false,
                            u.remote_branch.clone(),
                        ),
                        None => (
                            branch.clone(),
                            None,
                            None,
                            true,
                            false,
                            false,
                            String::new(),
                        ),
                    }
                }
                Head::Detached { target } => {
                    let short = target.get(..8).unwrap_or(target).to_string();
                    (
                        format!("detached HEAD ({})", short),
                        None,
                        None,
                        false,
                        true,
                        false,
                        String::new(),
                    )
                }
                Head::Unborn { branch } => (
                    format!("no commits yet ({})", branch),
                    None,
                    None,
                    false,
                    false,
                    true,
                    String::new(),
                ),
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

        // Pull: enabled whenever the current branch has an upstream (attached,
        // born). We intentionally do NOT require behind>0: the behind count is
        // only as fresh as the last fetch, so gating on a stale behind==0 would
        // wrongly disable Pull right after the upstream advanced (the GitHub-merge
        // catch-22). Pull fetches first anyway, so a truly up-to-date pull is a
        // harmless no-op.
        let pull_on = !no_upstream;
        // Push: enabled whenever a remote exists and HEAD is an attached, born
        // branch. Like Pull we don't require ahead>0 (stale until fetch); pushing
        // with nothing ahead is a harmless no-op. Dirty WT is irrelevant — push
        // never changes local state.
        let push_on = !not_attached && self.has_remote;
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
    fn make_summary(
        ahead: usize,
        behind: usize,
        is_dirty: bool,
        stash_count: usize,
    ) -> StatusBarSummary {
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
    /// Pull/Push stay ENABLED: ahead/behind are only as fresh as the last fetch,
    /// so gating on them caused the "can't pull after a GitHub merge" catch-22.
    /// They no-op when the branch is genuinely in sync.
    #[test]
    fn toolbar_clean_behind0() {
        let s = make_summary(0, 0, false, 0);
        let t = s.toolbar_state();
        assert!(
            t.pull_on,
            "pull stays on with an upstream (counts may be stale)"
        );
        assert!(
            t.push_on,
            "push stays on with a remote (counts may be stale)"
        );
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

    /// fixture main (ahead=1, behind=0): Pull is now ENABLED — it is no longer
    /// gated on behind>0 (it would be stale until a fetch anyway).
    #[test]
    fn toolbar_fixture_main_behind0_pull_on() {
        // This mirrors the fixture repo: main is 1 ahead, 0 behind.
        let s = make_summary(1, 0, false, 0);
        let t = s.toolbar_state();
        assert!(
            t.pull_on,
            "pull stays on with an upstream even when behind=0"
        );
        assert!(t.push_on, "fixture main (ahead=1) must have push=on");
        assert!(t.undo_on, "fixture main (ahead=1) must have undo=on");
    }

    /// feature/two (ahead=0, behind=1): Pull on; Push is now also ENABLED — it
    /// is no longer gated on ahead>0 (pushing nothing is a harmless no-op).
    #[test]
    fn toolbar_feature_two_behind1_pull_on() {
        // Mirrors fixture feature/two: 0 ahead, 1 behind.
        let s = make_summary(0, 1, false, 0);
        let t = s.toolbar_state();
        assert!(t.pull_on, "feature/two (behind=1) must have pull=on");
        assert!(
            t.push_on,
            "push stays on with a remote (ahead=0 no longer disables)"
        );
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
    /// Sync-flavoured info (blue, 4s): renders the same big spinning sync icon
    /// as the busy snackbar, so "already up to date" reads consistently with
    /// an in-flight pull/push. Used for the no-op pull/push snackbars.
    Sync,
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
    /// Creation time; drives the enter animation and auto-dismiss timing.
    pub born: std::time::Instant,
    /// `Some(t)` once the toast has begun sliding out (auto-expiry or the ×
    /// button): `t` is when the exit animation started. The toast keeps
    /// rendering during the exit so the slide-out is visible, then a one-shot
    /// timer removes it. `None` while it is entering / visible.
    pub dismissing: Option<std::time::Instant>,
}

impl Toast {
    /// Auto-dismiss lifetime by kind (errors stay longer).
    fn lifetime(&self) -> Duration {
        match self.kind {
            ToastKind::Error => Duration::from_secs(8),
            _ => Duration::from_secs(4),
        }
    }

    /// The toast has been visible long enough to start sliding out (and is not
    /// already doing so).
    fn should_start_exit(&self) -> bool {
        self.dismissing.is_none() && self.born.elapsed() >= self.lifetime()
    }
}

/// Maximum simultaneously visible toasts (oldest dropped beyond this).
const TOASTS_MAX: usize = 4;

/// Snackbar slide animation timings / distance.
const TOAST_ENTER_MS: u64 = 240;
const TOAST_EXIT_MS: u64 = 220;
/// Remove the toast slightly before the exit animation's nominal end so it
/// never reverts to its resting (visible) state for a frame before removal.
const TOAST_REMOVE_MS: u64 = 200;
/// Horizontal slide distance (px): far enough to clear the left window edge,
/// so the toast slides fully in from / out to off-screen.
const TOAST_SLIDE_PX: f32 = 500.0;

// ──────────────────────────────────────────────────────────────

/// W29-I18N-WAVE2: build the localized blocker list for an [`OperationPlan`].
///
/// `blockers` is the English-only list from the plan (preserved for the
/// execute-guard and tests). `keyed` yields `(english, localized)` pairs for the
/// in-scope validation reasons. Each blocker whose text matches a keyed
/// `english` is shown in its `localized` form; every other blocker passes
/// through verbatim. Order follows `blockers`.
fn localize_plan_blockers(
    blockers: &[String],
    keyed: impl Iterator<Item = (String, String)>,
) -> Vec<SharedString> {
    let map: std::collections::HashMap<String, String> = keyed.collect();
    blockers
        .iter()
        .map(|b| match map.get(b) {
            Some(localized) => SharedString::from(localized.clone()),
            None => SharedString::from(b.clone()),
        })
        .collect()
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
    /// Stash nodes rendered in the graph below the WIP row (ADR-0088).
    pub stash_graph_rows: Vec<commit_list::StashRow>,
    /// Lanes used by stash branch lines (passed to the graph painter so those
    /// nodes/edges are drawn in the stash colour).
    pub stash_graph_lanes: Vec<usize>,
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
    /// W16-DIFFSTAT: per-file additions/deletions for the selected commit,
    /// keyed by row index. Computed lazily alongside `diff_cache` and consumed
    /// by the Inspector changed-files list (truncated set only).
    pub diffstat_cache: HashMap<usize, Vec<FileDiffStat>>,
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
    /// When `Some`, the amend confirmation modal is visible (T-COMMIT-011).
    pub amend_modal: Option<AmendPlanModal>,
    /// When `Some`, the stash-pop confirmation modal is visible (T-HT-007).
    pub pop_modal: Option<PopPlanModal>,
    /// When `Some`, the stash-drop confirmation modal is visible (ADR-0087).
    pub stash_drop_modal: Option<StashDropModal>,
    /// When `Some`, the push plan confirmation modal is visible (T-HT-004).
    pub push_modal: Option<PushPlanModal>,
    pub branch_plan_modal: Option<BranchPlanModal>,
    pub set_upstream_modal: Option<SetUpstreamModal>,
    pub rename_branch_modal: Option<RenameBranchModal>,
    /// When `Some`, the merge plan confirmation modal is visible.
    pub merge_modal: Option<MergePlanModal>,
    /// When `Some`, the remote tracking checkout plan modal is visible.
    pub tracking_checkout_modal: Option<TrackingCheckoutPlanModal>,
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
    // ── T-COMMIT-009 / W14-TEMPLATE: structured template mode ─────
    /// `true` when the commit message is being authored via the structured
    /// template fields (type/scope/summary/body/test/risk); `false` = the plain
    /// single Input. Persisted to / restored from the draft's `mode` field.
    pub commit_template_mode: bool,
    /// Lazily-created `InputState`s for the six template fields, in order:
    /// `[type, scope, summary, body, test, risk]`. Created on first switch into
    /// template mode (requires `&mut Window`). Same gpui-component widget as the
    /// plain Input — no hand-written input widgets.
    pub commit_template_inputs: Option<[Entity<InputState>; 6]>,
    // ── T-COMMIT-016: Smart Commit Message (W14-SMART) ───────────
    /// Smart Commit state: rule-based always on, LLM opt-in + detection.
    pub smart_commit: smart_commit::SmartCommitState,
    /// Guard so Ollama detection runs at most once per repo path.
    pub smart_commit_detected_for: Option<PathBuf>,
    /// A generated message produced on a background thread, queued for the next
    /// render to push into the commit-message Input (which needs `&mut Window`).
    pub pending_smart_msg: Option<String>,
    // ── T028: branch jump (scroll to commit) ─────────────────
    /// Scroll handle for the "commit-list" uniform_list.
    /// Stored in KagiApp so it persists across render frames.
    pub commit_scroll_handle: UniformListScrollHandle,
    /// PERF-SIDEBAR-VIRT: scroll handle for the sidebar navigator
    /// `uniform_list` ("sidebar-list"). Persisted across frames.
    pub sidebar_scroll_handle: UniformListScrollHandle,
    /// PERF-SIDEBAR-VIRT: pre-flattened sidebar rows, rebuilt each render from
    /// the snapshot fields (branches/remotes/tags/…) honouring collapse +
    /// filter state. The "sidebar-list" `uniform_list` processor reads
    /// `self.sidebar_rows[i]`, so the sidebar costs O(visible rows) per frame.
    pub sidebar_rows: Vec<sidebar::SidebarRow>,
    /// PERF: scroll handle for the commit panel's Unstaged `uniform_list`
    /// (shared across flat/tree views — only one is visible at a time).
    pub cp_unstaged_scroll_handle: UniformListScrollHandle,
    /// PERF: scroll handle for the commit panel's Staged `uniform_list`.
    pub cp_staged_scroll_handle: UniformListScrollHandle,
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
    // ── T-UNDOREDO-001 / ADR-0081: Operation Undo / Redo history ──
    /// In-session undo/redo stack of ref-moving operations (commit, merge,
    /// cherry-pick, revert, amend, undo-commit). Entries record the branch and
    /// the before/after commit SHAs; undo/redo move the branch ref between them
    /// via the safe pipeline. Lost on quit (reflog is the durable backstop).
    pub operation_history: kagi::git::OperationHistory,
    /// Whether the reflog-seed of `operation_history` has been attempted for the
    /// current repo (ADR-0084). Set on the first render with a repo open so undo
    /// works on a freshly-opened repo (the initial CLI/snapshot path never calls
    /// `reload()`); reset on reload / tab switch so the next repo re-seeds.
    pub history_seed_attempted: bool,
    /// Set while an Undo/Redo plan modal is open; carries the entry being
    /// previewed and whether it is an undo (`true`) or redo (`false`).
    pub history_modal: Option<HistoryPlanModal>,
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
    /// Settings overlay: whether the Theme dropdown (appearance section) is
    /// expanded. The option list renders inline below the field when `true`.
    pub settings_theme_open: bool,
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
    /// W13-BRANCHTREE: collapsed branch *groups* (the `/`-prefix sub-trees
    /// inside LOCAL / REMOTE BRANCHES). Keys are dynamic strings of the form
    /// `local:feat` / `remote:origin` — hence a separate `HashSet<String>`
    /// rather than the `&'static str` `sidebar_collapsed` above.
    /// Default-expanded (a key present ⇒ that group is collapsed), mirroring
    /// `sidebar_collapsed` semantics. Preserved across reloads.
    pub branch_groups_collapsed: HashSet<String>,
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
    /// True while a background fetch is in flight (refresh / auto-fetch),
    /// so we never stack concurrent fetches.
    pub fetch_in_flight: bool,
    /// True while the periodic background auto-fetch ticker task is alive
    /// (spawned lazily from render; see `ensure_auto_fetch_ticker`).
    pub auto_fetch_ticker_alive: bool,
    /// When `Some`, the refresh icon spins (set on click; cleared after one
    /// full rotation in render).
    pub refresh_spin_started: Option<Instant>,
    /// Last commit-message value mirrored to the per-branch draft file
    /// (T-COMMIT-007). Compared each frame to detect edits cheaply.
    pub last_draft_value: String,
    /// Debounce generation for the draft autosave writer.
    pub draft_save_gen: u64,
    /// Debounce generation for modal live re-planning. Each input change
    /// bumps it; a 250ms timer task re-plans only if no newer change arrived.
    /// Per-keystroke synchronous re-planning (backend open + plan build,
    /// the stash modal even scans status) was the user-reported input lag.
    pub modal_replan_gen: u64,
    /// Name of the git operation currently running on a background thread
    /// (e.g. "pull"/"push"). While `Some`, toolbar git buttons are disabled
    /// and new plan modals are refused so operations never overlap.
    pub busy_op: Option<&'static str>,
    // ── W2-DELETE: Delete-branch modal ───────────────────────
    /// When `Some`, the delete-branch confirmation modal is visible.
    pub delete_branch_modal: Option<DeleteBranchModal>,
    // ── W17-DISCARD: discard confirmation modal ──────────────
    /// When `Some`, the discard (danger) confirmation modal is visible.
    pub discard_modal: Option<DiscardModal>,
    /// Commit row context menu state (right-click anchor + target row).
    pub commit_menu: Option<CommitMenuState>,
    /// Branch sidebar context menu state (right-click anchor + target branch).
    pub branch_menu: Option<BranchMenuState>,
    pub stash_menu: Option<stash_menu::StashMenuState>,
    /// Unstaged file-row context menu (right-click): (unstaged index, anchor).
    /// Offers Discard for eligible (tracked, non-conflicted) rows.
    pub file_menu: Option<(usize, gpui::Point<gpui::Pixels>)>,
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
    /// Linux/FreeBSD client-side menu dropdown currently open from the in-app
    /// menu bar. Native macOS menus are provided by `cx.set_menus`, so this is
    /// only read on Linux/FreeBSD (dead on other targets).
    #[cfg_attr(not(any(target_os = "linux", target_os = "freebsd")), allow(dead_code))]
    pub platform_menu_open: Option<usize>,
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
    // ── W30-CONFLICT-UI: Conflict Mode (ADR-0056) ────────────────
    /// `Some` while the repository is mid merge / rebase / cherry-pick / revert
    /// with conflicts (Conflict Mode).  `None` in Normal mode.  Detected via
    /// `detect_conflict_session` at startup, after the FS watcher fires, and
    /// after any operation finishes (all funnel through `reload()` /
    /// `detect_conflict_mode`).  The repository is untouched until Continue /
    /// Abort execute through the existing plan pipeline.
    pub conflict: Option<conflict_view::ConflictMode>,
    /// Guard so render-time detection runs at most once per (repo_path) until a
    /// reload / tab switch invalidates it — mirrors `avatar_fetch_for`.  Holds
    /// the repo path whose conflict state has been detected this cycle.
    pub conflict_detected_for: Option<PathBuf>,
    /// W32-CONFLICT-EDITOR: `Some(path)` while the dedicated hunk-level Conflict
    /// Editor is open for that conflicting file.  `None` shows the Dashboard
    /// (`conflict_view`, W33's lane).  Set/cleared only by the editor handlers
    /// here; the editor reads the per-file [`HunkModel`] from `self.conflict`'s
    /// buffer.
    pub conflict_editing: Option<PathBuf>,
    /// W32: last-saved resolved text per file, so a Save can log a before→after
    /// file-content hash pair (T-035) without re-reading the working tree.
    pub conflict_editing_before_text: HashMap<PathBuf, String>,
    /// T-CONFLICT-UI-001/005: the three CodeEditor `InputState`s backing the
    /// Conflict Editor's A / B / Result panes (ADR-0069).  Lazily created in a
    /// `Window` context (`sync_conflict_editor_inputs`) and rebuilt whenever the
    /// edited file or its assembled Result text changes.  `None` until the
    /// editor first renders for a content file.
    pub conflict_editor_inputs: Option<ConflictEditorInputs>,
    /// T-CONFLICT-UX-015: whether the Result pane is in Edit mode (editable)
    /// rather than Preview (read-only).
    pub conflict_result_editing: bool,
    /// T-CONFLICT-POLISH-042: armed state for the destructive "Reset all"
    /// (two-stage confirm).
    pub conflict_reset_all_armed: bool,
    /// T-CONFLICT-UI-003: A|B pane width split ratio (fraction given to A).
    pub conflict_ab_split: f32,
    /// T-CONFLICT-UI-003: A·B / Result vertical split ratio (fraction to A·B).
    pub conflict_result_split: f32,
    /// T-CONFLICT-UI-003: measured (top, bottom) screen-px bounds of the
    /// editor's split region, for absolute-coordinate divider dragging.
    pub conflict_geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    /// T-CONFLICT-UI-003: measured (left, right) screen-px bounds of the A·B
    /// row, for the vertical A|B divider drag.
    pub conflict_ab_geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    /// T-CONFLICT-FLOW-030/031 (ADR-0068): when `true`, a merge has been
    /// continued (every file saved + staged) and we are showing the commit
    /// message panel pre-filled with the merge message.  MERGE_HEAD is still
    /// present, so the commit panel's commit button creates the 2-parent merge
    /// commit via `execute_merge_commit`.  Cleared on commit / abort / reload.
    pub conflict_merge_commit_pending: bool,
    /// Set by `detect_conflict_mode` when the in-progress operation is a **merge**
    /// whose conflicts are all resolved (MERGE_HEAD present, no remaining unmerged
    /// index entries).  This is the "ready to create the merge commit" state — the
    /// app shows the commit panel, not an empty Conflict Mode editor.  Used by
    /// `reload` to keep the merge commit panel alive across the FS-watcher reload
    /// that the resolution staging itself triggers (otherwise the panel would be
    /// torn down and re-replaced by an empty conflict view).
    pub merge_commit_ready: bool,
    /// Auto-update (ADR-0082): the offered update + its source release, set by the
    /// startup background check when a newer stable release exists for this
    /// platform. `None` = up to date / not yet checked / skipped.
    pub update_available: Option<(
        kagi_domain::update::UpdatePlan,
        kagi_domain::update::ReleaseInfo,
    )>,
    /// Run-once guard for the startup update check.
    pub update_checked: bool,
    /// Whether the update detail modal is open.
    pub update_modal_open: bool,
    /// Whether an install is in progress (disables the confirm button).
    pub update_installing: bool,
    /// Progress / error line shown in the update modal.
    pub update_status: Option<SharedString>,
    /// Last loaded working-tree status, used by the FS watcher's working-tree
    /// path to skip a refresh when nothing the parent repo cares about changed
    /// (e.g. churn inside a nested worktree, which `working_tree_status` treats as
    /// opaque). Set on every `reload`.
    pub last_working_status: Option<kagi::git::WorkingTreeStatus>,
    /// T-CONFLICT-UX-010/012: index (among conflict hunks) of the focused hunk in
    /// the per-hunk Conflict Editor, so the selected-hunk highlight tracks the
    /// hunk the user last interacted with / navigated to.
    pub conflict_selected_hunk: usize,
    /// ADR-0070: shared A/B uniform-list scroll handle for synchronized vertical
    /// scrolling in the Conflict Editor.
    pub conflict_ab_scroll_handle: UniformListScrollHandle,
    /// T-CONFLICT-FLOW-032 (ADR-0068): sequencer `<op> --continue` confirmation
    /// modal, shown when Continue routes a rebase / cherry-pick / revert.
    pub conflict_continue_modal: Option<ConflictContinuePlanModal>,
}

/// T-CONFLICT-UI-001: the Result `InputState` entity backing the Conflict
/// Editor, plus the cache key (edited path + assembled Result hash) used to
/// detect when the Result text needs to be re-pushed into the editor.
///
/// A and B are row lists with checkboxes; `result` remains a CodeEditor
/// InputState for Preview/Edit mode (ADR-0071 / UX-015).
pub struct ConflictEditorInputs {
    /// The conflicting file these inputs are bound to.
    pub path: PathBuf,
    /// Result code editor (read-only in Preview, editable in Edit mode).
    pub result: Entity<InputState>,
    /// Hash of the Result text last pushed, so we only `set_value` on change
    /// (avoids clobbering an in-progress manual edit every frame).
    pub content_sig: u64,
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
    eprintln!("[kagi] graph: lane_count={}", lane_count);
    eprintln!("[kagi] commit list rows: {}", rows.len());
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
            worktrees,
        } = view;

        KagiApp {
            root_focus: None,
            header,
            rows,
            stash_graph_rows,
            stash_graph_lanes,
            details,
            selected: None,
            error: None,
            repo_path: None,
            diff_cache: HashMap::new(),
            diffstat_cache: HashMap::new(),
            main_diff: None,
            compare_view: None,
            main_diff_scroll_handle: UniformListScrollHandle::new(),
            branches,
            plan_modal: None,
            pull_modal: None,
            undo_modal: None,
            amend_modal: None,
            pop_modal: None,
            stash_drop_modal: None,
            push_modal: None,
            branch_plan_modal: None,
            set_upstream_modal: None,
            rename_branch_modal: None,
            merge_modal: None,
            tracking_checkout_modal: None,
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
            badge_col_w: theme::read_col_width("badge_col_w")
                .map(|w| w.clamp(BADGE_COL_MIN, BADGE_COL_MAX))
                .unwrap_or(BADGE_COL_DEFAULT),
            graph_col_w: theme::read_col_width("graph_col_w")
                .map(|w| w.clamp(GRAPH_COL_MIN, GRAPH_COL_MAX))
                .unwrap_or(GRAPH_COL_DEFAULT),
            bottom_panel_open: true, // user request: terminal visible by default
            bottom_panel_height: BOTTOM_PANEL_H_UNSET,
            bottom_tab: BottomTab::Terminal, // user request: terminal is the default tab
            commit_panel_open: false,
            commit_panel: None,
            commit_input: None,
            commit_template_mode: false,
            commit_template_inputs: None,
            smart_commit: smart_commit::SmartCommitState::load(),
            smart_commit_detected_for: None,
            pending_smart_msg: None,
            commit_scroll_handle: UniformListScrollHandle::new(),
            sidebar_scroll_handle: UniformListScrollHandle::new(),
            sidebar_rows: Vec::new(),
            cp_unstaged_scroll_handle: UniformListScrollHandle::new(),
            cp_staged_scroll_handle: UniformListScrollHandle::new(),
            branch_targets,
            commit_row_index,
            status_summary,
            toolbar_state,
            op_entries,
            oplog_scroll_handle: UniformListScrollHandle::new(),
            oplog_expanded: None,
            operation_history: kagi::git::OperationHistory::new(),
            history_seed_attempted: false,
            history_modal: None,
            terminal_sessions: HashMap::new(),
            tabs: Vec::new(),
            active_tab: 0,
            watcher_generation: 0,
            inspector_tree_view: true,
            inspector_split: INSPECTOR_SPLIT_DEFAULT,
            inspector_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            graph_compact: theme::compact_graph(),
            settings_theme_open: false,
            graph_scroll_x: 0.0,
            // W2-SIDEBAR
            remote_branches,
            tags,
            worktrees,
            branch_upstream_info,
            sidebar_collapsed: HashSet::new(),
            branch_groups_collapsed: HashSet::new(),
            sidebar_filter: None,
            // W3-NOTIFY
            toasts: Vec::new(),
            next_toast_id: 0,
            toast_ticker_alive: false,
            fetch_in_flight: false,
            auto_fetch_ticker_alive: false,
            busy_op: None,
            modal_replan_gen: 0,
            last_draft_value: String::new(),
            draft_save_gen: 0,
            refresh_spin_started: None,
            // W2-DELETE
            delete_branch_modal: None,
            discard_modal: None,
            commit_menu: None,
            branch_menu: None,
            stash_menu: None,
            file_menu: None,
            // W5-MENU
            sidebar_visible: true,
            inspector_visible: true,
            menu_overlay: None,
            platform_menu_open: None,
            // W6-TABSPEED
            tab_cache: HashMap::new(),
            switch_generation: 0,
            loading_tab: None,
            // W11-AVATAR
            avatar_images: HashMap::new(),
            avatar_fetch_for: None,
            // W30-CONFLICT-UI
            conflict: None,
            conflict_detected_for: None,
            conflict_editing: None,
            conflict_editing_before_text: HashMap::new(),
            conflict_editor_inputs: None,
            conflict_result_editing: false,
            conflict_reset_all_armed: false,
            conflict_ab_split: CONFLICT_AB_DEFAULT,
            conflict_result_split: CONFLICT_RESULT_DEFAULT,
            conflict_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            conflict_ab_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            conflict_merge_commit_pending: false,
            merge_commit_ready: false,
            update_available: None,
            update_checked: false,
            update_modal_open: false,
            update_installing: false,
            update_status: None,
            last_working_status: None,
            conflict_selected_hunk: 0,
            conflict_ab_scroll_handle: UniformListScrollHandle::new(),
            conflict_continue_modal: None,
        }
    }

    /// Construct a placeholder for the no-argument / error case.
    pub fn with_error(message: impl Into<String>) -> Self {
        KagiApp {
            root_focus: None,
            header: SharedString::from("kagi"),
            rows: Vec::new(),
            stash_graph_rows: Vec::new(),
            stash_graph_lanes: Vec::new(),
            details: Vec::new(),
            selected: None,
            error: Some(SharedString::from(message.into())),
            repo_path: None,
            diff_cache: HashMap::new(),
            diffstat_cache: HashMap::new(),
            main_diff: None,
            compare_view: None,
            main_diff_scroll_handle: UniformListScrollHandle::new(),
            branches: Vec::new(),
            plan_modal: None,
            pull_modal: None,
            undo_modal: None,
            amend_modal: None,
            pop_modal: None,
            stash_drop_modal: None,
            push_modal: None,
            branch_plan_modal: None,
            set_upstream_modal: None,
            rename_branch_modal: None,
            merge_modal: None,
            tracking_checkout_modal: None,
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
            badge_col_w: theme::read_col_width("badge_col_w")
                .map(|w| w.clamp(BADGE_COL_MIN, BADGE_COL_MAX))
                .unwrap_or(BADGE_COL_DEFAULT),
            graph_col_w: theme::read_col_width("graph_col_w")
                .map(|w| w.clamp(GRAPH_COL_MIN, GRAPH_COL_MAX))
                .unwrap_or(GRAPH_COL_DEFAULT),
            bottom_panel_open: true, // user request: terminal visible by default
            bottom_panel_height: BOTTOM_PANEL_H_UNSET,
            bottom_tab: BottomTab::Terminal, // user request: terminal is the default tab
            commit_panel_open: false,
            commit_panel: None,
            commit_input: None,
            commit_template_mode: false,
            commit_template_inputs: None,
            smart_commit: smart_commit::SmartCommitState::load(),
            smart_commit_detected_for: None,
            pending_smart_msg: None,
            commit_scroll_handle: UniformListScrollHandle::new(),
            sidebar_scroll_handle: UniformListScrollHandle::new(),
            sidebar_rows: Vec::new(),
            cp_unstaged_scroll_handle: UniformListScrollHandle::new(),
            cp_staged_scroll_handle: UniformListScrollHandle::new(),
            branch_targets: HashMap::new(),
            commit_row_index: HashMap::new(),
            status_summary: StatusBarSummary::default(),
            toolbar_state: ToolbarState::default(),
            op_entries: VecDeque::new(),
            oplog_scroll_handle: UniformListScrollHandle::new(),
            oplog_expanded: None,
            operation_history: kagi::git::OperationHistory::new(),
            history_seed_attempted: false,
            history_modal: None,
            terminal_sessions: HashMap::new(),
            tabs: Vec::new(),
            active_tab: 0,
            watcher_generation: 0,
            inspector_tree_view: true,
            inspector_split: INSPECTOR_SPLIT_DEFAULT,
            inspector_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            graph_compact: theme::compact_graph(),
            settings_theme_open: false,
            graph_scroll_x: 0.0,
            // W2-SIDEBAR
            remote_branches: Vec::new(),
            tags: Vec::new(),
            worktrees: Vec::new(),
            branch_upstream_info: HashMap::new(),
            sidebar_collapsed: HashSet::new(),
            branch_groups_collapsed: HashSet::new(),
            sidebar_filter: None,
            // W3-NOTIFY
            toasts: Vec::new(),
            next_toast_id: 0,
            toast_ticker_alive: false,
            fetch_in_flight: false,
            auto_fetch_ticker_alive: false,
            busy_op: None,
            modal_replan_gen: 0,
            last_draft_value: String::new(),
            draft_save_gen: 0,
            refresh_spin_started: None,
            // W2-DELETE
            delete_branch_modal: None,
            discard_modal: None,
            commit_menu: None,
            branch_menu: None,
            stash_menu: None,
            file_menu: None,
            // W5-MENU
            sidebar_visible: true,
            inspector_visible: true,
            menu_overlay: None,
            platform_menu_open: None,
            // W6-TABSPEED
            tab_cache: HashMap::new(),
            switch_generation: 0,
            loading_tab: None,
            // W11-AVATAR
            avatar_images: HashMap::new(),
            avatar_fetch_for: None,
            // W30-CONFLICT-UI
            conflict: None,
            conflict_detected_for: None,
            conflict_editing: None,
            conflict_editing_before_text: HashMap::new(),
            conflict_editor_inputs: None,
            conflict_result_editing: false,
            conflict_reset_all_armed: false,
            conflict_ab_split: CONFLICT_AB_DEFAULT,
            conflict_result_split: CONFLICT_RESULT_DEFAULT,
            conflict_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            conflict_ab_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            conflict_merge_commit_pending: false,
            merge_commit_ready: false,
            update_available: None,
            update_checked: false,
            update_modal_open: false,
            update_installing: false,
            update_status: None,
            last_working_status: None,
            conflict_selected_hunk: 0,
            conflict_ab_scroll_handle: UniformListScrollHandle::new(),
            conflict_continue_modal: None,
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
        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] reload: repo open error: {}", e);
                return;
            }
        };
        let snap = match repo.snapshot(10_000) {
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
        self.diffstat_cache = HashMap::new();
        self.main_diff = None;
        self.compare_view = None;
        self.plan_modal = None;
        self.pull_modal = None;
        self.undo_modal = None;
        self.amend_modal = None;
        self.pop_modal = None;
        self.stash_drop_modal = None;
        self.branch_plan_modal = None;
        self.set_upstream_modal = None;
        self.rename_branch_modal = None;
        self.discard_modal = None;
        self.create_branch_modal = None;
        self.create_worktree_modal = None;
        self.modal_focus = None;
        self.stash_push_modal = None;
        self.stash_apply_modal = None;
        self.stash_push_focus = None;
        self.cherry_pick_modal = None;
        self.revert_modal = None;
        self.conflict_continue_modal = None;
        // A merge that has been continued to the commit panel triggers its own
        // FS-watcher reload (staging writes the working tree + index). Preserve
        // the commit panel + merge message across that self-induced reload so the
        // user is not bounced out of the commit screen; the post-detect block
        // below confirms the merge is still pending (else it resets everything).
        let was_merge_commit_pending = self.conflict_merge_commit_pending;
        self.commit_menu = None;
        self.file_menu = None;
        self.stash_menu = None;
        if !was_merge_commit_pending {
            // ADR-0068: a reload after commit / abort ends any continued-merge flow.
            self.conflict_merge_commit_pending = false;
            // T025/T026: reset commit panel and input so it reflects fresh status after reload.
            self.commit_panel_open = false;
            self.commit_panel = None;
            self.commit_input = None;
            // T-COMMIT-009: reset template mode + field inputs to match commit_input.
            self.commit_template_mode = false;
            self.commit_template_inputs = None;
        }
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
        // W13-BRANCHTREE: branch_groups_collapsed is likewise preserved so the
        // user's per-group ▸/▾ state survives checkout/reload.

        // ADR-0030 §5: keep the stale-while-revalidate cache fresh.
        self.tab_cache.insert(repo_path.clone(), view.clone());

        // Fold the snapshot-derived data in (assignment only).
        self.apply_tab_view(view);

        // ADR-0084: seed the undo/redo history from the branch reflog when it is
        // empty (freshly-opened repo / post-branch-switch) so Cmd+Z works
        // immediately. Only seed when empty — never clobber the in-session stack.
        self.seed_history_from_reflog(&repo);

        // Baseline for the FS watcher's working-tree path (skip-if-unchanged).
        self.last_working_status = Some(snap.status.clone());

        // W30-CONFLICT-UI / ADR-0056: re-detect Conflict Mode every reload so a
        // conflict produced by the GUI's own operation OR by external CLI (the
        // watcher path runs through reload) puts the app into / out of Conflict
        // Mode.  Force re-detection by invalidating the render-time guard.
        self.conflict_detected_for = None;
        self.detect_conflict_mode();

        // Re-resolve the continued-merge flow after detection.
        if was_merge_commit_pending {
            if self.merge_commit_ready {
                // Still a resolved merge awaiting its commit: keep the commit
                // panel up (refresh the staged list from the index) and keep the
                // pre-filled / user-edited merge message entity untouched.
                let mut panel = CommitPanelState::from_repo(&repo_path);
                if let Some(ref existing) = self.commit_panel {
                    panel.tree_view = existing.tree_view;
                }
                self.commit_panel = Some(panel);
                self.commit_panel_open = true;
                self.conflict = None;
                self.conflict_merge_commit_pending = true;
            } else {
                // The merge commit was created (MERGE_HEAD gone) or aborted — end
                // the flow and reset the deferred commit-panel state.
                self.conflict_merge_commit_pending = false;
                self.commit_panel_open = false;
                self.commit_panel = None;
                self.commit_input = None;
                self.commit_template_mode = false;
                self.commit_template_inputs = None;
            }
        }
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
        self.stash_graph_rows = view.stash_graph_rows;
        self.stash_graph_lanes = view.stash_graph_lanes;
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
        let prev_commit_id: Option<CommitId> = self
            .selected
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
        self.status_footer =
            FooterStatus::Idle(SharedString::from("[kagi] refreshed (external change)"));

        // Notify gpui that state has changed so the window repaints.
        cx.notify();
    }

    /// Working-tree change refresh (FS watcher, [`watcher::WatchEvent::WorkTree`]).
    ///
    /// Files changed on disk outside `.git` — so the WIP / working-tree status may
    /// have changed, but the commit graph did not. Computes the new status on a
    /// **background thread** and only does a (full) refresh if it actually differs
    /// from [`Self::last_working_status`]. This makes churn that doesn't affect the
    /// parent repo's status (e.g. writes inside a nested worktree, which
    /// `working_tree_status` treats as opaque) a cheap no-op — no UI-thread work,
    /// no reload storm — while real edits/adds/deletes update the WIP promptly.
    pub fn refresh_working_tree_external(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let bg_path = repo_path.clone();
        let task = cx.background_spawn(async move {
            kagi::git::Backend::open(&bg_path)
                .ok()
                .and_then(|b| b.working_tree_status().ok())
        });
        cx.spawn(async move |this, acx| {
            let new_status = task.await;
            let _ = this.update(acx, |app, cx| {
                let Some(new_status) = new_status else {
                    return;
                };
                if app.last_working_status.as_ref() == Some(&new_status) {
                    return; // working-tree status unchanged → nothing to do.
                }
                eprintln!("[kagi] watcher: working-tree changed — refreshing WIP");
                // In-place WIP/status update — do NOT full-reload (that re-snapshots
                // the graph and closes the commit panel). Branch / ahead-behind are
                // unchanged by a working-tree edit, so only the dirty/count fields
                // and the commit panel's file lists need refreshing.
                app.status_summary.is_dirty = new_status.is_dirty();
                app.status_summary.staged = new_status.staged.len();
                app.status_summary.unstaged = new_status.unstaged.len();
                app.is_dirty = new_status.is_dirty();
                app.last_working_status = Some(new_status);
                // Refresh the open commit panel's lists in place (keeps it open).
                if app.commit_panel.is_some() {
                    if let Some(rp) = app.repo_path.clone() {
                        if let Some(panel) = app.commit_panel.as_mut() {
                            panel.reload_status(&rp);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ── W30-CONFLICT-UI: Conflict Mode (ADR-0056) ────────────────────────

    /// Detect (or clear) Conflict Mode for the currently-open repository.
    ///
    /// Runs at most once per `repo_path` per cycle (the `conflict_detected_for`
    /// guard, reset by `reload()` / tab switch / the watcher).  Opens the repo
    /// read-only, calls `detect_conflict_session`, and on a hit builds a fresh
    /// `ResolutionBuffer` from the index (preferring a previously autosaved
    /// buffer so a partial resolution survives a restart), recomputes each
    /// file's status from the buffer, and stores the `ConflictMode`.  On a miss
    /// it clears `self.conflict`.  The repository is never mutated here.
    pub fn detect_conflict_mode(&mut self) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                self.conflict = None;
                return;
            }
        };
        // Run-once guard per repo path.
        if self.conflict_detected_for.as_deref() == Some(repo_path.as_path()) {
            return;
        }
        self.conflict_detected_for = Some(repo_path.clone());

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(_) => {
                self.conflict = None;
                return;
            }
        };

        // Recomputed fresh every detection (the run-once guard is reset by
        // reload / tab switch before each call).
        self.merge_commit_ready = false;

        let session = match repo.detect_conflict_session() {
            Some(s) => s,
            None => {
                if self.conflict.is_some() {
                    eprintln!("[kagi] conflict-mode: cleared");
                }
                self.conflict = None;
                self.conflict_editing = None;
                return;
            }
        };

        // A merge with MERGE_HEAD present but no remaining unmerged index entries
        // is not a conflict to resolve — it is a resolved merge ready to commit.
        // Show the commit panel (handled by `reload`), not an empty Conflict Mode
        // editor.  Without this the FS-watcher reload that staging triggers would
        // re-enter Conflict Mode with zero files and clobber the commit panel.
        if matches!(session.op, kagi::git::ConflictOp::Merge { .. }) && session.files.is_empty() {
            eprintln!("[kagi] conflict-mode: merge resolved — ready to commit");
            self.merge_commit_ready = true;
            self.conflict = None;
            self.conflict_editing = None;
            return;
        }

        // Build / reload the resolution buffer.  A previously-autosaved buffer
        // (e.g. from before a restart) is preferred so partial work survives;
        // otherwise materialize a fresh buffer from the index conflicts.
        let buffer = kagi::git::ResolutionBuffer::load(&repo_path)
            .or_else(|| repo.resolution_buffer_from_repo().ok())
            .unwrap_or_else(|| kagi::git::ResolutionBuffer::new(&repo_path));

        // Current branch short name (for the side_labels left role).
        let current_branch = self.status_summary.branch.clone();

        // Recompute per-file status from the buffer (detection seeds Unresolved).
        let mut session = session;
        let residue = buffer.files_with_marker_residue();
        for f in &mut session.files {
            if buffer.has_resolution(&f.path) {
                f.status = if residue.contains(&f.path) {
                    kagi::git::ConflictStatus::NeedsReview
                } else {
                    kagi::git::ConflictStatus::Resolved
                };
            } else {
                f.status = kagi::git::ConflictStatus::Unresolved;
            }
        }

        // Preserve the previously-selected file across re-detections; otherwise
        // open the first unresolved file (KDiff3-style "land on work to do").
        let prev_selected = self.conflict.as_ref().and_then(|c| c.selected_file);
        let selected_file = prev_selected
            .filter(|&i| i < session.files.len())
            .or_else(|| {
                session
                    .files
                    .iter()
                    .position(|f| f.status == kagi::git::ConflictStatus::Unresolved)
            })
            .or_else(|| (!session.files.is_empty()).then_some(0));

        eprintln!(
            "[kagi] conflict-mode: {} {} file(s)",
            session.op.slug(),
            session.files.len()
        );

        // W32: close the editor if the file being edited is no longer conflicted.
        if let Some(editing) = self.conflict_editing.clone() {
            if !session.files.iter().any(|f| f.path == editing) {
                self.conflict_editing = None;
            }
        }
        // W33: preserve the dashboard editing-file index across re-detection.
        let prev_editing = self.conflict.as_ref().and_then(|c| c.editing_file);
        let editing_file = prev_editing.filter(|&i| i < session.files.len());

        self.conflict = Some(conflict_view::ConflictMode {
            session,
            buffer,
            current_branch,
            selected_file,
            editing_file,
            abort_armed: false,
        });

        // The center A/B editor renders from the hunk model, which needs the
        // repo to materialize zdiff3 markers. With auto-selection the user never
        // clicked, so build the hunk model for the selected content file here
        // (otherwise the editor shows the "no hunk model" fallback message).
        if let Some(c) = self.conflict.as_mut() {
            if let Some(idx) = c.selected_file {
                if let Some(f) = c.session.files.get(idx) {
                    if f.kind == kagi::git::ConflictKind::Content {
                        let path = f.path.clone();
                        if let Some(markers) = repo.materialized_markers(&c.buffer, &path) {
                            c.buffer.ensure_hunks(&path, &markers);
                        }
                        self.conflict_editing = Some(path);
                    }
                }
            }
        }
    }

    // ────────────────────────────────────────────────────────────
    // W32-CONFLICT-EDITOR: hunk-level editor open/close + hunk dispatch
    // ────────────────────────────────────────────────────────────

    /// Open the dedicated Conflict Editor for the conflicting file at `path`.
    ///
    /// Builds (idempotently) the per-file [`HunkModel`] in the buffer from the
    /// repository's zdiff3 materialization, then sets `conflict_editing`.  If the
    /// file has no usable text merge (binary / single-sided) the editor still
    /// opens and shows guidance (the hunk model is absent).  The repository is
    /// opened read-only; nothing is written.
    pub fn conflict_open_editor(&mut self, path: &std::path::Path) {
        // Materialize the markers (needs the repo) and build the hunk model.
        if let Some(repo_path) = self.repo_path.clone() {
            if let Ok(repo) = kagi::git::Backend::open(&repo_path) {
                if let Some(c) = self.conflict.as_mut() {
                    if let Some(markers) = repo.materialized_markers(&c.buffer, path) {
                        c.buffer.ensure_hunks(path, &markers);
                    }
                }
            }
        }
        // Keep the Dashboard selection in sync so back/forth is coherent.
        if let Some(c) = self.conflict.as_mut() {
            if let Some(idx) = c.session.files.iter().position(|f| f.path == path) {
                c.selected_file = Some(idx);
                c.editing_file = Some(idx);
            }
        }
        self.conflict_editing = Some(path.to_path_buf());
    }

    /// T-CONFLICT-UX-010/012: set the focused hunk (selected-hunk highlight).
    pub fn conflict_editor_select_hunk(&mut self, hunk_index: usize) {
        self.conflict_selected_hunk = hunk_index;
    }

    fn conflict_editor_after_selection_change(
        &mut self,
        path: &std::path::Path,
        selected_hunk: Option<usize>,
    ) {
        self.conflict_reset_all_armed = false;
        if let Some(hunk) = selected_hunk {
            self.conflict_selected_hunk = hunk;
        }
        if let Some(i) = self.conflict_editor_inputs.as_mut() {
            i.content_sig = 0;
        }
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        let residue = c.buffer.files_with_marker_residue();
        if let Some(f) = c.session.files.iter_mut().find(|f| f.path == path) {
            f.status = if !c.buffer.has_resolution(path) {
                kagi::git::ConflictStatus::Unresolved
            } else if residue.contains(&f.path) {
                kagi::git::ConflictStatus::NeedsReview
            } else {
                kagi::git::ConflictStatus::Resolved
            };
        }
        let _ = c.buffer.autosave();
    }

    pub fn conflict_editor_set_file_side(
        &mut self,
        path: &std::path::Path,
        side: kagi::git::resolution::SelectionSide,
        taken: bool,
    ) {
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        if c.buffer.set_file_side_selection(path, side, taken) {
            self.conflict_editor_after_selection_change(path, None);
        }
    }

    pub fn conflict_editor_set_hunk_side(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        side: kagi::git::resolution::SelectionSide,
        taken: bool,
    ) {
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        if c.buffer
            .set_hunk_side_selection(path, hunk_index, side, taken)
        {
            self.conflict_editor_after_selection_change(path, Some(hunk_index));
        }
    }

    pub fn conflict_editor_set_hunk_line(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        side: kagi::git::resolution::SelectionSide,
        line_index: usize,
        taken: bool,
    ) {
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        if c.buffer
            .set_hunk_line_selection(path, hunk_index, side, line_index, taken)
        {
            self.conflict_editor_after_selection_change(path, Some(hunk_index));
        }
    }

    pub fn conflict_editor_set_hunk_order(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        order: kagi::git::resolution::LineOrder,
    ) {
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        if c.buffer.set_hunk_line_order(path, hunk_index, order) {
            self.conflict_editor_after_selection_change(path, Some(hunk_index));
        }
    }

    /// T-CONFLICT-POLISH-042: "Reset all" is destructive (drops every hunk
    /// choice for this file), so it is two-stage: the first click arms the
    /// confirmation, the second performs the reset.  The armed flag is cleared
    /// by any other editor interaction (handled where those run) and on the
    /// performed reset.
    pub fn conflict_editor_reset_all_request(&mut self, path: &std::path::Path) {
        if self.conflict_reset_all_armed {
            self.conflict_reset_all_armed = false;
            self.conflict_editor_reset_all(path);
        } else {
            self.conflict_reset_all_armed = true;
        }
    }

    /// T-CONFLICT-UX-015: toggle the Result pane between Preview (read-only) and
    /// Edit (editable) mode.  Leaving Edit mode does not discard the text — the
    /// edits were already pulled into the buffer via `set_manual_text` during
    /// the sync pass.
    pub fn conflict_editor_toggle_result_mode(&mut self) {
        self.conflict_result_editing = !self.conflict_result_editing;
        // Force the inputs to re-sync (mode is part of the content signature).
        if let Some(i) = self.conflict_editor_inputs.as_mut() {
            i.content_sig = 0;
        }
    }

    /// Reset every hunk of `path` to unresolved (toolbar "Reset all").
    pub fn conflict_editor_reset_all(&mut self, path: &std::path::Path) {
        // Force the editor inputs to re-sync after the reset.
        if let Some(i) = self.conflict_editor_inputs.as_mut() {
            i.content_sig = 0;
        }
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        let n = c.buffer.hunk_count(path);
        for i in 0..n {
            c.buffer.reset_hunk(path, i);
        }
        // Reset leaves marker residue → status becomes NeedsReview (still has a
        // result draft, but unresolved markers remain).
        let residue = c.buffer.files_with_marker_residue();
        if let Some(f) = c.session.files.iter_mut().find(|f| f.path == path) {
            f.status = if residue.contains(&f.path) {
                kagi::git::ConflictStatus::NeedsReview
            } else if c.buffer.has_resolution(path) {
                kagi::git::ConflictStatus::Resolved
            } else {
                kagi::git::ConflictStatus::Unresolved
            };
        }
        let _ = c.buffer.autosave();
    }

    /// Move the editor's view to the next (`dir > 0`) / previous (`dir < 0`)
    /// **unresolved** hunk by selecting an adjacent still-conflicted file when the
    /// current one is done.  MVP: hunks scroll within the file; this navigates the
    /// file selection so prev/next always lands on work to do.
    pub fn conflict_editor_nav_hunk(&mut self, dir: i32) {
        // For MVP, prev/next reuse the Dashboard unresolved-file navigation and
        // re-open the editor on the newly selected file.
        self.conflict_nav_unresolved(dir);
        if let Some(c) = self.conflict.as_ref() {
            if let Some(idx) = c.selected_file {
                if let Some(f) = c.session.files.get(idx) {
                    let p = f.path.clone();
                    self.conflict_open_editor(&p);
                }
            }
        }
    }

    /// Entry point for "Open external tool" (ADR-0060 / ADR-0064 toolbar).  The
    /// actual launch is W33's lane; here we only record the intent + toast so the
    /// button is wired and discoverable.
    pub fn conflict_editor_open_external(&mut self, path: &std::path::Path) {
        eprintln!(
            "[kagi] conflict-editor: external tool requested for {} (launch is W33)",
            path.display()
        );
        self.push_toast(
            ToastKind::Info,
            SharedString::from(format!(
                "External merge tool launch is not wired yet ({}).",
                path.display()
            )),
        );
    }

    /// Save resolution (ADR-0068 / T-CONFLICT-UX-013/014): write the resolved
    /// Result to the **working tree**, run the marker-residue check (markers
    /// remaining BLOCK the save), then **stage** the file so its index unmerged
    /// entries (stage 1/2/3) collapse to stage 0.  Moves the file into Resolved
    /// Files, re-evaluates the continue gate, autosaves the buffer, and records
    /// the resolution action to the operation log (T-035).  No commit is created.
    pub fn conflict_editor_save(&mut self, path: &std::path::Path) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(c) = self.conflict.as_ref() else {
            return;
        };

        // Before/after hashes of the file's resolved text for the oplog.
        let before_text = self
            .conflict_editing_before_text
            .get(path)
            .cloned()
            .unwrap_or_default();
        let after_text = c.buffer.resolved_text(path).unwrap_or_default();
        let before_hash = short_hash(&before_text);
        let after_hash = short_hash(&after_text);

        // Per-hunk action summary for the log.
        let actions = c
            .buffer
            .hunk_model(path)
            .map(|m| {
                m.hunks()
                    .iter()
                    .enumerate()
                    .map(|(i, h)| format!("{}:{}", i, hunk_choice_slug(&h.choice)))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let session_slug = c.session.op.slug().to_string();
        let op_name = format!("conflict-save:{}", session_slug);
        let before = StateSummary {
            head: format!("session={} file={}", session_slug, path.display()),
            dirty: format!("hunks=[{}] before={}", actions, before_hash),
        };

        // Open the repo and perform the real Save: WT write + marker block + stage
        // (index unmerged → stage 0).  Marker residue is a HARD block here.
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", e)),
                );
                return;
            }
        };
        let buffer = match self.conflict.as_ref() {
            Some(c) => c.buffer.clone(),
            None => return,
        };
        match repo.execute_conflict_save(&buffer, path) {
            Ok(_outcome) => {
                // Staged → mark the file Resolved and re-evaluate the gate.
                if let Some(c) = self.conflict.as_mut() {
                    let _ = c.buffer.autosave();
                    let residue = c.buffer.files_with_marker_residue();
                    if let Some(f) = c.session.files.iter_mut().find(|f| f.path == path) {
                        f.status = if residue.contains(&f.path) {
                            kagi::git::ConflictStatus::NeedsReview
                        } else {
                            kagi::git::ConflictStatus::Resolved
                        };
                    }
                }
                let after = StateSummary {
                    head: format!(
                        "staged (stage 0) before={} after={}",
                        before_hash, after_hash
                    ),
                    dirty: "clean".to_string(),
                };
                self.record_op(&op_name, before, OpOutcome::Success { after }, &repo_path);
                self.conflict_editing_before_text
                    .insert(path.to_path_buf(), after_text);
                // Re-detect so the staged file leaves the conflicted index set.
                self.conflict_detected_for = None;
                self.detect_conflict_mode();
                self.push_toast(
                    ToastKind::Success,
                    SharedString::from(Msg::EditorSavedResolved.t()),
                );
            }
            Err(e) => {
                // Marker residue / write failure: hard block (ADR-0068).
                let err_msg = format!("{}", e);
                self.record_op(
                    &op_name,
                    before,
                    OpOutcome::Refused {
                        blockers: vec![err_msg],
                    },
                    &repo_path,
                );
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(Msg::EditorMarkerWarning.t()),
                );
            }
        }
    }

    /// Select a conflicting file (open its detail + Result preview).
    pub fn conflict_select_file(&mut self, idx: usize) {
        let mut open_path: Option<PathBuf> = None;
        if let Some(c) = self.conflict.as_mut() {
            if let Some(f) = c.session.files.get(idx) {
                c.selected_file = Some(idx);
                // W32: activating a content conflict opens the dedicated
                // hunk-level Conflict Editor (binary / single-sided files have no
                // hunk model and stay on the Dashboard choose UI).
                if f.kind == kagi::git::ConflictKind::Content {
                    open_path = Some(f.path.clone());
                }
            }
        }
        if let Some(p) = open_path {
            self.conflict_open_editor(&p);
        }
    }

    /// Move the selection to the previous (`dir < 0`) or next (`dir > 0`)
    /// **unresolved** file, wrapping around (KDiff3-style nav).
    pub fn conflict_nav_unresolved(&mut self, dir: i32) {
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        let n = c.session.files.len();
        if n == 0 {
            return;
        }
        let start = c.selected_file.unwrap_or(0);
        // Scan up to n positions in the requested direction for an unresolved file.
        for step in 1..=n {
            let i = if dir >= 0 {
                (start + step) % n
            } else {
                (start + n - (step % n)) % n
            };
            if c.session.files[i].status == kagi::git::ConflictStatus::Unresolved {
                c.selected_file = Some(i);
                return;
            }
        }
        // None unresolved — just step to the neighbour so nav still feels alive.
        let i = if dir >= 0 {
            (start + 1) % n
        } else {
            (start + n - 1) % n
        };
        c.selected_file = Some(i);
    }

    /// Apply a per-file side choice to the in-memory resolution buffer, then
    /// recompute that file's status.  The repository is untouched (in-memory
    /// first); the buffer is autosaved so the partial resolution survives.
    pub fn conflict_apply_choice(
        &mut self,
        path: &std::path::Path,
        choice: kagi::git::ResolutionChoice,
    ) {
        let Some(c) = self.conflict.as_mut() else {
            return;
        };
        match c.buffer.apply_choice(path, choice) {
            Ok(()) => {
                // Refresh status for this file from the buffer.
                let residue = c.buffer.files_with_marker_residue();
                if let Some(f) = c.session.files.iter_mut().find(|f| f.path == path) {
                    f.status = if residue.contains(&f.path) {
                        kagi::git::ConflictStatus::NeedsReview
                    } else {
                        kagi::git::ConflictStatus::Resolved
                    };
                }
                // Autosave (ADR-0057): never lose a partial resolution.
                let _ = c.buffer.autosave();
                eprintln!(
                    "[kagi] conflict-mode: choice {} for {}",
                    match choice {
                        kagi::git::ResolutionChoice::Current => "current",
                        kagi::git::ResolutionChoice::Incoming => "incoming",
                        kagi::git::ResolutionChoice::BothCurrentFirst => "both(current-first)",
                        kagi::git::ResolutionChoice::BothIncomingFirst => "both(incoming-first)",
                    },
                    path.display()
                );
            }
            Err(e) => {
                eprintln!(
                    "[kagi] conflict-mode: choice failed for {}: {}",
                    path.display(),
                    e
                );
                self.push_toast(ToastKind::Error, SharedString::from(format!("{}", e)));
            }
        }
    }

    /// Continue the in-progress operation (ADR-0068 routing — T-CONFLICT-FLOW-030/
    /// 032).  Gates through `plan_conflict_continue_route`, then:
    ///
    /// - **merge** → transition to the commit message panel pre-filled with the
    ///   merge message (`conflict_merge_commit_pending = true`).  **No commit is
    ///   created here** — the commit panel's commit button calls
    ///   `start_merge_commit`, which creates the 2-parent merge commit.
    /// - **rebase / cherry-pick / revert** → open the `<op> --continue`
    ///   confirmation modal (`conflict_continue_modal`); the sequencer runs only
    ///   when the user confirms (`confirm_conflict_continue`).
    pub fn conflict_continue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(mode) = self.conflict.clone() else {
            return;
        };

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", e)),
                );
                return;
            }
        };

        let op_name = format!("{}-continue", mode.session.op.slug());
        let route = match repo.plan_conflict_continue_route(
            &mode.session,
            &mode.buffer,
            &mode.current_branch,
        ) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] refused: {} blocked: {}", op_name, e);
                // Surface the specific (localized) blocking reason (ADR-0067).
                if let Some(first) = repo.continue_blockers(&mode.session, &mode.buffer).first() {
                    self.push_toast(ToastKind::Error, conflict_view::blocker_msg(first).t());
                } else {
                    self.push_toast(ToastKind::Error, SharedString::from(format!("{}", e)));
                }
                self.record_op(
                    &op_name,
                    StateSummary {
                        head: format!("op={}", mode.session.op.slug()),
                        dirty: "blocked".to_string(),
                    },
                    OpOutcome::Refused {
                        blockers: vec![format!("{}", e)],
                    },
                    &repo_path,
                );
                cx.notify();
                return;
            }
        };

        match route {
            kagi::git::ContinueRoute::MergeCommitPanel { message } => {
                // Transition to the commit message panel pre-filled with the merge
                // message.  MERGE_HEAD stays present so the commit becomes a merge
                // commit.  No commit is created here (ADR-0068).
                //
                // Stage the resolutions into the index first: the per-file Save is
                // optional, so the index may still hold unmerged entries.  Without
                // this the commit panel shows nothing staged (Commit disabled) and
                // execute_merge_commit refuses the still-conflicted index.
                if let Err(e) = repo.stage_conflict_resolution(&mode.session, &mode.buffer) {
                    eprintln!("[kagi] refused: {} stage failed: {}", op_name, e);
                    self.push_toast(
                        ToastKind::Error,
                        SharedString::from(format!("Could not stage resolution: {}", e)),
                    );
                    cx.notify();
                    return;
                }
                eprintln!(
                    "[kagi] {}: routing to commit message panel (merge)",
                    op_name
                );
                self.open_commit_panel(window, cx);
                self.commit_template_mode = false;
                if let Some(input) = self.commit_input.clone() {
                    input.update(cx, |state, cx| state.set_value(message.clone(), window, cx));
                }
                if let Some(panel) = self.commit_panel.as_mut() {
                    panel.commit_msg = message.clone();
                }
                self.conflict_merge_commit_pending = true;
            }
            kagi::git::ContinueRoute::SequencerPlan(plan) => {
                // Confirmation modal before advancing the sequencer.
                eprintln!(
                    "[kagi] {}: opening continue confirmation (sequencer)",
                    op_name
                );
                self.conflict_continue_modal = Some(ConflictContinuePlanModal {
                    plan: std::sync::Arc::new(*plan),
                    error: None,
                });
            }
        }
        cx.notify();
    }

    /// Confirm the sequencer `<op> --continue` plan (T-CONFLICT-FLOW-032): run
    /// `execute_conflict_continue` (which stages the resolution and advances the
    /// sequencer), record the oplog, drop the autosaved buffer, and reload.
    pub fn confirm_conflict_continue(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(mode) = self.conflict.clone() else {
            return;
        };
        let Some(modal) = self.conflict_continue_modal.clone() else {
            return;
        };
        let plan = modal.plan;

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", e)),
                );
                return;
            }
        };
        let op_name = format!("{}-continue", mode.session.op.slug());

        match repo.execute_conflict_continue(&mode.session, &mode.buffer) {
            Ok(_outcome) => {
                eprintln!("[kagi] executed: {}", op_name);
                let _ = kagi::git::ResolutionBuffer::clear(&repo_path);
                let after = StateSummary {
                    head: plan.predicted.head.clone(),
                    dirty: "staged".to_string(),
                };
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                self.conflict_continue_modal = None;
                self.reload();
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                eprintln!("[kagi] {} failed: {}", op_name, err_msg);
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                if let Some(modal) = self.conflict_continue_modal.as_mut() {
                    modal.error = Some(SharedString::from(err_msg));
                }
            }
        }
        cx.notify();
    }

    /// Cancel the sequencer continue confirmation modal.
    pub fn cancel_conflict_continue(&mut self) {
        self.conflict_continue_modal = None;
    }

    /// Abort the in-progress operation through the existing plan pipeline:
    /// `plan_conflict_abort` → `execute_conflict_abort` → oplog → re-detect.
    /// Abort is always available (no blockers); the partial resolution buffer is
    /// preserved by the backend (ADR-0057).
    pub fn conflict_abort(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(mode) = self.conflict.clone() else {
            return;
        };

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", e)),
                );
                return;
            }
        };

        let plan = match repo.plan_conflict_abort(&mode.session) {
            Ok(p) => p,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("abort plan error: {}", e)),
                );
                return;
            }
        };
        let op_name = format!("{}-abort", mode.session.op.slug());

        match repo.execute_conflict_abort(&mode.session, &mode.buffer) {
            Ok(_outcome) => {
                eprintln!("[kagi] executed: {}", op_name);
                let after = StateSummary {
                    head: plan.predicted.head.clone(),
                    dirty: "clean".to_string(),
                };
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                self.reload();
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                eprintln!("[kagi] {} failed: {}", op_name, err_msg);
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Failed { error: err_msg },
                    &repo_path,
                );
            }
        }
        cx.notify();
    }

    /// Two-stage Abort (ADR-0067): the first click arms the confirm, the second
    /// executes.  Surfaces the "saved resolution may be lost" warning in the UI
    /// (the dashboard shows the hint while armed).
    pub fn conflict_abort_request(&mut self, cx: &mut Context<Self>) {
        let armed = self
            .conflict
            .as_ref()
            .map(|c| c.abort_armed)
            .unwrap_or(false);
        if !armed {
            if let Some(c) = self.conflict.as_mut() {
                c.abort_armed = true;
            }
            eprintln!("[kagi] conflict-mode: abort armed (second confirm required)");
            return;
        }
        // Armed → execute (conflict_abort re-detects and rebuilds the mode).
        self.conflict_abort(cx);
    }

    /// Skip the current sequencer step (rebase / cherry-pick / revert) through
    /// the plan pipeline (T-042, ADR-0067): `plan_conflict_skip` → execute →
    /// oplog → re-detect.  Merge has no skip (the button is hidden for merge;
    /// the backend `plan_conflict_skip` also errors for merge as a guard).
    pub fn conflict_skip(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let Some(mode) = self.conflict.clone() else {
            return;
        };

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", e)),
                );
                return;
            }
        };

        let plan = match repo.plan_conflict_skip(&mode.session) {
            Ok(p) => p,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("skip plan error: {}", e)),
                );
                return;
            }
        };
        let op_name = format!("{}-skip", mode.session.op.slug());

        match repo.execute_conflict_skip(&mode.session, &mode.buffer) {
            Ok(_outcome) => {
                eprintln!("[kagi] executed: {}", op_name);
                let after = StateSummary {
                    head: plan.predicted.head.clone(),
                    dirty: "current step dropped".to_string(),
                };
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                self.reload();
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                eprintln!("[kagi] {} failed: {}", op_name, err_msg);
                self.record_op(
                    &op_name,
                    plan.current.clone(),
                    OpOutcome::Failed { error: err_msg },
                    &repo_path,
                );
            }
        }
        cx.notify();
    }

    /// Open the configured external merge tool for the selected conflict file
    /// (ADR-0060 / T-050).  Reads `settings.json` `"mergetool"` and substitutes
    /// `$LOCAL` / `$BASE` / `$REMOTE` / `$MERGED`.  If unset, shows how to
    /// configure it (we do NOT invent a default tool).  No plan needed
    /// (read-only launch); a note is recorded to the oplog footer via the toast.
    pub fn conflict_open_external_tool(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        let Some(c) = self.conflict.as_ref() else {
            return;
        };
        let Some(idx) = c.selected_file else { return };
        let Some(file) = c.session.files.get(idx) else {
            return;
        };

        let template = match theme::read_setting("mergetool") {
            Some(t) if !t.trim().is_empty() => t,
            _ => {
                self.push_toast(ToastKind::Info, Msg::ConflictExternalToolUnset.t());
                return;
            }
        };

        let workdir = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let merged = workdir.join(&file.path);
        let merged_str = merged.to_string_lossy().into_owned();
        // $LOCAL/$BASE/$REMOTE are the current/base/incoming versions; in the
        // in-memory MVP we point every side at the conflicted working-tree file
        // (which contains the markers) so external tools that re-parse markers
        // (e.g. `code --wait`, `vimdiff $MERGED`) work.  Tools needing distinct
        // side files are a v0.2 enhancement (materialize the three sides first).
        let cmd = template
            .replace("$LOCAL", &merged_str)
            .replace("$BASE", &merged_str)
            .replace("$REMOTE", &merged_str)
            .replace("$MERGED", &merged_str);

        eprintln!("[kagi] conflict-mode: launch external tool: {}", cmd);
        match std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(&workdir)
            .spawn()
        {
            Ok(_) => self.push_toast(
                ToastKind::Info,
                SharedString::from(format!("{}: {}", Msg::ConflictExternalTool.t(), merged_str)),
            ),
            Err(e) => self.push_toast(
                ToastKind::Error,
                SharedString::from(format!("external tool failed: {}", e)),
            ),
        }
    }

    /// Open the integrated terminal at the repository root (ADR-0060 / T-051).
    pub fn conflict_open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.bottom_panel_open = true;
        self.bottom_tab = BottomTab::Terminal;
        self.ensure_terminal(window, cx);
    }

    /// Copy the selected conflict file's absolute path to the clipboard
    /// (ADR-0060 / T-052).
    pub fn conflict_copy_path(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.conflict.as_ref() else {
            return;
        };
        let Some(idx) = c.selected_file else { return };
        let Some(file) = c.session.files.get(idx) else {
            return;
        };
        let abs = match self.repo_path.clone() {
            Some(p) => p.join(&file.path).to_string_lossy().into_owned(),
            None => file.path.to_string_lossy().into_owned(),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(abs.clone()));
        self.push_toast(ToastKind::Success, SharedString::from(abs));
    }

    /// Copy the git command suggestion for the current operation + intent
    /// (ADR-0060 / T-052), e.g. `git merge --continue` / `git rebase --abort` /
    /// `git rebase --skip`.
    pub fn conflict_copy_git_command(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.conflict.as_ref() else {
            return;
        };
        let slug = c.session.op.slug();
        let is_sequencer = c.session.op.is_sequencer();
        // Offer the most useful command for the current state: continue when the
        // gate is open, otherwise abort; sequencer ops also note --skip.
        let cmd = if c.can_continue() {
            format!("git {} --continue", slug)
        } else if is_sequencer {
            format!("git {} --skip   # or: git {} --abort", slug, slug)
        } else {
            format!("git {} --abort", slug)
        };
        cx.write_to_clipboard(ClipboardItem::new_string(cmd.clone()));
        self.push_toast(ToastKind::Success, SharedString::from(cmd));
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
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };

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

        let task = cx
            .background_spawn(async move { avatar_fetch::resolve_avatars(&owner, &repo, &emails) });
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

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] plan: repo open error: {}", e);
                return;
            }
        };

        match repo.plan_checkout(&branch) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: checkout {} blockers={} warnings={}",
                    branch,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.plan_modal = Some(CheckoutPlanModal {
                    stash_first: false,
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

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] checkout-commit plan: repo open error: {}", e);
                return;
            }
        };

        match repo.plan_checkout_commit(&commit_id) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: checkout-commit {} blockers={} warnings={}",
                    commit_id.short(),
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.plan_modal = Some(CheckoutPlanModal {
                    stash_first: false,
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
            localized_blockers: Vec::new(),
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
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] replan_create_branch: repo open error: {}", e);
                return;
            }
        };
        match repo.plan_create_branch_with_checkout(&name, &at, checkout_after) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: create-branch '{}' checkout_after={} blockers={} warnings={}",
                    name,
                    checkout_after,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                // W29-I18N-WAVE2: localize the keyed branch-name reasons; any
                // non-keyed plan blocker (commit-existence, checkout-after) is
                // passed through in English.
                let keyed = repo.create_branch_name_errors(&name);
                let localized = localize_plan_blockers(
                    &plan.blockers,
                    keyed
                        .iter()
                        .map(|e| (e.to_string(), crate::ui::i18n::branch_name_error(e))),
                );
                if let Some(ref mut modal) = self.create_branch_modal {
                    modal.plan = Some(std::sync::Arc::new(plan));
                    modal.localized_blockers = localized;
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
                    OpOutcome::Refused {
                        blockers: plan.blockers.clone(),
                    },
                    rp,
                );
            }
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    "create-branch",
                    plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                if let Some(ref mut m) = self.create_branch_modal {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
        };

        // Preflight check (re-use checkout preflight: verifies HEAD unchanged).
        if let Err(e) = repo.preflight_check(&plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "create-branch",
                plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            if let Some(ref mut m) = self.create_branch_modal {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        // Execute create-branch.
        if let Err(e) = repo.execute_create_branch(&modal.input, &modal.at) {
            let err_msg = format!("Create branch failed: {}", e);
            self.record_op(
                "create-branch",
                plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            if let Some(ref mut m) = self.create_branch_modal {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        eprintln!(
            "[kagi] executed: create-branch '{}' @ {}",
            modal.input,
            modal.at.short()
        );

        // Verify: confirm the branch now exists.
        let repo2 = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] verify: repo open error: {}", e);
                self.reload();
                return;
            }
        };
        let branch_exists = repo2.local_branch_exists(&modal.input);
        if branch_exists {
            eprintln!("[kagi] verified: branch '{}' exists", modal.input);
        } else {
            eprintln!(
                "[kagi] verify: branch '{}' NOT found after create",
                modal.input
            );
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
            OpOutcome::Success {
                after: create_after.clone(),
            },
            &repo_path,
        );

        if modal.checkout_after {
            let checkout_plan = match repo2.plan_checkout(&modal.input) {
                Ok(plan) => plan,
                Err(e) => {
                    let err_msg = format!("Checkout plan failed after branch creation: {}", e);
                    self.record_op(
                        "checkout",
                        create_after,
                        OpOutcome::Failed {
                            error: err_msg.clone(),
                        },
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
                    OpOutcome::Refused {
                        blockers: checkout_plan.blockers.clone(),
                    },
                    &repo_path,
                );
                if let Some(ref mut m) = self.create_branch_modal {
                    m.error = Some(SharedString::from(
                        "Branch created, but checkout was refused by the checkout plan.",
                    ));
                }
                return;
            }
            if let Err(e) = repo2.preflight_check(&checkout_plan) {
                let err_msg = format!("Checkout preflight failed: {}", e);
                self.record_op(
                    "checkout",
                    checkout_plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                if let Some(ref mut m) = self.create_branch_modal {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
            if let Err(e) = repo2.execute_checkout(&modal.input) {
                let err_msg = format!("Checkout failed: {}", e);
                self.record_op(
                    "checkout",
                    checkout_plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
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
                OpOutcome::Success {
                    after: checkout_plan.predicted.clone(),
                },
                &repo_path,
            );
        }

        // Reload display data (new branch badge should appear).
        self.reload();
    }

    // ── Create-worktree modal (T-CM-023) ─────────────────────

    pub fn open_create_worktree_modal(&mut self, at: CommitId, cx: &mut Context<Self>) {
        self.open_create_worktree_modal_prefilled(at, String::new(), false, cx);
    }

    pub fn open_create_worktree_modal_prefilled(
        &mut self,
        at: CommitId,
        branch_prefill: String,
        allow_existing_branch: bool,
        cx: &mut Context<Self>,
    ) {
        if self.modal_focus.is_none() {
            self.modal_focus = Some(cx.focus_handle());
        }
        let start_title = self.commit_title_for(&at);
        let branch_input = branch_prefill;
        let default_branch = if branch_input.is_empty() {
            "new-branch"
        } else {
            branch_input.as_str()
        };
        let path_input = self.default_worktree_path(default_branch);
        self.create_worktree_modal = Some(CreateWorktreeModal {
            at,
            start_title,
            branch_input,
            branch_state: None, // lazy (render)
            path_input,
            path_state: None, // lazy (render)
            path_touched: false,
            allow_existing_branch,
            active_field: WorktreeModalField::Branch,
            plan: None,
            error: None,
            localized_blockers: Vec::new(),
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
        let (at, branch, path, allow_existing_branch) = match self.create_worktree_modal.as_ref() {
            Some(m) => (
                m.at.clone(),
                m.branch_input.clone(),
                m.path_input.clone(),
                m.allow_existing_branch,
            ),
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] replan_create_worktree: repo open error: {}", e);
                return;
            }
        };
        let plan_result = if allow_existing_branch {
            repo.plan_open_worktree_for_branch(&branch, &path)
        } else {
            repo.plan_create_worktree(&branch, &path, &at)
        };
        match plan_result {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: create-worktree '{}' path='{}' blockers={} warnings={}",
                    branch,
                    path,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                // W29-I18N-WAVE2: localize the keyed branch-name reasons (only
                // when creating a new branch) and the keyed worktree-path reasons
                // (empty / already exists). Other blockers stay English.
                let mut keyed: Vec<(String, String)> = Vec::new();
                if !allow_existing_branch {
                    for e in repo.create_branch_name_errors(&branch) {
                        keyed.push((e.to_string(), crate::ui::i18n::branch_name_error(&e)));
                    }
                }
                if let Err(kagi::git::ops::WorktreeValidationError::Keyed(e)) =
                    repo.validate_worktree_path_keyed(&path)
                {
                    keyed.push((e.to_string(), crate::ui::i18n::worktree_path_error(&e)));
                }
                let localized = localize_plan_blockers(&plan.blockers, keyed.into_iter());
                if let Some(ref mut modal) = self.create_worktree_modal {
                    modal.plan = Some(std::sync::Arc::new(plan));
                    modal.localized_blockers = localized;
                }
            }
            Err(e) => {
                eprintln!("[kagi] plan: create-worktree error: {}", e);
            }
        }
    }

    /// W15-ASYNCOPS: UI-path create-worktree — checks out a full tree into a new
    /// linked worktree on a background thread. The headless KAGI_* path executes
    /// `execute_create_worktree` directly (no confirm_* wrapper). On failure the
    /// footer/toast carry the error (the modal is already closed, matching the
    /// stash async path).
    pub fn start_create_worktree(&mut self, cx: &mut Context<Self>) {
        // Rebuild from the latest input so a fast type-then-click can't execute
        // a stale plan.
        self.run_modal_replans();
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
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
                    OpOutcome::Refused {
                        blockers: plan.blockers.clone(),
                    },
                    rp,
                );
            }
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        self.busy_op = Some("create-worktree");
        self.create_worktree_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyCreateWorktree.t()));
        eprintln!("[kagi] async: create-worktree started");

        let branch_input = modal.branch_input.clone();
        let path_input = modal.path_input.clone();
        let at = modal.at.clone();
        let allow_existing_branch = modal.allow_existing_branch;
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let task = cx.background_spawn(async move {
            create_worktree_blocking(
                &bg_path,
                &bg_plan,
                &branch_input,
                &path_input,
                &at,
                allow_existing_branch,
            )
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok(after) => {
                        eprintln!("[kagi] async: create-worktree finished");
                        app.record_op(
                            "create-worktree",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: create-worktree failed — {}", err_msg);
                        app.record_op(
                            "create-worktree",
                            plan.current.clone(),
                            OpOutcome::Failed { error: err_msg },
                            &repo_path,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
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
        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] replan_stash_push: repo open error: {}", e);
                return;
            }
        };
        let msg_opt = if message_str.is_empty() {
            None
        } else {
            Some(message_str.as_str())
        };
        match repo.plan_stash_push(msg_opt, true) {
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
    pub fn confirm_stash_push(&mut self, cx: &mut Context<Self>) {
        // The live plan is debounced; rebuild it from the latest input so a
        // fast type-then-click can never execute a stale plan.
        self.run_modal_replans();
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.stash_push_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let plan = match modal.plan.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        if !plan.blockers.is_empty() {
            eprintln!("[kagi] refused: stash-push plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "stash-push",
                    plan.current.clone(),
                    OpOutcome::Refused {
                        blockers: plan.blockers.clone(),
                    },
                    rp,
                );
            }
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        // Stashing copies the working tree (incl. untracked) into the stash
        // — minutes on big repos. Run it on a background thread (W3 pattern)
        // so the UI stays responsive instead of appearing frozen.
        self.busy_op = Some("stash");
        self.stash_push_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyStash.t()));
        eprintln!("[kagi] async: stash-push started");

        let msg_opt = if modal.input.is_empty() {
            None
        } else {
            Some(modal.input.clone())
        };
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let task =
            cx.background_spawn(async move { stash_push_blocking(&bg_path, &bg_plan, msg_opt) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((summary, after)) => {
                        eprintln!("[kagi] async: stash-push finished");
                        app.record_op(
                            "stash-push",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "stash: {}",
                            summary
                        )));
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: stash-push failed — {}", err_msg);
                        app.record_op(
                            "stash-push",
                            plan.current.clone(),
                            OpOutcome::Failed { error: err_msg },
                            &repo_path,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
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

        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] plan: stash-apply repo open error: {}", e);
                return;
            }
        };

        match repo.plan_stash_apply(index) {
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
                    OpOutcome::Refused {
                        blockers: plan.blockers.clone(),
                    },
                    rp,
                );
            }
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    "stash-apply",
                    plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                if let Some(ref mut m) = self.stash_apply_modal {
                    m.error = Some(SharedString::from(err_msg));
                }
                return;
            }
        };

        // Preflight check (HEAD + stash count).
        if let Err(e) = repo.preflight_check_stash(&plan, plan.stash_count_at_plan()) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "stash-apply",
                plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            if let Some(ref mut m) = self.stash_apply_modal {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        // Execute stash apply (apply only — no pop, no drop).
        if let Err(e) = repo.execute_stash_apply(modal.index) {
            let err_msg = format!("Stash apply failed: {}", e);
            self.record_op(
                "stash-apply",
                plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            if let Some(ref mut m) = self.stash_apply_modal {
                m.error = Some(SharedString::from(err_msg));
            }
            return;
        }

        eprintln!("[kagi] executed: stash-apply index={}", modal.index);

        // Verify: check working tree is dirty and stash entry still exists.
        let mut repo2 = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] verify: repo open error: {}", e);
                self.reload();
                return;
            }
        };
        let after_summary = match repo2.snapshot(10_000) {
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
                    eprintln!(
                        "[kagi] verified: stash count={} (entry preserved)",
                        stash_count
                    );
                } else {
                    eprintln!(
                        "[kagi] verify: stash count={} (expected >= {})",
                        stash_count,
                        plan.stash_count_at_plan()
                    );
                }
                StateSummary {
                    head: snap.head.display(),
                    dirty: if is_dirty {
                        "dirty".to_string()
                    } else {
                        "clean".to_string()
                    },
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
            OpOutcome::Success {
                after: after_summary,
            },
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

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] cherry-pick plan: repo open error: {}", e);
                return;
            }
        };

        match repo.plan_cherry_pick(&commit_id) {
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

    /// W15-ASYNCOPS: UI-path cherry-pick — background thread + start/finish
    /// toasts. The headless KAGI_* path executes `execute_cherry_pick` directly.
    pub fn start_cherry_pick(&mut self, cx: &mut Context<Self>) {
        let modal = match self.cherry_pick_modal.clone() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: cherry-pick plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "cherry-pick",
                    modal.plan.current.clone(),
                    OpOutcome::Refused {
                        blockers: modal.plan.blockers.clone(),
                    },
                    rp,
                );
            }
            self.cherry_pick_modal = None;
            cx.notify();
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        self.busy_op = Some("cherry-pick");
        self.cherry_pick_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyCherryPick.t()));
        eprintln!("[kagi] async: cherry-pick started");

        let plan = modal.plan.clone();
        let commit_id = modal.commit_id.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_commit = commit_id.clone();
        // T-UNDOREDO-001: capture the branch + tip BEFORE the op (main thread).
        let history_before = self.head_branch_and_sha();
        let task = cx
            .background_spawn(async move { cherry_pick_blocking(&bg_path, &bg_plan, &bg_commit) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((_summary, after)) => {
                        eprintln!("[kagi] async: cherry-pick finished");
                        app.record_op(
                            "cherry-pick",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        if let (Some((branch, before)), Some((_, after_sha))) =
                            (history_before.clone(), app.head_branch_and_sha())
                        {
                            app.record_history(
                                kagi::git::OperationKind::CherryPick,
                                &branch,
                                before,
                                after_sha,
                                format!("cherry-pick {}", commit_id.short()),
                            );
                        }
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: cherry-pick failed — {}", err_msg);
                        app.record_op(
                            "cherry-pick",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.cherry_pick_modal = Some(CherryPickModal {
                            commit_id: commit_id.clone(),
                            plan: plan.clone(),
                            error: Some(SharedString::from(err_msg)),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
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

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] revert plan: repo open error: {}", e);
                return;
            }
        };

        match repo.plan_revert(&commit_id) {
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

    /// W15-ASYNCOPS: UI-path revert — background thread + start/finish toasts.
    /// The headless KAGI_* path executes `execute_revert` directly.
    pub fn start_revert(&mut self, cx: &mut Context<Self>) {
        let modal = match self.revert_modal.clone() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: revert plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "revert",
                    modal.plan.current.clone(),
                    OpOutcome::Refused {
                        blockers: modal.plan.blockers.clone(),
                    },
                    rp,
                );
            }
            self.revert_modal = None;
            cx.notify();
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        self.busy_op = Some("revert");
        self.revert_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyRevert.t()));
        eprintln!("[kagi] async: revert started");

        let plan = modal.plan.clone();
        let commit_id = modal.commit_id.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_commit = commit_id.clone();
        // T-UNDOREDO-001: capture the branch + tip BEFORE the op (main thread).
        let history_before = self.head_branch_and_sha();
        let task =
            cx.background_spawn(async move { revert_blocking(&bg_path, &bg_plan, &bg_commit) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((_summary, after)) => {
                        eprintln!("[kagi] async: revert finished");
                        app.record_op(
                            "revert",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        if let (Some((branch, before)), Some((_, after_sha))) =
                            (history_before.clone(), app.head_branch_and_sha())
                        {
                            app.record_history(
                                kagi::git::OperationKind::Revert,
                                &branch,
                                before,
                                after_sha,
                                format!("revert {}", commit_id.short()),
                            );
                        }
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: revert failed — {}", err_msg);
                        app.record_op(
                            "revert",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.revert_modal = Some(RevertModal {
                            commit_id: commit_id.clone(),
                            plan: plan.clone(),
                            error: Some(SharedString::from(err_msg)),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
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
            dismissing: None,
        });
        if self.toasts.len() > TOASTS_MAX {
            self.toasts.remove(0);
        }
    }

    /// Begin sliding a toast out (× button or auto-expiry), then remove it once
    /// the exit animation has played. Marking `dismissing` keeps the card in the
    /// tree so the slide-out is visible; a one-shot timer does the actual
    /// removal. No-op if the toast is gone or already leaving.
    pub fn start_toast_exit(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(toast) = self.toasts.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if toast.dismissing.is_some() {
            return;
        }
        toast.dismissing = Some(Instant::now());
        cx.notify();
        cx.spawn(async move |this, acx| {
            gpui::Timer::after(Duration::from_millis(TOAST_REMOVE_MS)).await;
            let _ = this.update(acx, |app, cx| {
                let before = app.toasts.len();
                app.toasts.retain(|t| t.id != id);
                if app.toasts.len() != before {
                    cx.notify();
                }
            });
        })
        .detach();
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
        if self.set_upstream_modal.is_some() {
            self.replan_set_upstream();
        }
        if self.rename_branch_modal.is_some() {
            self.replan_rename_branch();
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
            let auto = self.default_worktree_path(if branch.is_empty() {
                "new-branch"
            } else {
                &branch
            });
            if let Some(m) = self.create_worktree_modal.as_mut() {
                m.path_input = auto.clone();
                if let Some(st) = m.path_state.clone() {
                    st.update(cx, |s, cx| s.set_value(auto, window, cx));
                }
            }
            self.schedule_modal_replan(cx);
        }

        // ── Commit-message draft autosave (T-COMMIT-007 / T-COMMIT-009) ──
        // In template mode the saved message is the *assembled* plain text
        // (ADR-0042) — edits to any of the six fields are detected via the
        // assembled value changing.
        if self.commit_panel_open {
            let has_input = self.commit_input.is_some()
                || (self.commit_template_mode && self.commit_template_inputs.is_some());
            if has_input {
                let v = self.effective_commit_message(cx);
                if v != self.last_draft_value {
                    self.last_draft_value = v;
                    self.draft_save_gen = self.draft_save_gen.wrapping_add(1);
                    let gen = self.draft_save_gen;
                    let mode = if self.commit_template_mode {
                        "template"
                    } else {
                        "plain"
                    };
                    let mode = mode.to_string();
                    cx.spawn(async move |this, acx| {
                        gpui::Timer::after(Duration::from_millis(250)).await;
                        let _ = this.update(acx, |app, _cx| {
                            if app.draft_save_gen != gen {
                                return;
                            }
                            let Some(rp) = app.repo_path.clone() else {
                                return;
                            };
                            let branch = app.status_summary.branch.clone();
                            let msg = app.last_draft_value.clone();
                            if msg.trim().is_empty() {
                                let _ = kagi::git::clear_draft(&rp, &branch);
                            } else {
                                let _ = kagi::git::save_draft(&rp, &branch, &msg, &mode);
                                eprintln!("[kagi] draft: saved {}", branch);
                            }
                        });
                    })
                    .detach();
                }
            }
        }

        // ── Stash push (message) ────────────────────────────
        if let Some(m) = self.stash_push_modal.as_mut() {
            if m.input_state.is_none() {
                let st = cx
                    .new(|cx| InputState::new(window, cx).placeholder("stash message (optional)"));
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

        if let Some(m) = self.set_upstream_modal.as_mut() {
            if m.input_state.is_none() {
                let initial = m.input.clone();
                let st = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("origin/branch")
                        .default_value(initial)
                });
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

        if let Some(m) = self.rename_branch_modal.as_mut() {
            if m.input_state.is_none() {
                let initial = m.input.clone();
                let st = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("branch-name")
                        .default_value(initial)
                });
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

        // ── T-CONFLICT-UI-001/005/UX-015: Conflict Editor code editors ──
        self.sync_conflict_editor_inputs(window, cx);
    }

    /// T-CONFLICT-UI-001: lazily create / refresh the Result CodeEditor
    /// `InputState` backing the Conflict Editor (ADR-0071).
    ///
    /// `InputState` needs a `Window`, so this runs from `sync_modal_inputs`
    /// (already on the window-context render path).  A and B are row lists;
    /// `result` mirrors the assembled Result in Preview mode and is the
    /// editable surface in Edit mode (UX-015).  The text is only re-pushed when
    /// the file or the assembled content changes
    /// (tracked by `content_sig`) so an in-progress manual edit is never
    /// clobbered every frame.  When Edit mode is on we instead *pull* the
    /// Result editor's text into the buffer via `set_manual_text`.
    fn sync_conflict_editor_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Only relevant while editing a content file with a hunk model.
        let Some(path) = self.conflict_editing.clone() else {
            self.conflict_editor_inputs = None;
            return;
        };
        let Some(c) = self.conflict.as_ref() else {
            self.conflict_editor_inputs = None;
            return;
        };
        let Some(model) = c.buffer.hunk_model(&path) else {
            self.conflict_editor_inputs = None;
            return;
        };

        // Assemble the Result text block (chars/line-safe join).
        let result_text = model.assembled_text();
        let edit_mode = self.conflict_result_editing;

        // Edit mode: pull the user's edits out of the Result editor into the
        // buffer (set_manual_text), then return (do not overwrite their text).
        if edit_mode {
            if let Some(inputs) = self.conflict_editor_inputs.as_ref() {
                if inputs.path == path {
                    let edited = inputs.result.read(cx).value().to_string();
                    if edited != result_text {
                        if let Some(c) = self.conflict.as_mut() {
                            let _ = c.buffer.set_manual_text(&path, &edited);
                            let _ = c.buffer.autosave();
                            // Refresh the file status from the buffer.
                            let residue = c.buffer.files_with_marker_residue();
                            if let Some(f) = c.session.files.iter_mut().find(|f| f.path == path) {
                                f.status = if residue.contains(&f.path) {
                                    kagi::git::ConflictStatus::NeedsReview
                                } else if c.buffer.has_resolution(&path) {
                                    kagi::git::ConflictStatus::Resolved
                                } else {
                                    kagi::git::ConflictStatus::Unresolved
                                };
                            }
                        }
                    }
                    // A/B row lists never change while editing the Result; keep as-is.
                    return;
                }
            }
        }

        let sig = conflict_content_sig(&path, &result_text, edit_mode);

        // Reuse existing inputs if the path + content + mode are unchanged.
        if let Some(inputs) = self.conflict_editor_inputs.as_ref() {
            if inputs.path == path && inputs.content_sig == sig {
                return;
            }
        }

        // Build or refresh.  Create the entities once per path; otherwise reuse.
        let need_create = self
            .conflict_editor_inputs
            .as_ref()
            .map(|i| i.path != path)
            .unwrap_or(true);

        if need_create {
            let result = cx.new(|cx| InputState::new(window, cx).code_editor("text"));
            self.conflict_editor_inputs = Some(ConflictEditorInputs {
                path: path.clone(),
                result,
                content_sig: 0,
            });
        }

        if let Some(inputs) = self.conflict_editor_inputs.as_ref() {
            inputs
                .result
                .update(cx, |s, cx| s.set_value(result_text.clone(), window, cx));
        }
        if let Some(inputs) = self.conflict_editor_inputs.as_mut() {
            inputs.content_sig = sig;
        }
    }

    /// Apply a horizontal wheel delta to the graph column scroll offset.
    /// Vertical deltas are ignored (the commit list owns vertical scroll).
    fn scroll_graph_by(&mut self, delta: &gpui::ScrollDelta, cx: &mut Context<Self>) {
        let dx = match delta {
            gpui::ScrollDelta::Pixels(p) => f32::from(p.x),
            // W28: one "line" step = one zoom-scaled lane pitch.
            gpui::ScrollDelta::Lines(l) => l.x * graph_view::lane_w(),
        };
        if dx.abs() < 0.01 {
            return;
        }
        let lane_count = self.rows.first().map(|r| r.lane_count).unwrap_or(0);
        // W28: scroll content extent uses the scaled lane pitch so a fully
        // zoomed graph can still be scrolled to reveal its rightmost lanes.
        let max = (lane_count as f32 * graph_view::lane_w() - self.graph_col_w).max(0.0);
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
        cx.spawn(async move |this, acx| loop {
            gpui::Timer::after(Duration::from_millis(150)).await;
            let finished = this.update(acx, |app, cx| {
                // Begin the slide-out for any toast that has hit its lifetime;
                // `start_toast_exit` handles the removal once it has animated.
                let expiring: Vec<u64> = app
                    .toasts
                    .iter()
                    .filter(|t| t.should_start_exit())
                    .map(|t| t.id)
                    .collect();
                for id in expiring {
                    app.start_toast_exit(id, cx);
                }
                // The ticker only needs to keep watching while a toast is still
                // counting down (not yet leaving). Once every toast is either
                // gone or already sliding out, its removal timer takes over.
                let still_watching = app.toasts.iter().any(|t| t.dismissing.is_none());
                if !still_watching {
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
        })
        .detach();
    }

    /// Render the toast stack as an absolute overlay (bottom-right, above
    /// the status bar). Returns `None` when there is nothing to show.
    fn render_toasts(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        if self.toasts.is_empty() && self.busy_op.is_none() {
            return None;
        }
        let mut stack = div()
            .absolute()
            .bottom(theme::scaled_px(34.))
            .left(theme::scaled_px(12.))
            .w(theme::scaled_px(460.))
            .flex()
            .flex_col()
            .gap_2();

        // While an async op runs, show a busy snackbar with a spinning sync icon
        // (user request) — a lighter alternative to a blocking popup.
        if let Some(op) = self.busy_op {
            stack = stack.child(self.render_busy_snackbar(op));
        }

        for toast in &self.toasts {
            let (accent, glyph) = match toast.kind {
                ToastKind::Info => (theme().color_branch, "\u{27f3}"), // ⟳
                ToastKind::Success => (theme().color_success, "\u{2713}"), // ✓
                ToastKind::Error => (theme().color_blocker, "\u{2715}"), // ✕
                ToastKind::Sync => (theme().color_branch, ""),
            };
            let id = toast.id;
            let is_sync = toast.kind == ToastKind::Sync;
            // Sync toasts reuse the busy snackbar's big spinning icon (user
            // request: "already up to date" must match an in-flight op); the
            // others keep the compact text glyph.
            let icon_el: gpui::AnyElement = if is_sync {
                self.big_sync_icon(accent, ("kagi-toast-sync", id))
            } else {
                div()
                    .text_color(rgb(accent))
                    .child(SharedString::from(glyph))
                    .into_any_element()
            };
            let leaving = toast.dismissing.is_some();
            let dismiss = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
                this.start_toast_exit(id, cx);
            });
            // Explicit width so the animated margin-left slides the whole card
            // horizontally (a stretched flex child wouldn't translate cleanly).
            let card = div()
                .w(theme::scaled_px(460.))
                .flex()
                .flex_row()
                .when(is_sync, |d| d.items_center().gap_3())
                .when(!is_sync, |d| d.items_start().gap_2())
                .px_4()
                .py_3()
                .rounded(theme::scaled_px(8.))
                .bg(rgb(theme().panel))
                .border_1()
                .border_color(rgb(accent))
                .text_base()
                .text_color(rgb(theme().text_main))
                .child(div().flex_shrink_0().child(icon_el))
                .child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .child(toast.message.clone()),
                )
                .child(
                    div()
                        .id(("toast-dismiss", id))
                        .flex_shrink_0()
                        .px_1()
                        .text_color(rgb(theme().text_muted))
                        .hover(|s| s.text_color(rgb(theme().text_main)))
                        .on_click(dismiss)
                        .child(SharedString::from("\u{00d7}")),
                );

            // Slide + fade: in from the left on appear, out to the left on
            // dismiss. Keyed by toast id so the animation plays once and holds.
            use gpui::AnimationExt as _;
            let animated = if leaving {
                card.with_animation(
                    ("kagi-toast-exit", id),
                    gpui::Animation::new(Duration::from_millis(TOAST_EXIT_MS))
                        .with_easing(gpui::quadratic),
                    |el, delta| el.ml(px(-TOAST_SLIDE_PX * delta)).opacity(1.0 - delta),
                )
                .into_any_element()
            } else {
                card.with_animation(
                    ("kagi-toast-enter", id),
                    gpui::Animation::new(Duration::from_millis(TOAST_ENTER_MS))
                        .with_easing(gpui::ease_out_quint()),
                    |el, delta| el.ml(px(-TOAST_SLIDE_PX * (1.0 - delta))).opacity(delta),
                )
                .into_any_element()
            };
            stack = stack.child(animated);
        }
        Some(stack.into_any())
    }

    /// A snackbar shown while an async op runs: a continuously spinning sync
    /// icon + a friendly label (user request — a non-blocking alternative to a
    /// modal busy-spinner). Driven automatically by `busy_op`, so every async
    /// op gets one for free.
    /// The big spinning sync icon shared by the busy snackbar and the
    /// sync-flavoured no-op toasts (`ToastKind::Sync`), so every sync-icon
    /// snackbar looks identical. `key` keeps each animation instance distinct.
    fn big_sync_icon(&self, accent: u32, key: impl Into<gpui::ElementId>) -> gpui::AnyElement {
        use gpui::AnimationExt as _;
        const SPIN_MS: u64 = 700;
        gpui::svg()
            .path("icons/refresh-cw.svg")
            // ~2× the header spinner (user request) so the snackbar reads
            // clearly as "working".
            .w(theme::scaled_px(32.0))
            .h(theme::scaled_px(32.0))
            .text_color(rgb(accent))
            .with_animation(
                key,
                gpui::Animation::new(Duration::from_millis(SPIN_MS)).repeat(),
                |svg, delta| {
                    svg.with_transformation(gpui::Transformation::rotate(gpui::radians(
                        delta * std::f32::consts::TAU,
                    )))
                },
            )
            .into_any_element()
    }

    fn render_busy_snackbar(&self, op: &'static str) -> gpui::AnyElement {
        let accent = theme().color_branch;
        let icon = self.big_sync_icon(accent, "kagi-busy-snackbar-spin");
        div()
            .w(theme::scaled_px(460.))
            .flex()
            .flex_row()
            .items_center()
            // 1.5× the toast gap (8px → 12px) so the larger sync icon breathes
            // a bit more from the label (user request).
            .gap_3()
            .px_4()
            .py_3()
            .rounded(theme::scaled_px(8.))
            .bg(rgb(theme().panel))
            .border_1()
            .border_color(rgb(accent))
            .text_base()
            .text_color(rgb(theme().text_main))
            .child(div().flex_shrink_0().child(icon))
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(SharedString::from(busy_label(op))),
            )
            .into_any()
    }

    /// Render the stash graph rows (ADR-0088): one fixed row per stash, shown
    /// directly below the WIP row, in the stash colour with an inbox icon and a
    /// graph node that connects down to the stash's base commit. Left-click pops,
    /// right-click opens the stash menu (same as the sidebar).
    fn render_stash_graph_rows(
        &self,
        badge_col_w: f32,
        graph_col_w: f32,
        graph_scroll_x: f32,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let visible_lanes = graph_view::lanes_for_width(graph_col_w);
        let stash_color = theme().color_warning;
        let stash_lanes = self.stash_graph_lanes.clone();
        let rh = row_height(self.graph_compact);

        // Lanes of connected stashes rendered *above* the current row, whose
        // branch lines must keep passing straight down through this row (fixes
        // the topmost stash's line vanishing at the next stash row).
        let mut passing_lanes: Vec<usize> = Vec::new();

        self.stash_graph_rows
            .iter()
            .map(|sr| {
                let index = sr.index;
                let label = sr.label.clone();
                let msg_for_menu = sr.label.to_string();
                let mut edges: Vec<kagi::graph::GraphEdge> = passing_lanes
                    .iter()
                    .map(|&lane| kagi::graph::GraphEdge {
                        from_lane: lane,
                        to_lane: lane,
                        kind: kagi::graph::EdgeKind::Pass,
                    })
                    .collect();
                if sr.connected {
                    // This stash's own line leaves its node downward; below this
                    // row it becomes a pass-through for subsequent rows.
                    edges.push(kagi::graph::GraphEdge {
                        from_lane: sr.lane,
                        to_lane: sr.lane,
                        kind: kagi::graph::EdgeKind::OutOfNode,
                    });
                    passing_lanes.push(sr.lane);
                }
                let pop = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
                    this.open_pop_modal(index);
                    cx.notify();
                });
                let menu = cx.listener(move |this, e: &gpui::MouseDownEvent, _w, cx| {
                    this.open_stash_menu(index, msg_for_menu.clone(), e.position);
                    cx.stop_propagation();
                    cx.notify();
                });
                let (cb, cbd, ct) = theme::badge_style(stash_color);
                div()
                    .id(("stash-graph-row", index))
                    .flex()
                    .flex_row()
                    .items_center()
                    .w_full()
                    .px_3()
                    .h(px(rh))
                    .on_click(pop)
                    .on_mouse_down(gpui::MouseButton::Right, menu)
                    .hover(|s| s.bg(rgb(theme().selected)))
                    // Badge column: a yellow stash chip with an inbox icon.
                    .child(
                        div()
                            .w(theme::scaled_px(badge_col_w))
                            .flex_shrink_0()
                            .overflow_hidden()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_start()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .px_1()
                                    .rounded_sm()
                                    .bg(gpui::rgba(cb))
                                    .border_1()
                                    .border_color(gpui::rgba(cbd))
                                    .text_color(rgb(ct))
                                    .text_sm()
                                    .child(
                                        gpui::svg()
                                            .path("icons/inbox.svg")
                                            .w(theme::scaled_px(12.))
                                            .h(theme::scaled_px(12.))
                                            .text_color(rgb(ct)),
                                    )
                                    .child(SharedString::from("stash")),
                            )
                            // Connector line into the BRANCH/TAG pane toward the
                            // stash node (only when it connects to a base).
                            .when(sr.connected, |el| {
                                el.child(div().flex_1().h_full().flex().items_center().child(
                                    div().w_full().h(theme::scaled_px(1.)).bg(rgb(stash_color)),
                                ))
                            }),
                    )
                    // Inner divider spacer (badge|graph), bridged for the connector.
                    .child(
                        div()
                            .relative()
                            .w(theme::scaled_px(INNER_DIV_W))
                            .h_full()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().w(px(1.)).h_full().bg(rgb(theme().surface)))
                            .when(sr.connected, |el| {
                                el.child(div().absolute().inset_0().flex().items_center().child(
                                    div().w_full().h(theme::scaled_px(1.)).bg(rgb(stash_color)),
                                ))
                            }),
                    )
                    // Graph column: the stash node + line down to its base.
                    .child(
                        div()
                            .w(theme::scaled_px(graph_col_w))
                            .h_full()
                            .flex_shrink_0()
                            .overflow_hidden()
                            .when(visible_lanes > 0, |el| {
                                el.child(
                                    graph_view::graph_canvas(
                                        sr.lane,
                                        edges,
                                        visible_lanes,
                                        false,
                                        false,
                                        true,
                                        graph_scroll_x,
                                        stash_lanes.clone(),
                                    )
                                    .size_full(),
                                )
                            }),
                    )
                    // Inner divider spacer (graph|message).
                    .child(
                        div()
                            .w(theme::scaled_px(INNER_DIV_W))
                            .flex_shrink_0()
                            .flex()
                            .justify_center()
                            .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))),
                    )
                    // Message column: the stash label, in the stash colour.
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .truncate()
                            .text_color(rgb(stash_color))
                            .child(label),
                    )
                    .into_any()
            })
            .collect()
    }

    /// Read the current HEAD branch name + commit SHA from the open repo.
    /// Returns `None` for detached/unborn HEAD or any open/read failure — used
    /// to capture before/after snapshots for the operation-history recording
    /// (T-UNDOREDO-001). The view never holds git2 directly: this goes through
    /// the `kagi::git::Backend`.
    fn head_branch_and_sha(&self) -> Option<(String, kagi::git::CommitId)> {
        let repo_path = self.repo_path.clone()?;
        let backend = kagi::git::Backend::open(&repo_path).ok()?;
        let branch = backend.head_shorthand()?;
        let sha = backend.head_commit_id()?;
        Some((branch, sha))
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
            OpOutcome::Success { after } => (
                SharedString::from(format!("{}: {} → {}", op, before.head, after.head)),
                true,
            ),
            OpOutcome::Failed { error } => (
                SharedString::from(format!("{}: failed — {}", op, error)),
                false,
            ),
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
    /// Stash the working tree ahead of an Enter-checkout. Returns `true`
    /// when the tree is clean afterwards; on Refused/Failed the plan modal
    /// shows the error and the checkout is aborted.
    fn stash_before_checkout(&mut self) -> bool {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return false,
        };
        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                if let Some(m) = self.plan_modal.as_mut() {
                    m.error = Some(SharedString::from(format!("stash: repo open error: {}", e)));
                }
                return false;
            }
        };
        let msg = "kagi: auto-stash before checkout";
        let plan = match repo.plan_stash_push(Some(msg), true) {
            Ok(p) => p,
            Err(e) => {
                if let Some(m) = self.plan_modal.as_mut() {
                    m.error = Some(SharedString::from(format!("stash plan error: {}", e)));
                }
                return false;
            }
        };
        if !plan.blockers.is_empty() {
            eprintln!("[kagi] refused: auto-stash has blockers, checkout aborted");
            self.record_op(
                "stash-push",
                plan.current.clone(),
                OpOutcome::Refused {
                    blockers: plan.blockers.clone(),
                },
                &repo_path,
            );
            if let Some(m) = self.plan_modal.as_mut() {
                m.error = Some(SharedString::from(format!(
                    "stash refused: {}",
                    plan.blockers.join(" / ")
                )));
            }
            return false;
        }
        match repo.execute_stash_push(Some(msg), true) {
            Ok(()) => {
                eprintln!("[kagi] executed: auto-stash before checkout");
                self.record_op(
                    "stash-push",
                    plan.current.clone(),
                    OpOutcome::Success {
                        after: plan.predicted.clone(),
                    },
                    &repo_path,
                );
                // Keep status fresh so the checkout preflight sees the
                // now-clean tree.
                self.reload();
                true
            }
            Err(e) => {
                let err = format!("stash failed: {}", e);
                self.record_op(
                    "stash-push",
                    plan.current.clone(),
                    OpOutcome::Failed { error: err.clone() },
                    &repo_path,
                );
                if let Some(m) = self.plan_modal.as_mut() {
                    m.error = Some(SharedString::from(err));
                }
                false
            }
        }
    }

    pub fn confirm_checkout(&mut self) {
        let modal = match self.plan_modal.clone() {
            Some(m) => m,
            None => return,
        };
        // Enter-checkout on a dirty tree: stash the changes first (plan
        // pipeline; refused/failed stash aborts the checkout with the error
        // shown in the modal).
        if modal.stash_first && self.status_summary.is_dirty && !self.stash_before_checkout() {
            return;
        }
        // Defence in depth: the UI never renders the confirm button when
        // blockers exist, but refuse here too so no code path can execute a
        // blocked plan.
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "checkout",
                    modal.plan.current.clone(),
                    OpOutcome::Refused {
                        blockers: modal.plan.blockers.clone(),
                    },
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

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    op_name,
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.plan_modal = Some(CheckoutPlanModal {
                    stash_first: false,
                    target: modal.target.clone(),
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
                return;
            }
        };

        // Preflight check.
        if let Err(e) = repo.preflight_check(&modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                op_name,
                modal.plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            self.plan_modal = Some(CheckoutPlanModal {
                stash_first: false,
                target: modal.target.clone(),
                plan: modal.plan.clone(),
                error: Some(SharedString::from(err_msg)),
            });
            return;
        }

        // Execute checkout (safe mode only).
        let execute_result = match &modal.target {
            CheckoutPlanTarget::Branch(branch) => repo.execute_checkout(branch),
            CheckoutPlanTarget::Commit(commit_id) => repo.execute_checkout_commit(commit_id),
        };
        if let Err(e) = execute_result {
            let err_msg = format!("Checkout failed: {}", e);
            self.record_op(
                op_name,
                modal.plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            self.plan_modal = Some(CheckoutPlanModal {
                stash_first: false,
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
        let mut repo2 = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] verify: repo open error: {}", e);
                self.reload();
                return;
            }
        };
        let after_summary = match repo2.snapshot(10_000) {
            Ok(snap) => {
                match (&modal.target, &snap.head) {
                    (
                        CheckoutPlanTarget::Branch(branch),
                        Head::Attached {
                            branch: actual_branch,
                            ..
                        },
                    ) if actual_branch == branch => {
                        eprintln!("[kagi] verified: HEAD={}", actual_branch);
                    }
                    (CheckoutPlanTarget::Commit(commit_id), Head::Detached { target })
                        if target == &commit_id.0 =>
                    {
                        eprintln!("[kagi] verified: detached HEAD={}", commit_id.short());
                    }
                    other => {
                        eprintln!(
                            "[kagi] verify: unexpected HEAD state after checkout: {:?}",
                            other
                        );
                    }
                }
                StateSummary {
                    head: snap.head.display(),
                    dirty: if snap.status.is_dirty() {
                        "dirty".to_string()
                    } else {
                        "clean".to_string()
                    },
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
            OpOutcome::Success {
                after: after_summary,
            },
            &repo_path,
        );

        // Reload display data.
        self.reload();
    }

    /// W15-ASYNCOPS: UI-path checkout — runs `checkout_blocking` on a background
    /// thread so a large `checkout_tree` write never freezes the window. The
    /// headless `KAGI_CHECKOUT*` path keeps using `confirm_checkout` (sync).
    pub fn start_checkout(&mut self, cx: &mut Context<Self>) {
        let modal = match self.plan_modal.clone() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        // Enter-checkout on a dirty tree: stash the changes first (synchronous;
        // armed/two-stage style state stays on the main thread). A refused/failed
        // auto-stash aborts the checkout with the error shown in the modal.
        if modal.stash_first && self.status_summary.is_dirty && !self.stash_before_checkout() {
            return;
        }
        // Defence in depth: never execute a blocked plan.
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: plan has blockers, not executing");
            if let Some(ref rp) = self.repo_path.clone() {
                self.record_op(
                    "checkout",
                    modal.plan.current.clone(),
                    OpOutcome::Refused {
                        blockers: modal.plan.blockers.clone(),
                    },
                    rp,
                );
            }
            self.plan_modal = None;
            cx.notify();
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

        self.busy_op = Some("checkout");
        self.plan_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyCheckout.t()));
        eprintln!("[kagi] async: checkout started");

        let plan = modal.plan.clone();
        let target = modal.target.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_target = target.clone();
        let task =
            cx.background_spawn(async move { checkout_blocking(&bg_path, &bg_plan, &bg_target) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((_summary, after)) => {
                        eprintln!("[kagi] async: checkout finished");
                        app.record_op(
                            op_name,
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: checkout failed — {}", err_msg);
                        app.record_op(
                            op_name,
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.plan_modal = Some(CheckoutPlanModal {
                            stash_first: false,
                            target: target.clone(),
                            plan: plan.clone(),
                            error: Some(SharedString::from(err_msg)),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ── T-HT-003: Pull ────────────────────────────────────────

    /// Build a pull plan and open the confirmation modal.
    pub fn open_pull_modal(&mut self) {
        // W3-NOTIFY: refuse while a background op runs.
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "pull: repo open error: {}",
                    e
                )));
                return;
            }
        };
        match repo.plan_pull() {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: pull blockers={} warnings={}",
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                // Already-up-to-date pull (nothing to pull by local knowledge)
                // is not worth a blocking popup (user request): snackbar instead.
                // Background auto-fetch keeps the behind count fresh; the title
                // carries the "up to date (local knowledge…)" behind-label from
                // ops::plan_pull when behind == 0.
                if plan.blockers.is_empty()
                    && plan.warnings.is_empty()
                    && plan.title.contains("up to date (local knowledge")
                {
                    self.push_toast(
                        ToastKind::Sync,
                        SharedString::from(Msg::AlreadyUpToDatePull.t()),
                    );
                    self.status_footer = FooterStatus::Idle(SharedString::from(""));
                    return;
                }
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
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
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
                    OpOutcome::Success {
                        after: after_summary,
                    },
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
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
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
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
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
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.pull_modal = None;
            cx.notify();
            return;
        }

        self.busy_op = Some("pull");
        self.pull_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyPull.t()));
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
                    OpOutcome::Success {
                        after: after_summary,
                    },
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
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "push: repo open error: {}",
                    e
                )));
                return;
            }
        };
        match repo.plan_push() {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: push blockers={} warnings={} preview_commits={}",
                    plan.blockers.len(),
                    plan.warnings.len(),
                    plan.preview_commits.len(),
                );
                // No-op push (already up to date — nothing to push) is not worth
                // a blocking popup (user request): show a snackbar instead. The
                // "nothing to push" blocker is the *only* blocker in this case
                // (see ops::plan_push step 6).
                if !plan.blockers.is_empty()
                    && plan.blockers.iter().all(|b| b.contains("nothing to push"))
                {
                    self.push_toast(
                        ToastKind::Sync,
                        SharedString::from(Msg::AlreadyUpToDatePush.t()),
                    );
                    self.status_footer = FooterStatus::Idle(SharedString::from(""));
                    return;
                }
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
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
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
                    OpOutcome::Success {
                        after: after_summary,
                    },
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
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
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
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
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
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.push_modal = None;
            cx.notify();
            return;
        }

        self.busy_op = Some("push");
        self.push_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyPush.t()));
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
                    OpOutcome::Success {
                        after: after_summary,
                    },
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

    pub fn open_branch_plan_modal(&mut self, branch_name: String, kind: BranchPlanKind) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "branch operation: repo open error: {}",
                    e
                )));
                return;
            }
        };
        let plan_result = match kind {
            BranchPlanKind::PullFfOnly => repo.plan_pull_branch_ff(&branch_name),
            BranchPlanKind::Push => repo.plan_push_branch(&branch_name, false),
            BranchPlanKind::PushSetUpstream => repo.plan_push_branch(&branch_name, true),
        };
        match plan_result {
            Ok(plan) => {
                self.branch_plan_modal = Some(BranchPlanModal {
                    kind,
                    branch_name,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "branch operation plan error: {}",
                    e
                )));
            }
        }
    }

    pub fn cancel_branch_plan_modal(&mut self) {
        self.branch_plan_modal = None;
    }

    pub fn start_branch_plan(&mut self, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.branch_plan_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let op_name = match modal.kind {
            BranchPlanKind::PullFfOnly => "branch-pull-ff",
            BranchPlanKind::Push => "branch-push",
            BranchPlanKind::PushSetUpstream => "branch-push-set-upstream",
        };
        if !modal.plan.blockers.is_empty() {
            self.record_op(
                op_name,
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.branch_plan_modal = None;
            cx.notify();
            return;
        }

        self.busy_op = Some(op_name);
        self.branch_plan_modal = None;
        self.status_footer =
            FooterStatus::Busy(SharedString::from(format!("{} in progress...", op_name)));
        let bg_path = repo_path.clone();
        let bg_modal = modal.clone();
        let task = cx.background_spawn(async move { branch_plan_blocking(&bg_path, &bg_modal) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok(after) => {
                        app.record_op(
                            op_name,
                            modal.plan.current.clone(),
                            OpOutcome::Success {
                                after: after.clone(),
                            },
                            &repo_path,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "{}: {}",
                            op_name, after.dirty
                        )));
                        app.reload();
                    }
                    Err(err_msg) => {
                        app.record_op(
                            op_name,
                            modal.plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.branch_plan_modal = Some(BranchPlanModal {
                            kind: modal.kind.clone(),
                            branch_name: modal.branch_name.clone(),
                            plan: modal.plan.clone(),
                            error: Some(SharedString::from(err_msg)),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn open_set_upstream_modal(&mut self, branch_name: String) {
        let input = self
            .branch_upstream_info
            .get(&branch_name)
            .map(|u| u.remote_branch.clone())
            .unwrap_or_else(|| format!("origin/{}", branch_name));
        self.set_upstream_modal = Some(SetUpstreamModal {
            branch_name,
            input,
            input_state: None,
            plan: None,
            error: None,
        });
        self.replan_set_upstream();
    }

    pub fn cancel_set_upstream_modal(&mut self) {
        self.set_upstream_modal = None;
    }

    fn replan_set_upstream(&mut self) {
        let (branch_name, input) = match self.set_upstream_modal.as_ref() {
            Some(m) => (m.branch_name.clone(), m.input.clone()),
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };
        match repo.plan_set_upstream(&branch_name, &input) {
            Ok(plan) => {
                if let Some(m) = self.set_upstream_modal.as_mut() {
                    m.plan = Some(std::sync::Arc::new(plan));
                }
            }
            Err(e) => {
                if let Some(m) = self.set_upstream_modal.as_mut() {
                    m.error = Some(SharedString::from(format!(
                        "Set upstream plan error: {}",
                        e
                    )));
                }
            }
        }
    }

    pub fn start_set_upstream(&mut self, cx: &mut Context<Self>) {
        self.run_modal_replans();
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.set_upstream_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let plan = match modal.plan.clone() {
            Some(p) => p,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !plan.blockers.is_empty() {
            self.record_op(
                "set-upstream",
                plan.current.clone(),
                OpOutcome::Refused {
                    blockers: plan.blockers.clone(),
                },
                &repo_path,
            );
            return;
        }

        self.busy_op = Some("set-upstream");
        self.set_upstream_modal = None;
        let branch_name = modal.branch_name.clone();
        let upstream = modal.input.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let task = cx.background_spawn(async move {
            set_upstream_blocking(&bg_path, &bg_plan, &branch_name, &upstream)
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok(after) => {
                        app.record_op(
                            "set-upstream",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.reload();
                    }
                    Err(err_msg) => {
                        app.record_op(
                            "set-upstream",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.set_upstream_modal = Some(SetUpstreamModal {
                            branch_name: modal.branch_name.clone(),
                            input: modal.input.clone(),
                            input_state: None,
                            plan: Some(plan.clone()),
                            error: Some(SharedString::from(err_msg)),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn open_rename_branch_modal(&mut self, branch_name: String) {
        let existing: Vec<String> = self.branches.iter().map(|(name, _)| name.clone()).collect();
        let validation = validate_branch_rename(&branch_name, &branch_name, &existing);
        self.rename_branch_modal = Some(RenameBranchModal {
            old_name: branch_name.clone(),
            input: branch_name,
            input_state: None,
            validation,
            plan: None,
            error: None,
        });
        self.replan_rename_branch();
    }

    pub fn cancel_rename_branch_modal(&mut self) {
        self.rename_branch_modal = None;
    }

    fn replan_rename_branch(&mut self) {
        let (old_name, input) = match self.rename_branch_modal.as_ref() {
            Some(m) => (m.old_name.clone(), m.input.clone()),
            None => return,
        };
        let existing: Vec<String> = self.branches.iter().map(|(name, _)| name.clone()).collect();
        let validation = validate_branch_rename(&old_name, &input, &existing);
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };
        match repo.plan_rename_branch(&old_name, &input) {
            Ok(plan) => {
                if let Some(m) = self.rename_branch_modal.as_mut() {
                    m.validation = validation;
                    m.plan = Some(std::sync::Arc::new(plan));
                }
            }
            Err(e) => {
                if let Some(m) = self.rename_branch_modal.as_mut() {
                    m.validation = validation;
                    m.error = Some(SharedString::from(format!("Rename plan error: {}", e)));
                }
            }
        }
    }

    pub fn start_rename_branch(&mut self, cx: &mut Context<Self>) {
        self.run_modal_replans();
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.rename_branch_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let plan = match modal.plan.clone() {
            Some(p) => p,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !plan.blockers.is_empty() {
            self.record_op(
                "rename-branch",
                plan.current.clone(),
                OpOutcome::Refused {
                    blockers: plan.blockers.clone(),
                },
                &repo_path,
            );
            return;
        }
        self.busy_op = Some("rename-branch");
        self.rename_branch_modal = None;
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let old_name = modal.old_name.clone();
        let new_name = modal.input.clone();
        let task = cx.background_spawn(async move {
            rename_branch_blocking(&bg_path, &bg_plan, &old_name, &new_name)
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok(after) => {
                        app.record_op(
                            "rename-branch",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.reload();
                    }
                    Err(err_msg) => {
                        app.record_op(
                            "rename-branch",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.rename_branch_modal = Some(RenameBranchModal {
                            old_name: modal.old_name.clone(),
                            input: modal.input.clone(),
                            input_state: None,
                            validation: modal.validation.clone(),
                            plan: Some(plan.clone()),
                            error: Some(SharedString::from(err_msg)),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ── T-BCM-030/T-BCM-061: Branch menu plans ───────────────

    pub fn open_merge_modal(&mut self, target: String, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // Current (checked-out) branch = the merge destination, captured on the
        // main thread for the modal's into-branch label (ADR-0079).
        let into_branch = self
            .branches
            .iter()
            .find(|(_, is_head)| *is_head)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| "HEAD".to_string());

        // Planning a merge runs an in-memory merge (conflict dry-run) which is
        // heavy on large repos — do it off the UI thread so the window doesn't
        // freeze. `busy_op` drives the spinning sync icon + blocks re-entry.
        self.busy_op = Some("merge-plan");
        self.status_footer = FooterStatus::Busy(SharedString::from("Planning merge…"));
        eprintln!("[kagi] async: merge plan started for {}", target);
        let bg_path = repo_path.clone();
        let bg_target = target.clone();
        let task = cx.background_spawn(async move {
            let repo =
                kagi::git::Backend::open(&bg_path).map_err(|e| format!("repo open error: {e}"))?;
            repo.plan_merge_branch(&bg_target)
                .map_err(|e| format!("{e}"))
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((plan, kind)) => {
                        eprintln!(
                            "[kagi] plan: merge {} blockers={} warnings={} preview_files={} kind={:?}",
                            target,
                            plan.blockers.len(),
                            plan.warnings.len(),
                            plan.preview_files.len(),
                            kind
                        );
                        app.status_footer = FooterStatus::Idle(SharedString::from(""));
                        app.merge_modal = Some(MergePlanModal {
                            target,
                            into_branch,
                            plan: std::sync::Arc::new(plan),
                            kind,
                            error: None,
                        });
                    }
                    Err(e) => {
                        app.status_footer = FooterStatus::Failed(SharedString::from(format!(
                            "merge plan error: {}",
                            e
                        )));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn cancel_merge_modal(&mut self) {
        self.merge_modal = None;
    }

    /// T-DNDMERGE-001 / ADR-0079 layer 2: the single entry point a branch
    /// drag-and-drop dispatches to.  `source` is the dragged branch (the merge
    /// source = the branch merged INTO HEAD) — a local branch name, or a
    /// remote-tracking ref like `origin/feature` for an upstream-only branch,
    /// which the planner resolves directly (no local branch is created).  This
    /// validates the obvious rejections (busy / not a branch / dropping the
    /// current branch onto itself) and, on success, delegates to the merge
    /// pipeline via
    /// [`open_merge_modal`] — it never touches git directly (the safety
    /// thesis: drop is a trigger; `plan_merge_branch` remains authoritative for
    /// dirty-WT / ff / conflict prediction).
    pub fn start_merge_from_drag(&mut self, source: String, cx: &mut Context<Self>) {
        let remotes: Vec<String> = self
            .remote_branches
            .iter()
            .map(|rb| format!("{}/{}", rb.remote, rb.name))
            .collect();
        match validate_merge_from_drag(&source, &self.branches, &remotes, self.busy_op.is_some()) {
            Ok(()) => {
                eprintln!(
                    "[kagi] drag-merge: start merge from drag — source={}",
                    source
                );
                self.open_merge_modal(source, cx);
            }
            Err(reason) => {
                eprintln!("[kagi] drag-merge: rejected — {}", reason);
                self.status_footer = FooterStatus::Idle(SharedString::from(reason));
            }
        }
        cx.notify();
    }

    pub fn start_merge(&mut self, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.merge_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: merge plan has blockers, not executing");
            self.record_op(
                "merge",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.merge_modal = None;
            cx.notify();
            return;
        }

        self.busy_op = Some("merge");
        self.merge_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyMerge.t()));
        eprintln!("[kagi] async: merge started");

        let plan = modal.plan.clone();
        let target = modal.target.clone();
        let kind = modal.kind.clone();
        let bg_path = repo_path.clone();
        let history_target = modal.target.clone();
        // T-UNDOREDO-001: capture the branch + tip BEFORE the merge (main thread).
        let history_before = self.head_branch_and_sha();
        let task =
            cx.background_spawn(async move { merge_blocking(&bg_path, &plan, &target, &kind) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((summary, after)) => {
                        eprintln!("[kagi] async: merge finished — {}", summary);
                        app.record_op(
                            "merge",
                            modal.plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        // Record for undo/redo only when the merge actually moved
                        // the branch ref (clean merge / fast-forward). A merge
                        // left in conflict has not moved HEAD, so before==after
                        // and record_history is a no-op.
                        if let (Some((branch, before)), Some((_, after_sha))) =
                            (history_before.clone(), app.head_branch_and_sha())
                        {
                            app.record_history(
                                kagi::git::OperationKind::Merge,
                                &branch,
                                before,
                                after_sha,
                                format!("merge {}", history_target),
                            );
                        }
                        // reload() resets the conflict-mode detection guard and
                        // re-runs detect_conflict_mode(); a merge that left
                        // conflict markers (MergeKind::Conflicts) therefore enters
                        // Conflict Mode here. Non-conflict merges stay Normal.
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: merge failed — {}", err_msg);
                        app.record_op(
                            "merge",
                            modal.plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.merge_modal = Some(MergePlanModal {
                            target: modal.target.clone(),
                            into_branch: modal.into_branch.clone(),
                            plan: modal.plan.clone(),
                            kind: modal.kind.clone(),
                            error: Some(SharedString::from(err_msg)),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn open_tracking_checkout_modal(&mut self, remote_branch: String) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "checkout tracking: repo open error: {}",
                    e
                )));
                return;
            }
        };
        let local_branch = default_tracking_branch_name(&remote_branch);
        match repo.plan_checkout_tracking_branch(&remote_branch, &local_branch) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: checkout-tracking {} -> {} blockers={} warnings={}",
                    remote_branch,
                    local_branch,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.tracking_checkout_modal = Some(TrackingCheckoutPlanModal {
                    remote_branch,
                    local_branch,
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "checkout tracking plan error: {}",
                    e
                )));
            }
        }
    }

    pub fn cancel_tracking_checkout_modal(&mut self) {
        self.tracking_checkout_modal = None;
    }

    pub fn start_tracking_checkout(&mut self, cx: &mut Context<Self>) {
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let modal = match self.tracking_checkout_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: checkout-tracking plan has blockers, not executing");
            self.record_op(
                "checkout-tracking",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.tracking_checkout_modal = None;
            cx.notify();
            return;
        }

        self.busy_op = Some("checkout");
        self.tracking_checkout_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyCheckout.t()));
        eprintln!("[kagi] async: checkout-tracking started");

        let plan = modal.plan.clone();
        let remote_branch = modal.remote_branch.clone();
        let local_branch = modal.local_branch.clone();
        let bg_path = repo_path.clone();
        let task = cx.background_spawn(async move {
            checkout_tracking_blocking(&bg_path, &plan, &remote_branch, &local_branch)
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((summary, after)) => {
                        eprintln!("[kagi] async: checkout-tracking finished — {}", summary);
                        app.record_op(
                            "checkout-tracking",
                            modal.plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: checkout-tracking failed — {}", err_msg);
                        app.record_op(
                            "checkout-tracking",
                            modal.plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.tracking_checkout_modal = Some(TrackingCheckoutPlanModal {
                            remote_branch: modal.remote_branch.clone(),
                            local_branch: modal.local_branch.clone(),
                            plan: modal.plan.clone(),
                            error: Some(SharedString::from(err_msg)),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ── T-HT-009: Undo Commit / T-HT-007: Stash Pop ──────────

    /// Build an undo-commit plan and open the confirmation modal.
    pub fn open_undo_modal(&mut self) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "undo: repo open error: {}",
                    e
                )));
                return;
            }
        };
        match repo.plan_undo_commit() {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: undo blockers={} warnings={}",
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.undo_modal = Some(UndoPlanModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("undo plan error: {}", e)));
            }
        }
    }

    pub fn cancel_undo_modal(&mut self) {
        self.undo_modal = None;
    }

    /// Confirm undo: preflight → execute (ref-only) → oplog → reload.
    pub fn confirm_undo(&mut self) {
        let modal = match self.undo_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: undo plan has blockers, not executing");
            self.record_op(
                "undo-commit",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            return;
        }
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    "undo-commit",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.undo_modal = Some(UndoPlanModal {
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
                return;
            }
        };
        if let Err(e) = repo.preflight_check(&modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "undo-commit",
                modal.plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            self.undo_modal = Some(UndoPlanModal {
                plan: modal.plan.clone(),
                error: Some(SharedString::from(err_msg)),
            });
            return;
        }
        match repo.execute_undo_commit() {
            Ok(outcome) => {
                eprintln!(
                    "[kagi] executed: undo {} -> now at {}",
                    outcome.undone.short(),
                    outcome.now_at.short()
                );
                self.undo_modal = None;
                let after = StateSummary {
                    head: format!("branch @ {}", outcome.now_at.short()),
                    dirty: "changes staged".to_string(),
                };
                self.record_op(
                    "undo-commit",
                    modal.plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                // T-UNDOREDO-001: record so the undo-commit itself is redoable
                // (entry.before = undone commit, entry.after = parent). An undo
                // of THIS entry re-applies the commit; a redo undoes it again.
                if let Some((branch, _)) = self.head_branch_and_sha() {
                    self.record_history(
                        kagi::git::OperationKind::UndoCommit,
                        &branch,
                        outcome.undone.clone(),
                        outcome.now_at.clone(),
                        format!("undo-commit {}", outcome.undone.short()),
                    );
                }
                self.status_footer = FooterStatus::Success(SharedString::from(format!(
                    "undo: {} (restore: git reset --soft {})",
                    outcome.undone.short(),
                    outcome.undone.short()
                )));
                self.reload();
            }
            Err(e) => {
                let err_msg = format!("Undo failed: {}", e);
                self.record_op(
                    "undo-commit",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.undo_modal = Some(UndoPlanModal {
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                });
            }
        }
    }

    // ── Operation Undo / Redo (T-UNDOREDO-001, ADR-0081) ─────

    /// ADR-0084: hydrate the in-session [`OperationHistory`] from the current
    /// branch's reflog when it is **empty** (freshly-opened repo, or after a
    /// branch switch which clears the per-repo stack). This makes Cmd+Z work
    /// immediately, even on operations performed outside this session.
    ///
    /// Only seeds when empty — an in-session stack (with precise summaries) is
    /// never clobbered. Reflog read failures are logged and ignored (best-effort).
    fn seed_history_from_reflog(&mut self, backend: &kagi::git::Backend) {
        if self.operation_history.len() != 0 {
            return;
        }
        match backend.history_from_reflog() {
            Ok(entries) => {
                if !entries.is_empty() {
                    eprintln!(
                        "[kagi] history: seeded {} entries from reflog",
                        entries.len()
                    );
                    self.operation_history = kagi::git::OperationHistory::seeded(entries);
                }
            }
            Err(e) => {
                eprintln!("[kagi] history: reflog seed failed: {}", e);
            }
        }
    }

    /// Record a successful ref-moving operation into the in-session
    /// [`OperationHistory`]. `before`/`after` are the branch tip SHAs around the
    /// operation; recording truncates any redo tail (standard undo-stack).
    ///
    /// No-op when the SHAs are identical (e.g. a no-op fast-forward) or when the
    /// branch name is empty (detached HEAD ops are not undoable in MVP).
    pub fn record_history(
        &mut self,
        kind: kagi::git::OperationKind,
        branch: &str,
        before: kagi::git::CommitId,
        after: kagi::git::CommitId,
        summary: impl Into<String>,
    ) {
        if branch.is_empty() || before == after {
            return;
        }
        let summary = summary.into();
        eprintln!(
            "[kagi] history: record {} on '{}' {} → {}",
            kind.slug(),
            branch,
            before.short(),
            after.short()
        );
        self.operation_history.record(kagi::git::HistoryEntry {
            kind,
            branch: branch.to_string(),
            before,
            after,
            summary,
        });
    }

    /// Open the Undo plan modal for the entry at the history cursor (the most
    /// recent applied operation). Builds a [`Backend::plan_undo`] preview.
    pub fn open_history_undo_modal(&mut self) {
        let entry = match self.operation_history.peek_undo().cloned() {
            Some(e) => e,
            None => {
                self.status_footer = FooterStatus::Idle(SharedString::from(Msg::NothingToUndo.t()));
                return;
            }
        };
        self.open_history_modal(entry, true);
    }

    /// Open the Redo plan modal for the entry just past the cursor.
    pub fn open_history_redo_modal(&mut self) {
        let entry = match self.operation_history.peek_redo().cloned() {
            Some(e) => e,
            None => {
                self.status_footer = FooterStatus::Idle(SharedString::from(Msg::NothingToRedo.t()));
                return;
            }
        };
        self.open_history_modal(entry, false);
    }

    /// Shared: build an undo/redo plan for `entry` and show the preview modal.
    fn open_history_modal(&mut self, entry: kagi::git::HistoryEntry, is_undo: bool) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "{}: repo open error: {}",
                    if is_undo { "undo" } else { "redo" },
                    e
                )));
                return;
            }
        };
        let plan_res = if is_undo {
            repo.plan_undo(&entry)
        } else {
            repo.plan_redo(&entry)
        };
        match plan_res {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: {} {} blockers={} warnings={}",
                    if is_undo { "undo" } else { "redo" },
                    entry.kind.slug(),
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.history_modal = Some(HistoryPlanModal {
                    plan: std::sync::Arc::new(plan),
                    entry,
                    is_undo,
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "{}: plan error: {}",
                    if is_undo { "undo" } else { "redo" },
                    e
                )));
            }
        }
    }

    /// Confirm the open Undo/Redo modal: run preflight + execute via the safe
    /// pipeline, advance/retreat the history cursor, record in the oplog, and
    /// reload. On a stale entry (preflight failure) the entry is left in place
    /// and the error is surfaced.
    pub fn confirm_history(&mut self) {
        let modal = match self.history_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let op_name = if modal.is_undo {
            format!("undo-{}", modal.entry.kind.slug())
        } else {
            format!("redo-{}", modal.entry.kind.slug())
        };

        if !modal.plan.blockers.is_empty() {
            self.record_op(
                &op_name,
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.history_modal = None;
            return;
        }

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    &op_name,
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.history_modal = Some(HistoryPlanModal {
                    error: Some(SharedString::from(err_msg)),
                    ..modal
                });
                return;
            }
        };

        if let Err(e) = repo.preflight_check(&modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                &op_name,
                modal.plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            self.history_modal = Some(HistoryPlanModal {
                error: Some(SharedString::from(err_msg)),
                ..modal
            });
            return;
        }

        let exec_res = if modal.is_undo {
            repo.execute_undo(&modal.entry)
        } else {
            repo.execute_redo(&modal.entry)
        };

        match exec_res {
            Ok(outcome) => {
                // Advance/retreat the cursor only after the ref move succeeds.
                if modal.is_undo {
                    self.operation_history.undo();
                } else {
                    self.operation_history.redo();
                }
                self.history_modal = None;
                let after = StateSummary {
                    head: format!("branch '{}' @ {}", outcome.branch, outcome.to.short()),
                    dirty: "index reset to target (working tree preserved)".to_string(),
                };
                self.record_op(
                    &op_name,
                    modal.plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                self.status_footer = FooterStatus::Success(SharedString::from(format!(
                    "{}: {} → {} (recover: git reflog)",
                    op_name,
                    outcome.from.short(),
                    outcome.to.short()
                )));
                self.reload();
            }
            Err(e) => {
                let err_msg = format!(
                    "{} failed: {}",
                    if modal.is_undo { "Undo" } else { "Redo" },
                    e
                );
                self.record_op(
                    &op_name,
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.history_modal = Some(HistoryPlanModal {
                    error: Some(SharedString::from(err_msg)),
                    ..modal
                });
            }
        }
    }

    // ── Amend (T-COMMIT-011, ADR-0040) ───────────────────────

    /// Build an amend plan for `mode` and open the confirmation modal.
    ///
    /// The new message is read from the commit input (UI path) or the commit
    /// panel's `commit_msg` (headless path).  For [`AmendMode::Staged`] the
    /// message is ignored by the backend.
    ///
    /// Entry point for the Commit Panel "Amend" control — wired by the PM when
    /// the W14-PREVIEW/TEMPLATE commit-panel lanes merge (this lane owns the
    /// backend + modal/confirm plumbing, not `commit_panel.rs`).
    pub fn open_amend_modal(&mut self, mode: AmendMode, cx: &mut Context<Self>) {
        let message: String = if let Some(ref input_entity) = self.commit_input {
            input_entity.read(cx).value().to_string()
        } else {
            self.commit_panel
                .as_ref()
                .map(|p| p.commit_msg.clone())
                .unwrap_or_default()
        };
        self.open_amend_modal_with_message(mode, message);
    }

    /// Build an amend plan from an explicit `message` (no `Context` needed).
    /// Used by the headless `KAGI_AMEND` path and by [`open_amend_modal`].
    pub fn open_amend_modal_with_message(&mut self, mode: AmendMode, message: String) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "amend: repo open error: {}",
                    e
                )));
                return;
            }
        };
        let msg_opt = if message.trim().is_empty() {
            None
        } else {
            Some(message.as_str())
        };
        match repo.plan_amend(mode, msg_opt) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: amend mode={:?} blockers={} warnings={} destructive={}",
                    mode,
                    plan.blockers.len(),
                    plan.warnings.len(),
                    plan.destructive
                );
                self.amend_modal = Some(AmendPlanModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    mode,
                    message,
                    confirm_armed: false,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("amend plan error: {}", e)));
            }
        }
    }

    /// Cancel the amend modal (also disarms the two-stage confirm).
    pub fn cancel_amend_modal(&mut self) {
        self.amend_modal = None;
    }

    /// First stage of the two-stage confirm: arm the action.  If already armed
    /// this is the final stage and executes the amend (ADR-0023 history-rewrite).
    pub fn confirm_amend(&mut self) {
        let modal = match self.amend_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        // Defence: never execute with blockers present.
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: amend plan has blockers, not executing");
            self.record_op(
                "amend",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            return;
        }

        // ── Two-stage confirm: first click only arms ─────────
        if !modal.confirm_armed {
            self.amend_modal = Some(AmendPlanModal {
                confirm_armed: true,
                ..modal
            });
            eprintln!("[kagi] amend: armed (second confirm required — history rewrite)");
            return;
        }

        // ── Armed: proceed to preflight → execute ────────────
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    "amend",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.amend_modal = Some(AmendPlanModal {
                    error: Some(SharedString::from(err_msg)),
                    ..modal
                });
                return;
            }
        };
        if let Err(e) = repo.preflight_check(&modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "amend",
                modal.plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            self.amend_modal = Some(AmendPlanModal {
                error: Some(SharedString::from(err_msg)),
                ..modal
            });
            return;
        }

        // ADR-0040: record the OLD HEAD SHA in the oplog BEFORE execution.
        // `record_op` writes the before-state; the success record below captures
        // the new HEAD so the旧→新 transition is fully logged.
        let msg_opt = if modal.message.trim().is_empty() {
            None
        } else {
            Some(modal.message.as_str())
        };
        match repo.execute_amend(modal.mode, msg_opt) {
            Ok(outcome) => {
                eprintln!(
                    "[kagi] executed: amend {} -> {}",
                    outcome.old.short(),
                    outcome.new.short()
                );
                self.amend_modal = None;
                let after = StateSummary {
                    head: format!(
                        "branch @ {} (was {})",
                        outcome.new.short(),
                        outcome.old.short()
                    ),
                    dirty: "amended".to_string(),
                };
                self.record_op(
                    "amend",
                    modal.plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                // T-UNDOREDO-001: undo of an amend moves the branch from the new
                // commit back to the pre-amend commit (still in the reflog).
                if let Some((branch, _)) = self.head_branch_and_sha() {
                    self.record_history(
                        kagi::git::OperationKind::Amend,
                        &branch,
                        outcome.old.clone(),
                        outcome.new.clone(),
                        format!("amend {} → {}", outcome.old.short(), outcome.new.short()),
                    );
                }
                self.status_footer = FooterStatus::Success(SharedString::from(format!(
                    "amend: {} → {} (restore: git reset --hard {})",
                    outcome.old.short(),
                    outcome.new.short(),
                    outcome.old.short()
                )));
                self.reload();
            }
            Err(e) => {
                let err_msg = format!("Amend failed: {}", e);
                self.record_op(
                    "amend",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.amend_modal = Some(AmendPlanModal {
                    error: Some(SharedString::from(err_msg)),
                    ..modal
                });
            }
        }
    }

    /// W15-ASYNCOPS: UI-path amend. The two-stage confirm (armed state) stays on
    /// the main thread; only the final armed execute (history rewrite — tree
    /// build + commit replace) runs on a background thread. Headless keeps
    /// `confirm_amend` (sync).
    pub fn start_amend(&mut self, cx: &mut Context<Self>) {
        let modal = match self.amend_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };

        // Defence: never execute with blockers present.
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: amend plan has blockers, not executing");
            self.record_op(
                "amend",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            return;
        }

        // First click only arms (main thread) — matches confirm_amend exactly.
        if !modal.confirm_armed {
            self.amend_modal = Some(AmendPlanModal {
                confirm_armed: true,
                ..modal
            });
            eprintln!("[kagi] amend: armed (second confirm required — history rewrite)");
            return;
        }

        // Armed → background execute. Refuse a concurrent background op.
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }

        self.busy_op = Some("amend");
        self.amend_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyAmend.t()));
        eprintln!("[kagi] async: amend started");

        let plan = modal.plan.clone();
        let mode = modal.mode;
        let message = modal.message.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_msg = message.clone();
        let task =
            cx.background_spawn(async move { amend_blocking(&bg_path, &bg_plan, mode, &bg_msg) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((after, old, new)) => {
                        eprintln!("[kagi] async: amend finished");
                        app.record_op(
                            "amend",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "amend: {} → {} (restore: git reset --hard {})",
                            old.short(),
                            new.short(),
                            old.short()
                        )));
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: amend failed — {}", err_msg);
                        app.record_op(
                            "amend",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.amend_modal = Some(AmendPlanModal {
                            plan: plan.clone(),
                            error: Some(SharedString::from(err_msg)),
                            mode,
                            message: message.clone(),
                            confirm_armed: false,
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Build a stash-pop plan and open the confirmation modal.
    pub fn open_pop_modal(&mut self, index: usize) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "pop: repo open error: {}",
                    e
                )));
                return;
            }
        };
        match repo.plan_stash_pop(index) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: stash-pop index={} blockers={} warnings={}",
                    index,
                    plan.blockers.len(),
                    plan.warnings.len()
                );
                self.pop_modal = Some(PopPlanModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    stash_index: index,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("pop plan error: {}", e)));
            }
        }
    }

    pub fn cancel_pop_modal(&mut self) {
        self.pop_modal = None;
    }

    /// Open the standalone stash-drop confirmation (ADR-0087, Destructive).
    pub fn open_stash_drop_modal(&mut self, index: usize) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "drop: repo open error: {}",
                    e
                )));
                return;
            }
        };
        match repo.plan_stash_drop(index) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: stash-drop index={} blockers={}",
                    index,
                    plan.blockers.len()
                );
                self.stash_drop_modal = Some(StashDropModal {
                    plan: std::sync::Arc::new(plan),
                    error: None,
                    stash_index: index,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("drop plan error: {}", e)));
            }
        }
    }

    pub fn cancel_stash_drop_modal(&mut self) {
        self.stash_drop_modal = None;
    }

    /// Open the stash right-click context menu (Apply / Drop). Left-click on a
    /// stash row pops instead (handled in the sidebar row builder).
    pub fn open_stash_menu(
        &mut self,
        index: usize,
        message: String,
        position: gpui::Point<gpui::Pixels>,
    ) {
        self.commit_menu = None;
        self.branch_menu = None;
        self.stash_menu = Some(stash_menu::StashMenuState {
            index,
            message,
            position,
        });
        eprintln!("[kagi] stash-menu: open index={}", index);
    }

    /// Dispatch a stash context-menu action.
    pub fn dispatch_stash_action(
        &mut self,
        action: stash_menu::StashAction,
        state: stash_menu::StashMenuState,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        match action {
            stash_menu::StashAction::Pop => self.open_pop_modal(state.index),
            stash_menu::StashAction::Apply => self.open_stash_apply_modal(state.index),
            stash_menu::StashAction::Drop => self.open_stash_drop_modal(state.index),
        }
    }

    /// Execute the stash drop on a background thread (Destructive, ADR-0087).
    pub fn start_stash_drop(&mut self, cx: &mut Context<Self>) {
        let modal = match self.stash_drop_modal.clone() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: drop plan has blockers, not executing");
            self.record_op(
                "stash-drop",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.stash_drop_modal = None;
            cx.notify();
            return;
        }

        self.busy_op = Some("stash-drop");
        self.stash_drop_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyStashDrop.t()));
        eprintln!("[kagi] async: stash-drop started");

        let plan = modal.plan.clone();
        let stash_index = modal.stash_index;
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let task = cx
            .background_spawn(async move { stash_drop_blocking(&bg_path, &bg_plan, stash_index) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((summary, after)) => {
                        eprintln!("[kagi] async: stash-drop finished");
                        app.record_op(
                            "stash-drop",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "stash drop: {}",
                            summary
                        )));
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: stash-drop failed — {}", err_msg);
                        app.record_op(
                            "stash-drop",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.stash_drop_modal = Some(StashDropModal {
                            plan: plan.clone(),
                            error: Some(SharedString::from(err_msg)),
                            stash_index,
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Confirm stash pop: preflight → apply-then-drop → oplog → reload.
    pub fn confirm_pop(&mut self) {
        let modal = match self.pop_modal.clone() {
            Some(m) => m,
            None => return,
        };
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: pop plan has blockers, not executing");
            self.record_op(
                "stash-pop",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            return;
        }
        let mut repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    "stash-pop",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.pop_modal = Some(PopPlanModal {
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                    stash_index: modal.stash_index,
                });
                return;
            }
        };
        if let Err(e) = repo.preflight_check(&modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "stash-pop",
                modal.plan.current.clone(),
                OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            self.pop_modal = Some(PopPlanModal {
                plan: modal.plan.clone(),
                error: Some(SharedString::from(err_msg)),
                stash_index: modal.stash_index,
            });
            return;
        }
        match repo.execute_stash_pop(modal.stash_index) {
            Ok(()) => {
                eprintln!("[kagi] executed: stash-pop index={}", modal.stash_index);
                self.pop_modal = None;
                let after = StateSummary {
                    head: modal.plan.current.head.clone(),
                    dirty: "changes restored (stash removed)".to_string(),
                };
                self.record_op(
                    "stash-pop",
                    modal.plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                self.status_footer =
                    FooterStatus::Success(SharedString::from("stash pop: applied and dropped"));
                self.reload();
            }
            Err(e) => {
                let err_msg = format!("Pop failed: {}", e);
                self.record_op(
                    "stash-pop",
                    modal.plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                self.pop_modal = Some(PopPlanModal {
                    plan: modal.plan.clone(),
                    error: Some(SharedString::from(err_msg)),
                    stash_index: modal.stash_index,
                });
            }
        }
    }

    /// W15-ASYNCOPS: UI-path stash-pop — background thread + start/finish toasts.
    /// Headless keeps `confirm_pop` (sync).
    pub fn start_pop(&mut self, cx: &mut Context<Self>) {
        let modal = match self.pop_modal.clone() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() {
            eprintln!("[kagi] refused: pop plan has blockers, not executing");
            self.record_op(
                "stash-pop",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.pop_modal = None;
            cx.notify();
            return;
        }

        self.busy_op = Some("stash-pop");
        self.pop_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyStashPop.t()));
        eprintln!("[kagi] async: stash-pop started");

        let plan = modal.plan.clone();
        let stash_index = modal.stash_index;
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let task =
            cx.background_spawn(async move { stash_pop_blocking(&bg_path, &bg_plan, stash_index) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((summary, after)) => {
                        eprintln!("[kagi] async: stash-pop finished");
                        app.record_op(
                            "stash-pop",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "stash pop: {}",
                            summary
                        )));
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: stash-pop failed — {}", err_msg);
                        app.record_op(
                            "stash-pop",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.pop_modal = Some(PopPlanModal {
                            plan: plan.clone(),
                            error: Some(SharedString::from(err_msg)),
                            stash_index,
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
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
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "delete-branch: repo open error: {}",
                    e
                )));
                return;
            }
        };
        match repo.plan_delete_branch(&branch_name) {
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
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "delete-branch plan error: {}",
                    e
                )));
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

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Repo open error: {}", e);
                self.record_op(
                    "delete-branch",
                    modal.plan.current.clone(),
                    kagi::git::oplog::OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
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

        if let Err(e) = repo.preflight_check(&modal.plan) {
            let err_msg = format!("Preflight failed: {}", e);
            self.record_op(
                "delete-branch",
                modal.plan.current.clone(),
                kagi::git::oplog::OpOutcome::Failed {
                    error: err_msg.clone(),
                },
                &repo_path,
            );
            self.delete_branch_modal = Some(DeleteBranchModal {
                branch_name: modal.branch_name.clone(),
                plan: modal.plan.clone(),
                error: Some(SharedString::from(err_msg)),
            });
            return;
        }

        match repo.execute_delete_branch(&modal.plan, &modal.branch_name) {
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
                    kagi::git::oplog::OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
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

    /// W15-ASYNCOPS: UI-path delete-branch — background thread + start/finish
    /// toasts (ref delete is lightweight, but kept on the background path for a
    /// uniform busy/disabled experience). Headless keeps `confirm_delete_branch`.
    pub fn start_delete_branch(&mut self, cx: &mut Context<Self>) {
        let modal = match self.delete_branch_modal.clone() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
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
            self.delete_branch_modal = None;
            cx.notify();
            return;
        }

        self.busy_op = Some("delete-branch");
        self.delete_branch_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyDeleteBranch.t()));
        eprintln!("[kagi] async: delete-branch started");

        let plan = modal.plan.clone();
        let branch_name = modal.branch_name.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_branch = branch_name.clone();
        let task =
            cx.background_spawn(
                async move { delete_branch_blocking(&bg_path, &bg_plan, &bg_branch) },
            );
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok(after) => {
                        eprintln!("[kagi] async: delete-branch finished");
                        let recovery_line = plan
                            .recovery
                            .lines()
                            .nth(1)
                            .unwrap_or("git branch …")
                            .to_string();
                        app.record_op(
                            "delete-branch",
                            plan.current.clone(),
                            kagi::git::oplog::OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "delete-branch: '{}' deleted (restore: {})",
                            branch_name, recovery_line
                        )));
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: delete-branch failed — {}", err_msg);
                        app.record_op(
                            "delete-branch",
                            plan.current.clone(),
                            kagi::git::oplog::OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.delete_branch_modal = Some(DeleteBranchModal {
                            branch_name: branch_name.clone(),
                            plan: plan.clone(),
                            error: Some(SharedString::from(err_msg)),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ── W17-DISCARD: discard danger modal (ADR-0046) ─────────

    /// Collect the eligible unstaged paths (excluding untracked / conflicted)
    /// plus the skipped set, from the current commit-panel status.
    /// Returns `(eligible, skipped)` as repo-relative forward-slash strings.
    fn discard_partition(&self) -> (Vec<String>, Vec<String>) {
        let mut eligible = Vec::new();
        let mut skipped = Vec::new();
        if let Some(panel) = self.commit_panel.as_ref() {
            for f in &panel.unstaged {
                let rel = f.path.to_string_lossy().replace('\\', "/");
                // Conflicted rows are not discardable. Untracked rows (surfaced as
                // `Added` entries) ARE discardable — they are deleted from disk
                // after an ODB backup (ADR-0083).
                if panel.is_conflicted(&f.path) {
                    skipped.push(rel);
                } else {
                    eligible.push(rel);
                }
            }
        }
        (eligible, skipped)
    }

    /// Open the discard modal for a single unstaged row (by its index in the
    /// commit panel's `unstaged` vector). Conflicted rows are not offered a
    /// Discard menu; untracked rows are (they are deleted after an ODB backup,
    /// ADR-0083).
    pub fn open_discard_modal_for_index(&mut self, index: usize) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let path = match self
            .commit_panel
            .as_ref()
            .and_then(|p| p.unstaged.get(index))
        {
            Some(f) => f.path.to_string_lossy().replace('\\', "/"),
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "discard: repo open error: {}",
                    e
                )));
                return;
            }
        };
        let paths = vec![path];
        match repo.plan_discard(&paths) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: discard 1 target blockers={}",
                    plan.blockers.len()
                );
                self.discard_modal = Some(DiscardModal {
                    plan: std::sync::Arc::new(plan),
                    paths,
                    skipped: Vec::new(),
                    is_all: false,
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("discard plan error: {}", e)));
            }
        }
    }

    /// Open the "Discard all" modal: every eligible unstaged file in one
    /// operation; untracked / conflicted files are listed as skipped.
    pub fn open_discard_all_modal(&mut self) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let (eligible, skipped) = self.discard_partition();
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.status_footer = FooterStatus::Failed(SharedString::from(format!(
                    "discard: repo open error: {}",
                    e
                )));
                return;
            }
        };
        match repo.plan_discard(&eligible) {
            Ok(plan) => {
                eprintln!(
                    "[kagi] plan: discard-all {} target(s) blockers={} skipped={}",
                    eligible.len(),
                    plan.blockers.len(),
                    skipped.len()
                );
                self.discard_modal = Some(DiscardModal {
                    plan: std::sync::Arc::new(plan),
                    paths: eligible,
                    skipped,
                    is_all: true,
                    error: None,
                });
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("discard plan error: {}", e)));
            }
        }
    }

    /// Dismiss the discard modal without acting.
    pub fn cancel_discard_modal(&mut self) {
        self.discard_modal = None;
    }

    /// Confirm the discard: run `discard_blocking` on a background thread
    /// (busy_op="discard"), then reload. Mirrors `start_pop`.
    pub fn start_discard(&mut self, cx: &mut Context<Self>) {
        let modal = match self.discard_modal.clone() {
            Some(m) => m,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if !modal.plan.blockers.is_empty() || modal.paths.is_empty() {
            eprintln!("[kagi] refused: discard plan has blockers / no targets");
            self.record_op(
                "discard",
                modal.plan.current.clone(),
                OpOutcome::Refused {
                    blockers: modal.plan.blockers.clone(),
                },
                &repo_path,
            );
            self.discard_modal = None;
            cx.notify();
            return;
        }

        self.busy_op = Some("discard");
        self.discard_modal = None;
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyDiscard.t()));
        eprintln!("[kagi] async: discard started");

        let plan = modal.plan.clone();
        let paths = modal.paths.clone();
        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_paths = paths.clone();
        let task =
            cx.background_spawn(async move { discard_blocking(&bg_path, &bg_plan, &bg_paths) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((summary, after)) => {
                        eprintln!("[kagi] async: discard finished");
                        app.record_op(
                            "discard",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        app.status_footer = FooterStatus::Success(SharedString::from(format!(
                            "discard: {}",
                            summary
                        )));
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: discard failed — {}", err_msg);
                        app.record_op(
                            "discard",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        app.discard_modal = Some(DiscardModal {
                            plan: plan.clone(),
                            paths: paths.clone(),
                            skipped: modal.skipped.clone(),
                            is_all: modal.is_all,
                            error: Some(SharedString::from(err_msg)),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ── T025: Commit Panel ────────────────────────────────────

    // ── T-COMMIT-009 / W14-TEMPLATE: structured message template ──

    /// Lazily create the six template-field `InputState`s (requires `&mut
    /// Window`). Order: `[type, scope, summary, body, test, risk]`. The body is
    /// multi-line; the rest are single-line. No-op once created.
    fn ensure_template_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.commit_template_inputs.is_some() {
            return;
        }
        let ty = cx.new(|cx| InputState::new(window, cx).placeholder("type (feat, fix, …)"));
        let scope = cx.new(|cx| InputState::new(window, cx).placeholder("scope (optional)"));
        let summary = cx.new(|cx| InputState::new(window, cx).placeholder("summary"));
        let body = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .auto_grow(2, 8)
                .placeholder("body (optional)")
        });
        let test =
            cx.new(|cx| InputState::new(window, cx).placeholder("Test: how verified (optional)"));
        let risk =
            cx.new(|cx| InputState::new(window, cx).placeholder("Risk: known risks (optional)"));
        self.commit_template_inputs = Some([ty, scope, summary, body, test, risk]);
    }

    /// Read the six template `InputState`s into a [`TemplateFields`].
    /// Returns `default()` when the inputs have not been created yet.
    fn template_fields_from_inputs(&self, cx: &Context<Self>) -> kagi::git::TemplateFields {
        match &self.commit_template_inputs {
            Some([ty, scope, summary, body, test, risk]) => kagi::git::TemplateFields::new(
                ty.read(cx).value().to_string(),
                scope.read(cx).value().to_string(),
                summary.read(cx).value().to_string(),
                body.read(cx).value().to_string(),
                test.read(cx).value().to_string(),
                risk.read(cx).value().to_string(),
            ),
            None => kagi::git::TemplateFields::default(),
        }
    }

    /// Write a [`TemplateFields`] into the six template `InputState`s.
    fn set_template_inputs(
        &mut self,
        fields: &kagi::git::TemplateFields,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_template_inputs(window, cx);
        if let Some([ty, scope, summary, body, test, risk]) = self.commit_template_inputs.clone() {
            ty.update(cx, |s, cx| s.set_value(fields.r#type.clone(), window, cx));
            scope.update(cx, |s, cx| s.set_value(fields.scope.clone(), window, cx));
            summary.update(cx, |s, cx| s.set_value(fields.summary.clone(), window, cx));
            body.update(cx, |s, cx| s.set_value(fields.body.clone(), window, cx));
            test.update(cx, |s, cx| s.set_value(fields.test.clone(), window, cx));
            risk.update(cx, |s, cx| s.set_value(fields.risk.clone(), window, cx));
        }
    }

    /// Toggle between plain and template authoring modes, carrying the content
    /// across so a toggle never loses the user's work (T-COMMIT-009):
    ///
    /// - plain → template: best-effort parse the plain Input into the fields.
    /// - template → plain: assemble the fields and pour the result into the
    ///   plain Input.
    ///
    /// The new mode is mirrored straight into the draft (bumping the autosave
    /// generation) so a mode switch survives a restart.
    pub fn toggle_commit_template_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.commit_template_mode {
            // template → plain: assemble + pour into the plain Input.
            let fields = self.template_fields_from_inputs(cx);
            let assembled = kagi::git::assemble(&fields);
            if self.commit_input.is_none() {
                let st = cx.new(|cx| InputState::new(window, cx).placeholder("Commit message"));
                self.commit_input = Some(st);
            }
            if let Some(input) = self.commit_input.clone() {
                input.update(cx, |s, cx| s.set_value(assembled, window, cx));
                input.update(cx, |s, cx| s.focus(window, cx));
            }
            self.commit_template_mode = false;
        } else {
            // plain → template: parse the plain Input into the fields.
            let plain = self
                .commit_input
                .as_ref()
                .map(|i| i.read(cx).value().to_string())
                .unwrap_or_default();
            let fields = kagi::git::parse_message(&plain);
            self.set_template_inputs(&fields, window, cx);
            self.commit_template_mode = true;
            // Focus the summary field (index 2) — the most-edited one.
            if let Some(inputs) = self.commit_template_inputs.clone() {
                inputs[2].update(cx, |s, cx| s.focus(window, cx));
            }
        }
        // Persist the new mode immediately (with the current effective message).
        self.bump_draft_for_mode_change(cx);
        cx.notify();
    }

    /// Compute the effective single-message text for the current mode: the
    /// assembled template (template mode) or the plain Input value (plain mode).
    /// Used by autosave so a template draft stores its expanded plain text
    /// (ADR-0042).
    fn effective_commit_message(&self, cx: &Context<Self>) -> String {
        if self.commit_template_mode {
            kagi::git::assemble(&self.template_fields_from_inputs(cx))
        } else {
            self.commit_input
                .as_ref()
                .map(|i| i.read(cx).value().to_string())
                .unwrap_or_default()
        }
    }

    /// Force a draft save on the next debounce tick after a mode change, so the
    /// `mode` field is persisted even if the message text is unchanged.
    fn bump_draft_for_mode_change(&mut self, cx: &mut Context<Self>) {
        let msg = self.effective_commit_message(cx);
        self.last_draft_value = msg;
        self.draft_save_gen = self.draft_save_gen.wrapping_add(1);
        let gen = self.draft_save_gen;
        let mode = if self.commit_template_mode {
            "template"
        } else {
            "plain"
        };
        let mode = mode.to_string();
        cx.spawn(async move |this, acx| {
            gpui::Timer::after(Duration::from_millis(250)).await;
            let _ = this.update(acx, |app, _cx| {
                if app.draft_save_gen != gen {
                    return;
                }
                let Some(rp) = app.repo_path.clone() else {
                    return;
                };
                let branch = app.status_summary.branch.clone();
                let msg = app.last_draft_value.clone();
                if msg.trim().is_empty() {
                    let _ = kagi::git::clear_draft(&rp, &branch);
                } else {
                    let _ = kagi::git::save_draft(&rp, &branch, &msg, &mode);
                }
            });
        })
        .detach();
    }

    /// Open the commit panel (triggered by clicking the WIP row).
    ///
    /// Loads the current staging status from the repository.
    /// Clears any existing commit selection so the two views are exclusive.
    pub fn open_commit_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // T026: lazy-create the InputState (requires &mut Window) on first open.
        if self.commit_input.is_none() {
            let input_entity =
                cx.new(|cx| InputState::new(window, cx).placeholder("Commit message"));
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

        // T-COMMIT-007 / T-COMMIT-009: restore the per-branch draft into an
        // empty input, honouring the persisted mode. A template draft stores its
        // expanded plain text (ADR-0042); on restore we re-parse it back into the
        // structured fields and re-open in template mode.
        if let Some(ref input_entity) = self.commit_input {
            let current = input_entity.read(cx).value().to_string();
            if current.trim().is_empty() {
                let branch = self.status_summary.branch.clone();
                if let Some(d) = kagi::git::load_draft(&repo_path, &branch) {
                    eprintln!("[kagi] draft: loaded {} (mode={})", branch, d.mode);
                    let entity = input_entity.clone();
                    if d.mode == "template" {
                        let fields = kagi::git::parse_message(&d.message);
                        self.set_template_inputs(&fields, window, cx);
                        self.commit_template_mode = true;
                        self.last_draft_value = d.message;
                    } else {
                        entity.update(cx, |state, cx| {
                            state.set_value(d.message, window, cx);
                        });
                        self.commit_template_mode = false;
                        self.last_draft_value = entity.read(cx).value().to_string();
                    }
                }
            }
        }

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

        // T-COMMIT-016: probe for a local Ollama server (reachability only;
        // no diff is sent). Runs at most once per repo, off the UI thread.
        self.ensure_smart_commit_detection(cx);
    }

    // ── T-COMMIT-016: Smart Commit Message (W14-SMART) ───────────

    /// Probe for a reachable local Ollama server in the background.
    ///
    /// Reachability only — a single short GET to `/api/tags`; the staged diff is
    /// **never** sent here.  Runs at most once per repo path, off the UI thread.
    /// On success the panel shows "Local LLM available".  No-op when
    /// `KAGI_OFFLINE=1`.
    fn ensure_smart_commit_detection(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        if self.smart_commit_detected_for.as_deref() == Some(repo_path.as_path()) {
            return;
        }
        self.smart_commit_detected_for = Some(repo_path);

        if message_gen::offline() {
            eprintln!("[kagi] smart-commit: offline (detection skipped)");
            return;
        }

        let host = smart_commit::SmartCommitState::ollama_host();
        let task = cx.background_spawn(async move {
            let available = message_gen::ollama_available(&host);
            let models = if available {
                message_gen::ollama_list_models(&host)
            } else {
                Vec::new()
            };
            (available, models)
        });
        cx.spawn(async move |this, acx| {
            let (available, models) = task.await;
            let _ = this.update(acx, |app, cx| {
                app.smart_commit.ollama_available = available;
                app.smart_commit.detected_models = models;
                eprintln!(
                    "[kagi] smart-commit: ollama_available={} models={}",
                    available,
                    app.smart_commit.detected_models.len()
                );
                cx.notify();
            });
        })
        .detach();
    }

    // ── Auto-update (ADR-0082, T-AUTOUPDATE-001) ─────────────────────────────

    /// Startup background update check (run-once). Best-effort and silent on
    /// failure — never blocks or interrupts. Skipped when offline, when the user
    /// disabled auto-check, or when `KAGI_NO_UPDATE_CHECK` is set (the headless
    /// test harness sets it so `cargo test` never hits the network).
    fn ensure_update_check(&mut self, cx: &mut Context<Self>) {
        if self.update_checked {
            return;
        }
        self.update_checked = true;
        if std::env::var_os("KAGI_NO_UPDATE_CHECK").is_some() {
            return;
        }
        if message_gen::offline() {
            return;
        }
        if theme::read_setting("update_auto_check").as_deref() == Some("false") {
            return;
        }
        let skipped = theme::read_setting("update_skipped");
        let task =
            cx.background_spawn(async move { kagi::update::check_for_update(skipped.as_deref()) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| match result {
                Ok(Some((plan, release))) => {
                    eprintln!(
                        "[kagi] update: {} available (current {})",
                        plan.tag,
                        env!("CARGO_PKG_VERSION")
                    );
                    app.update_available = Some((plan, release));
                    cx.notify();
                }
                Ok(None) => eprintln!("[kagi] update: up to date"),
                Err(e) => eprintln!("[kagi] update: check failed (ignored): {e}"),
            });
        })
        .detach();
    }

    /// Download + verify + install the offered update, then relaunch. On failure
    /// the running install is untouched and the error is shown in the modal.
    fn start_update_install(&mut self, cx: &mut Context<Self>) {
        let Some((plan, release)) = self.update_available.clone() else {
            return;
        };
        if self.update_installing {
            return;
        }
        self.update_installing = true;
        self.update_status = Some(SharedString::from("Downloading & verifying…"));
        cx.notify();
        let task = cx.background_spawn(async move {
            kagi::update::install(&plan, &release, &|m| eprintln!("[kagi] update: {m}"))
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| match result {
                Ok(relaunch) => {
                    eprintln!("[kagi] update: installed — relaunching");
                    relaunch.spawn_and_exit();
                }
                Err(e) => {
                    eprintln!("[kagi] update: failed: {e}");
                    app.update_installing = false;
                    app.update_status = Some(SharedString::from(format!("Update failed: {e}")));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// "Skip this version" — persist the tag so the banner stays hidden for it.
    fn skip_this_update(&mut self, cx: &mut Context<Self>) {
        if let Some((plan, _)) = &self.update_available {
            theme::write_setting("update_skipped", Some(&plan.tag));
        }
        self.update_available = None;
        self.update_modal_open = false;
        cx.notify();
    }

    /// Open the release page in the default browser (Phase-0 fallback / manual).
    fn open_release_page(&self) {
        let Some((plan, _)) = &self.update_available else {
            return;
        };
        let url = format!("https://github.com/TomiXRM/kagi/releases/tag/{}", plan.tag);
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&url).spawn();
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn();
    }

    /// Read the current commit-message Input value (UI) or headless `commit_msg`.
    fn smart_commit_current_msg(&self, cx: &Context<Self>) -> String {
        if let Some(ref input) = self.commit_input {
            input.read(cx).value().to_string()
        } else {
            self.commit_panel
                .as_ref()
                .map(|p| p.commit_msg.clone())
                .unwrap_or_default()
        }
    }

    /// Write `msg` into the commit-message Input (and the headless mirror).
    /// Only overwrites a non-empty existing message after the caller has
    /// decided to (rule-based/LLM both call this to *insert* the draft).
    fn smart_commit_set_msg(&mut self, msg: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = self.commit_input.clone() {
            input.update(cx, |state, cx| {
                state.set_value(msg.to_string(), window, cx);
            });
        }
        if let Some(panel) = self.commit_panel.as_mut() {
            panel.commit_msg = msg.to_string();
        }
    }

    /// "Suggest" button — rule-based draft (always available, never networked).
    ///
    /// Inserts the draft into the message Input.  If the Input already holds a
    /// non-empty message it is left untouched (the user's text wins; ticket:
    /// overwrite only when empty).
    pub fn smart_suggest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };
        let files = repo.collect_staged_files();
        let gi = message_gen::GenInput {
            diff: String::new(),
            lang: self.smart_commit.lang,
            style: self.smart_commit.style,
        };
        let msg = message_gen::rule_based(&gi, &files);
        if std::env::var("KAGI_SMART_SUGGEST").as_deref() == Ok("1") {
            eprintln!("[kagi] smart-suggest: {}", msg);
        }
        let existing = self.smart_commit_current_msg(cx);
        if existing.trim().is_empty() {
            self.smart_commit_set_msg(&msg, window, cx);
            self.smart_commit.status = Some("Rule-based suggestion inserted".to_string());
        } else {
            self.smart_commit.status = Some("Message not empty — kept your text".to_string());
        }
        cx.notify();
    }

    /// "Generate with Local LLM" button.
    ///
    /// Enforces the opt-in gates: if the user has not yet enabled LLM generation
    /// (or never confirmed a model) the consent / model-picker modal is shown
    /// first.  Only when all gates are cleared is the staged diff collected and
    /// sent to loopback Ollama (in the background, with a timeout).  Any failure
    /// falls back **quietly** to the rule-based draft.
    pub fn smart_generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if message_gen::offline() {
            // Offline → straight to rule-based, no modal.
            self.smart_suggest(window, cx);
            return;
        }
        // Gate 1: first-time consent.
        if !self.smart_commit.llm_enabled {
            self.smart_commit.modal = Some(smart_commit::SmartCommitModal::Consent);
            cx.notify();
            return;
        }
        // Gate 2: model selection (1 model still needs confirmation, multiple
        // must be chosen — both surface as the picker on first use).
        if self.smart_commit.model.is_none() {
            self.open_smart_model_picker(cx);
            return;
        }
        self.run_smart_generation(window, cx);
    }

    /// Show the model picker, listing the detected models (`/api/tags`).
    fn open_smart_model_picker(&mut self, cx: &mut Context<Self>) {
        let models = self.smart_commit.detected_models.clone();
        if models.is_empty() {
            // No models installed → nothing to pick; fall back quietly.
            self.smart_commit.status = Some("No local models found — using rule-based".to_string());
            cx.notify();
            return;
        }
        self.smart_commit.modal = Some(smart_commit::SmartCommitModal::ModelPicker { models });
        cx.notify();
    }

    /// Consent dialog confirmed: enable LLM, then proceed to model selection.
    pub fn confirm_smart_consent(&mut self, cx: &mut Context<Self>) {
        self.smart_commit.set_enabled(true);
        self.smart_commit.modal = None;
        eprintln!("[kagi] smart-commit: llm enabled (consent given)");
        // Move on to picking a model (always confirm at least once per ADR).
        if self.smart_commit.model.is_none() {
            self.open_smart_model_picker(cx);
        } else {
            cx.notify();
        }
    }

    /// Model chosen from the picker: persist it and continue to generation.
    pub fn choose_smart_model(
        &mut self,
        model: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.smart_commit.set_model(model.clone());
        self.smart_commit.modal = None;
        eprintln!("[kagi] smart-commit: model selected = {}", model);
        self.run_smart_generation(window, cx);
    }

    /// Dismiss any Smart Commit modal without action.
    pub fn cancel_smart_modal(&mut self, cx: &mut Context<Self>) {
        self.smart_commit.modal = None;
        cx.notify();
    }

    /// Collect the staged diff and dispatch generation on a background thread.
    ///
    /// Sends only the staged diff to loopback Ollama (ureq + global timeout in
    /// the backend).  On any `Err` the result falls back to the rule-based draft
    /// so the UI never blocks or shows a blocking error.
    fn run_smart_generation(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let (Some(model), lang, style) = (
            self.smart_commit.model.clone(),
            self.smart_commit.lang,
            self.smart_commit.style,
        ) else {
            return;
        };
        let host = smart_commit::SmartCommitState::ollama_host();
        let overwrite_ok = self.smart_commit_current_msg(cx).trim().is_empty();

        self.smart_commit.generating = true;
        self.smart_commit.status = Some("Generating with local LLM…".to_string());
        cx.notify();

        let task = cx.background_spawn(async move {
            let repo = match kagi::git::Backend::open(&repo_path) {
                Ok(r) => r,
                Err(_) => return None,
            };
            let files = repo.collect_staged_files();
            let diff = repo.collect_staged_diff();
            let gi = message_gen::GenInput { diff, lang, style };
            // LLM first; on Err fall back to the rule-based draft (quietly).
            let backend = message_gen::MessageBackend::Ollama { host, model };
            let (msg, used_llm) = match message_gen::generate_message(&backend, &gi, &files) {
                Ok(m) => (m, true),
                Err(e) => {
                    eprintln!("[kagi] smart-commit: llm failed ({}) → rule-based", e);
                    (message_gen::rule_based(&gi, &files), false)
                }
            };
            Some((msg, used_llm))
        });

        cx.spawn(async move |this, acx| {
            let out = task.await;
            let _ = this.update(acx, |app, cx| {
                app.smart_commit.generating = false;
                match out {
                    Some((msg, used_llm)) if !msg.trim().is_empty() => {
                        if overwrite_ok {
                            // The Input's set_value needs `&mut Window`, which is
                            // unavailable here. Mirror into the panel state and
                            // queue the message; the next render (which has a
                            // Window) pushes it into the Input.
                            if let Some(panel) = app.commit_panel.as_mut() {
                                panel.commit_msg = msg.clone();
                            }
                            app.pending_smart_msg = Some(msg.clone());
                            app.smart_commit.status = Some(if used_llm {
                                "Generated with local LLM".to_string()
                            } else {
                                "LLM unavailable — used rule-based".to_string()
                            });
                        } else {
                            app.smart_commit.status =
                                Some("Message not empty — kept your text".to_string());
                        }
                    }
                    _ => {
                        app.smart_commit.status =
                            Some("Generation failed — edit manually".to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Stage a single file in the commit panel.
    ///
    /// Calls `stage_file` from T024 and then refreshes the staging status.
    /// Stage every non-conflicted unstaged file (T-UI-002: Stage all).
    pub fn do_stage_all(&mut self) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let paths: Vec<std::path::PathBuf> = match self.commit_panel.as_ref() {
            Some(p) => p
                .unstaged
                .iter()
                .filter(|f| !p.is_conflicted(&f.path))
                .map(|f| f.path.clone())
                .collect(),
            None => return,
        };
        if paths.is_empty() {
            return;
        }
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };
        match repo.stage_files(&paths) {
            Ok(n) => {
                eprintln!("[kagi] staged-all: {} file(s)", n);
                if let Some(panel) = self.commit_panel.as_mut() {
                    panel.reload_status(&repo_path);
                }
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("stage all failed: {}", e)));
            }
        }
    }

    /// Unstage every staged file (T-UI-002: Unstage all).
    pub fn do_unstage_all(&mut self) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let paths: Vec<std::path::PathBuf> = match self.commit_panel.as_ref() {
            Some(p) => p.staged.iter().map(|f| f.path.clone()).collect(),
            None => return,
        };
        if paths.is_empty() {
            return;
        }
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };
        match repo.unstage_files(&paths) {
            Ok(n) => {
                eprintln!("[kagi] unstaged-all: {} file(s)", n);
                if let Some(panel) = self.commit_panel.as_mut() {
                    panel.reload_status(&repo_path);
                }
            }
            Err(e) => {
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("unstage all failed: {}", e)));
            }
        }
    }

    pub fn do_stage_file(&mut self, index: usize) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let path = match self
            .commit_panel
            .as_ref()
            .and_then(|p| p.unstaged.get(index))
        {
            Some(f) => f.path.clone(),
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] stage_file: repo open error: {}", e);
                return;
            }
        };
        if let Err(e) = repo.stage_file(&path) {
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
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] unstage_file: repo open error: {}", e);
                return;
            }
        };
        if let Err(e) = repo.unstage_file(&path) {
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
        // T026 / T-COMMIT-009: prefer the effective message (assembled template
        // in template mode, else the plain Input); fall back to commit_msg
        // (headless path).
        let msg: String = if self.commit_input.is_some() || self.commit_template_mode {
            self.effective_commit_message(cx)
        } else {
            match self.commit_panel.as_ref() {
                Some(p) => p.commit_msg.clone(),
                None => return,
            }
        };
        if msg.trim().is_empty() {
            return;
        }
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] plan_commit: repo open error: {}", e);
                return;
            }
        };
        match repo.plan_commit(&msg) {
            Ok(plan) => {
                let has_blockers = !plan.blockers.is_empty();
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
                // Smooth commit (user request): with no blockers, commit immediately
                // instead of showing a "commit?" confirmation popup. The pre-commit
                // checklist blockers (conflict markers / secrets / large binaries)
                // still surface the modal as a safety net. `start_commit` captures the
                // plan synchronously, so we can drop the modal right after to suppress
                // the popup; success/failure shows in the status footer.
                if !has_blockers {
                    self.start_commit(cx);
                    if let Some(ref mut panel) = self.commit_panel {
                        panel.plan_modal = None;
                    }
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

    /// W15-ASYNCOPS: UI-path commit — tree-build + write on a background thread.
    /// The message is read from the Input on the main thread; the branch draft is
    /// cleared in the finish step (also main thread). The headless KAGI_* path
    /// executes `execute_commit` directly.
    pub fn start_commit(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        if self.busy_op.is_some() {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::OpInProgress.t()));
            return;
        }
        let commit_message: String = if self.commit_input.is_some() || self.commit_template_mode {
            self.effective_commit_message(cx)
        } else {
            self.commit_panel
                .as_ref()
                .map(|p| p.commit_msg.clone())
                .unwrap_or_default()
        };
        let plan = match self
            .commit_panel
            .as_ref()
            .and_then(|p| p.plan_modal.as_ref())
        {
            Some(modal) => modal.plan.clone(),
            None => return,
        };
        if !plan.blockers.is_empty() {
            eprintln!("[kagi] refused: commit plan has blockers");
            return;
        }

        // ADR-0068 (T-CONFLICT-FLOW-031): a merge that was continued routes the
        // commit button here with MERGE_HEAD still present.  Create the 2-parent
        // merge commit (HEAD + MERGE_HEAD) + cleanup_state instead of a plain
        // single-parent commit.  This is synchronous (cheap; no tree rebuild on a
        // worker) so the conflict-mode transition stays simple.
        if self.conflict_merge_commit_pending {
            self.finish_merge_commit(&commit_message, &plan, cx);
            return;
        }

        self.busy_op = Some("commit");
        self.status_footer = FooterStatus::Busy(SharedString::from(Msg::BusyCommit.t()));
        eprintln!("[kagi] async: commit started");

        let bg_path = repo_path.clone();
        let bg_plan = plan.clone();
        let bg_msg = commit_message.clone();
        // T-UNDOREDO-001: capture branch + tip BEFORE the commit (main thread).
        let history_before = self.head_branch_and_sha();
        let history_summary_line: String = commit_message
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(72)
            .collect();
        let task = cx.background_spawn(async move { commit_blocking(&bg_path, &bg_plan, &bg_msg) });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.busy_op = None;
                match result {
                    Ok((_new_short, after)) => {
                        eprintln!("[kagi] async: commit finished");
                        // A successful commit clears the branch draft (T-COMMIT-007).
                        let branch = app.status_summary.branch.clone();
                        let _ = kagi::git::clear_draft(&repo_path, &branch);
                        eprintln!("[kagi] draft: cleared {}", branch);
                        app.last_draft_value = String::new();

                        app.record_op(
                            "commit",
                            plan.current.clone(),
                            OpOutcome::Success { after },
                            &repo_path,
                        );
                        if let (Some((hbranch, before)), Some((_, after_sha))) =
                            (history_before.clone(), app.head_branch_and_sha())
                        {
                            let summary =
                                format!("commit {} '{}'", after_sha.short(), history_summary_line);
                            app.record_history(
                                kagi::git::OperationKind::Commit,
                                &hbranch,
                                before,
                                after_sha,
                                summary,
                            );
                        }
                        app.reload();
                    }
                    Err(err_msg) => {
                        eprintln!("[kagi] async: commit failed — {}", err_msg);
                        app.record_op(
                            "commit",
                            plan.current.clone(),
                            OpOutcome::Failed {
                                error: err_msg.clone(),
                            },
                            &repo_path,
                        );
                        if let Some(ref mut panel) = app.commit_panel {
                            if let Some(ref mut modal) = panel.plan_modal {
                                modal.error = Some(SharedString::from(err_msg.clone()));
                            }
                        }
                        // Surface commit failures in the status footer too, so the
                        // error is visible even for the smooth (no-popup) commit path
                        // where the plan modal isn't shown.
                        app.status_footer = FooterStatus::Failed(SharedString::from(format!(
                            "commit failed: {}",
                            err_msg
                        )));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Create the 2-parent merge commit for the continued-merge flow (ADR-0068 /
    /// T-CONFLICT-FLOW-031): `execute_merge_commit` (HEAD + MERGE_HEAD parents +
    /// cleanup_state), then drop the resolution buffer, clear the merge-pending /
    /// commit-panel state, oplog, and reload (which clears Conflict Mode).
    fn finish_merge_commit(
        &mut self,
        message: &str,
        plan: &std::sync::Arc<OperationPlan>,
        cx: &mut Context<Self>,
    ) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                self.push_toast(
                    ToastKind::Error,
                    SharedString::from(format!("Repo open error: {}", e)),
                );
                return;
            }
        };
        match repo.execute_merge_commit(message) {
            Ok(id) => {
                eprintln!("[kagi] executed: merge commit {}", id.short());
                let _ = kagi::git::ResolutionBuffer::clear(&repo_path);
                let branch = self.status_summary.branch.clone();
                let _ = kagi::git::clear_draft(&repo_path, &branch);
                self.last_draft_value = String::new();
                let after = StateSummary {
                    head: format!("branch: {} (merge commit {})", branch, id.short()),
                    dirty: "clean".to_string(),
                };
                self.record_op(
                    "merge-commit",
                    plan.current.clone(),
                    OpOutcome::Success { after },
                    &repo_path,
                );
                // Leave the merge-commit / commit-panel state and re-detect so
                // Conflict Mode clears (MERGE_HEAD is gone after cleanup_state).
                self.conflict_merge_commit_pending = false;
                self.commit_panel_open = false;
                if let Some(panel) = self.commit_panel.as_mut() {
                    panel.plan_modal = None;
                }
                self.reload();
            }
            Err(e) => {
                let err_msg = format!("{}", e);
                eprintln!("[kagi] merge commit failed: {}", err_msg);
                self.record_op(
                    "merge-commit",
                    plan.current.clone(),
                    OpOutcome::Failed {
                        error: err_msg.clone(),
                    },
                    &repo_path,
                );
                if let Some(panel) = self.commit_panel.as_mut() {
                    if let Some(modal) = panel.plan_modal.as_mut() {
                        modal.error = Some(SharedString::from(err_msg));
                    }
                }
            }
        }
        cx.notify();
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
                detail
                    .full_sha
                    .as_ref()
                    .get(..8)
                    .unwrap_or(&detail.full_sha),
                parent_count,
            );
        }

        // Fetch changed files on-demand (only once per row).
        if !self.diff_cache.contains_key(&index) {
            let files_opt = self.fetch_changed_files(index);
            let n = files_opt.as_ref().map(|v| v.len()).unwrap_or(0);
            eprintln!("[kagi] changed files: {}", n);
            self.diff_cache.insert(index, files_opt);
            // W16-DIFFSTAT: aggregate per-file additions/deletions alongside.
            if let Some(stats) = self.fetch_diffstat(index) {
                self.diffstat_cache.insert(index, stats);
            }
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
                        file_tree::TreeRow::File {
                            depth,
                            name,
                            file_index,
                            ..
                        } => {
                            eprintln!(
                                "[kagi] tree: {}FILE {} (idx={})",
                                "  ".repeat(*depth),
                                name,
                                file_index
                            );
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
            MainDiffSource::Commit {
                row_index,
                file_index,
            } => {
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
            MainDiffSource::Compare {
                base,
                target,
                file_index,
            } => {
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
                let cur = match cur {
                    Some(c) => c,
                    None => return,
                };
                if len == 0 {
                    return;
                }
                let next = (cur as i64 + delta).clamp(0, len as i64 - 1) as usize;
                if next != cur {
                    self.open_main_diff_wip(commit_panel::CommitPanelFileRef::Unstaged {
                        index: next,
                    });
                }
            }
            MainDiffSource::Staged { path } => {
                let (cur, len) = match self.commit_panel.as_ref() {
                    Some(p) => (p.staged.iter().position(|f| f.path == path), p.staged.len()),
                    None => return,
                };
                let cur = match cur {
                    Some(c) => c,
                    None => return,
                };
                if len == 0 {
                    return;
                }
                let next = (cur as i64 + delta).clamp(0, len as i64 - 1) as usize;
                if next != cur {
                    self.open_main_diff_wip(commit_panel::CommitPanelFileRef::Staged {
                        index: next,
                    });
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
        use kagi::git::CommitId;

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

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };

        match repo.commit_file_diff(&id, &path) {
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
                eprintln!(
                    "[kagi] main-diff: open {} rows={} highlight={}",
                    path.display(),
                    row_count,
                    hl_lang
                );

                self.main_diff = Some(MainDiffView {
                    title,
                    stats,
                    rows,
                    source: MainDiffSource::Commit {
                        row_index: selected,
                        file_index,
                    },
                });
            }
            Err(e) => {
                eprintln!("[kagi] diff error: {}", e);
            }
        }
    }

    pub fn open_main_diff_compare(&mut self, file_index: usize) {
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

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };

        let file_diff_result = match view.target {
            CompareTarget::Head => {
                let head = match repo.head_commit_id() {
                    Some(id) => id,
                    None => return,
                };
                repo.compare_file_diff(&view.base, &head, &path)
            }
            CompareTarget::WorkingTree => {
                repo.compare_commit_to_workdir_file_diff(&view.base, &path)
            }
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
                eprintln!(
                    "[kagi] main-diff: open {} rows={} highlight={}",
                    path.display(),
                    row_count,
                    hl_lang
                );

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

        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(_) => return,
        };

        let file_diff_result = if is_staged {
            repo.staged_file_diff(&path)
        } else {
            repo.unstaged_file_diff(&path)
        };

        match file_diff_result {
            Ok(fd) => {
                let added: usize = fd
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| l.kind == DiffLineKind::Added)
                    .count();
                let removed: usize = fd
                    .hunks
                    .iter()
                    .flat_map(|h| h.lines.iter())
                    .filter(|l| l.kind == DiffLineKind::Removed)
                    .count();
                eprintln!(
                    "[kagi] commit-panel diff: {} (+{} -{})",
                    path.display(),
                    added,
                    removed
                );

                let fdv = FileDiffView::from_file_diff(&fd, 0);
                let stats = SharedString::from(format!("+{} \u{2212}{}", added, removed));
                let title = fdv.file_name.clone();
                let mut rows = fdv.rows;
                let row_count = rows.len();

                // T-UI-004: apply syntax highlighting once at open time.
                let hl_lang = highlight_diff_rows(&mut rows, &path);
                eprintln!(
                    "[kagi] main-diff: open {} rows={} highlight={}",
                    path.display(),
                    row_count,
                    hl_lang
                );

                let source = if is_staged {
                    MainDiffSource::Staged { path }
                } else {
                    MainDiffSource::Unstaged { path }
                };
                self.main_diff = Some(MainDiffView {
                    title,
                    stats,
                    rows,
                    source,
                });
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
        use kagi::git::CommitId;

        let repo_path = self.repo_path.as_ref()?;
        let detail = self.details.get(index)?;
        let id = CommitId(detail.full_sha.as_ref().to_string());

        let repo = kagi::git::Backend::open(repo_path).ok()?;
        repo.commit_changed_files(&id).ok()
    }

    /// W16-DIFFSTAT: aggregate per-file additions/deletions for the commit at
    /// `index`.  Returns `None` on failure (the UI simply omits the bar).
    fn fetch_diffstat(&self, index: usize) -> Option<Vec<FileDiffStat>> {
        use kagi::git::CommitId;

        let repo_path = self.repo_path.as_ref()?;
        let detail = self.details.get(index)?;
        let id = CommitId(detail.full_sha.as_ref().to_string());

        let repo = kagi::git::Backend::open(repo_path).ok()?;
        repo.commit_diffstat(&id).ok()
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
            if let Some(stats) = self.fetch_diffstat(row_index) {
                self.diffstat_cache.insert(row_index, stats);
            }
        }
    }

    pub fn open_compare_with_head(&mut self, target: CommitId) {
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
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] compare: repo open error: {}", e);
                return;
            }
        };
        let head = match repo.head_commit_id() {
            Some(id) => id,
            None => {
                eprintln!("[kagi] compare: HEAD unavailable");
                return;
            }
        };

        match repo.compare_commits(&target, &head) {
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
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("Compare failed: {}", e)));
            }
        }
    }

    pub fn open_compare_with_working_tree(&mut self, target: CommitId) {
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
        let repo = match kagi::git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[kagi] compare: repo open error: {}", e);
                return;
            }
        };
        match repo.working_tree_status() {
            Ok(status) if !status.is_dirty() => {
                eprintln!(
                    "[kagi] compare: {} <-> working tree disabled(local changes がありません)",
                    target.short()
                );
                self.status_footer =
                    FooterStatus::Idle(SharedString::from(Msg::NoLocalChanges.t()));
                return;
            }
            Err(e) => {
                eprintln!("[kagi] compare: status error: {}", e);
                return;
            }
            _ => {}
        }

        match repo.compare_commit_to_workdir(&target) {
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
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("Compare failed: {}", e)));
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
                eprintln!(
                    "[kagi] jump: branch '{}' not found in branch_targets",
                    branch_name
                );
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
        self.commit_menu = Some(CommitMenuState {
            row_index,
            position,
        });
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
                .and_then(|repo_path| kagi::git::Backend::open(repo_path).ok())
                .and_then(|repo| repo.is_ancestor_of_head(&target).ok())
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
            local_branches: self.branches.iter().map(|(n, _)| n.clone()).collect(),
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

    pub fn open_local_branch_menu(
        &mut self,
        branch_name: String,
        position: gpui::Point<gpui::Pixels>,
    ) {
        let target = match self.branch_targets.get(&branch_name) {
            Some(target) => target.clone(),
            None => {
                eprintln!(
                    "[kagi] branch-menu: local branch '{}' target not found",
                    branch_name
                );
                return;
            }
        };
        self.commit_menu = None;
        self.jump_to_branch(&branch_name);
        self.branch_menu = Some(BranchMenuState {
            name: branch_name.clone(),
            target,
            kind: BranchKind::Local,
            position,
        });
        eprintln!("[kagi] branch-menu: open local {}", branch_name);
    }

    pub fn open_remote_branch_menu(
        &mut self,
        display_name: String,
        target: CommitId,
        position: gpui::Point<gpui::Pixels>,
    ) {
        self.commit_menu = None;
        self.jump_to_commit(&target);
        self.branch_menu = Some(BranchMenuState {
            name: display_name.clone(),
            target,
            kind: BranchKind::Remote,
            position,
        });
        eprintln!("[kagi] branch-menu: open remote {}", display_name);
    }

    fn branch_menu_context(&self, state: &BranchMenuState) -> BranchMenuContext {
        let upstream = if matches!(state.kind, BranchKind::Local) {
            self.branch_upstream_info.get(&state.name)
        } else {
            None
        };
        let is_current = matches!(state.kind, BranchKind::Local)
            && self
                .branches
                .iter()
                .any(|(name, current)| name == &state.name && *current);
        let current_branch = self
            .branches
            .iter()
            .find_map(|(name, current)| current.then(|| name.clone()));
        let checked_out_worktree_path = if matches!(state.kind, BranchKind::Local) {
            self.worktrees
                .iter()
                .find(|wt| wt.branch.as_deref() == Some(state.name.as_str()))
                .map(|wt| wt.path.display().to_string())
        } else {
            None
        };
        BranchMenuContext {
            name: state.name.clone(),
            head_sha: state.target.0.clone(),
            kind: state.kind.clone(),
            is_current,
            has_upstream: upstream.is_some(),
            upstream_name: upstream.map(|u| u.remote_branch.clone()),
            ahead: upstream.map(|u| u.ahead).unwrap_or(0),
            behind: upstream.map(|u| u.behind).unwrap_or(0),
            dirty: self.status_summary.is_dirty,
            conflict_mode: if self.status_summary.conflict_count > 0 {
                BranchConflictMode::Conflicted
            } else {
                BranchConflictMode::None
            },
            protected: branch_menu::is_protected_branch(&state.name),
            checked_out_in_other_worktree: checked_out_worktree_path.is_some(),
            checked_out_worktree_path,
            merged_into_current: false,
            is_pushed: upstream.is_some(),
            detached_head: self.status_summary.is_detached,
            busy: self.busy_op.is_some(),
            current_branch,
        }
    }

    fn render_branch_menu_overlay(
        &self,
        state: BranchMenuState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let ctx = self.branch_menu_context(&state);
        let groups = branch_menu::branch_context_menu_items(&ctx);
        let header = branch_menu::header(&ctx);
        Some(branch_menu::render_branch_menu_overlay(
            state, header, groups, window, cx,
        ))
    }

    fn render_stash_menu_overlay(
        &self,
        state: stash_menu::StashMenuState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let groups = stash_menu::build_stash_menu();
        let header = SharedString::from(format!("stash@{{{}}}: {}", state.index, state.message));
        Some(stash_menu::render_stash_menu_overlay(
            state, header, groups, window, cx,
        ))
    }

    pub fn dispatch_branch_action(
        &mut self,
        action: BranchAction,
        state: BranchMenuState,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            BranchAction::CopyBranchName => {
                branch_menu::copy_branch_name(self, state.name, cx);
            }
            BranchAction::CopyHeadSha => {
                branch_menu::copy_head_sha(self, state.target.0, cx);
            }
            BranchAction::CopyUpstreamName => {
                let upstream = self
                    .branch_upstream_info
                    .get(&state.name)
                    .map(|u| u.remote_branch.clone());
                if let Some(upstream) = upstream {
                    branch_menu::copy_upstream_name(self, upstream, cx);
                }
            }
            BranchAction::RevealHead => {
                self.jump_to_commit(&state.target);
            }
            BranchAction::Checkout => {
                if matches!(state.kind, BranchKind::Local) {
                    self.open_plan_modal(state.name);
                } else {
                    self.open_tracking_checkout_modal(state.name);
                }
            }
            BranchAction::CreateBranchFromHere => {
                self.open_create_branch_modal(state.target, cx);
            }
            BranchAction::DeleteBranch => {
                if matches!(state.kind, BranchKind::Local) {
                    self.open_delete_branch_modal(state.name);
                }
            }
            BranchAction::Pull => {
                if matches!(state.kind, BranchKind::Local) {
                    let is_current = self
                        .branches
                        .iter()
                        .any(|(name, current)| name == &state.name && *current);
                    if is_current {
                        self.open_pull_modal();
                    } else {
                        self.open_branch_plan_modal(state.name, BranchPlanKind::PullFfOnly);
                    }
                }
            }
            BranchAction::Push => {
                if matches!(state.kind, BranchKind::Local) {
                    let is_current = self
                        .branches
                        .iter()
                        .any(|(name, current)| name == &state.name && *current);
                    if is_current {
                        self.open_push_modal();
                    } else {
                        self.open_branch_plan_modal(state.name, BranchPlanKind::Push);
                    }
                }
            }
            BranchAction::PushAndCreateUpstream => {
                if matches!(state.kind, BranchKind::Local) {
                    self.open_branch_plan_modal(state.name, BranchPlanKind::PushSetUpstream);
                }
            }
            BranchAction::SetUpstream => {
                if matches!(state.kind, BranchKind::Local) {
                    self.open_set_upstream_modal(state.name);
                }
            }
            BranchAction::RenameBranch => {
                if matches!(state.kind, BranchKind::Local) {
                    self.open_rename_branch_modal(state.name);
                }
            }
            BranchAction::OpenWorktreeFromBranch => {
                let existing_path = self
                    .worktrees
                    .iter()
                    .find(|wt| wt.branch.as_deref() == Some(state.name.as_str()))
                    .map(|wt| wt.path.display().to_string());
                if let Some(path) = existing_path {
                    self.status_footer = FooterStatus::Idle(SharedString::from(format!(
                        "worktree already exists: {}",
                        path
                    )));
                    self.push_toast(ToastKind::Info, format!("Worktree: {}", path));
                } else if matches!(state.kind, BranchKind::Local) {
                    self.open_create_worktree_modal_prefilled(state.target, state.name, true, cx);
                }
            }
            BranchAction::MergeIntoCurrent => {
                self.open_merge_modal(state.name, cx);
            }
            BranchAction::CreateWorktreeFromHere => {
                self.open_create_worktree_modal_prefilled(state.target, state.name, false, cx);
            }
            BranchAction::NoUpstreamInfo
            | BranchAction::PullFfOnly
            | BranchAction::FetchRemoteBranch
            | BranchAction::CreatePr
            | BranchAction::RebaseCurrentOnto
            | BranchAction::CreateTagHere
            | BranchAction::ResetCurrentToHead
            | BranchAction::ForceWithLeasePush
            | BranchAction::DeleteRemoteBranch => {
                self.status_footer =
                    FooterStatus::Idle(SharedString::from(Msg::BcmNotImplementedYet.t()));
            }
        }
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
                        let full_sha = detail.full_sha.as_ref().to_string();
                        let short: String = full_sha.chars().take(8).collect();
                        context_menu::copy_full_sha(self, full_sha, cx);
                        // W18-COAUTHOR-COPY: surface a toast so the copy is
                        // visible regardless of where it was triggered
                        // (hash chip click or the "Copy SHA" action button).
                        self.push_toast(ToastKind::Info, format!("Copied {}", short));
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
                    eprintln!(
                        "[kagi] context-menu: checkout-ref unavailable {}",
                        target.short()
                    );
                } else {
                    self.open_plan_modal(ref_name);
                }
            }
            CommitAction::CheckoutTrackingBranch(remote_name) => {
                // Remote-only branch: create a local tracking branch + checkout
                // (same flow as the sidebar remote-branch menu).
                self.open_tracking_checkout_modal(remote_name);
            }
            CommitAction::CreateBranchHere => {
                self.open_create_branch_modal(target, cx);
                eprintln!(
                    "[kagi] context-menu: create-branch {}",
                    self.create_branch_modal
                        .as_ref()
                        .map(|m| m.at.short())
                        .unwrap_or_default()
                );
            }
            CommitAction::CreateWorktreeHere => {
                self.open_create_worktree_modal(target, cx);
                eprintln!(
                    "[kagi] context-menu: create-worktree {}",
                    self.create_worktree_modal
                        .as_ref()
                        .map(|m| m.at.short())
                        .unwrap_or_default()
                );
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
                self.status_footer =
                    FooterStatus::Idle(SharedString::from(Msg::ResetUnimplemented.t()));
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

    /// True when keyboard focus is on the app root (the commit-list
    /// context). Keyboard shortcuts that act on the selected commit must
    /// not fire while the terminal, a text input or any other focusable
    /// element owns the focus — key events bubble up to the root from
    /// every focused element (user-reported: Enter in the terminal almost
    /// checked out the selected commit).
    fn root_has_focus(&self, window: &Window) -> bool {
        self.root_focus
            .as_ref()
            .is_some_and(|fh| fh.is_focused(window))
    }

    /// Enter while a modal is open: confirm/approve the active modal (highest
    /// priority first). Returns `true` if a modal was open — the caller consumes
    /// Enter and does NOT fall through to commit checkout. Each confirm method
    /// self-guards on blockers, so a blocked plan stays open. View-only/multi-
    /// choice overlays (update, menu/settings) are consumed but not actioned.
    /// (User request: Enter approves a modal, Esc cancels it.)
    fn confirm_active_modal(&mut self, cx: &mut Context<Self>) -> bool {
        if self.discard_modal.is_some() {
            self.start_discard(cx);
        } else if self.conflict_continue_modal.is_some() {
            self.confirm_conflict_continue(cx);
        } else if self.history_modal.is_some() {
            self.confirm_history();
        } else if self.amend_modal.is_some() {
            self.confirm_amend();
        } else if self.undo_modal.is_some() {
            self.confirm_undo();
        } else if self.cherry_pick_modal.is_some() {
            self.start_cherry_pick(cx);
        } else if self.revert_modal.is_some() {
            self.start_revert(cx);
        } else if self.stash_apply_modal.is_some() {
            self.confirm_stash_apply();
        } else if self.stash_push_modal.is_some() {
            self.confirm_stash_push(cx);
        } else if self.create_worktree_modal.is_some() {
            self.start_create_worktree(cx);
        } else if self.create_branch_modal.is_some() {
            self.confirm_create_branch();
        } else if self.rename_branch_modal.is_some() {
            self.start_rename_branch(cx);
        } else if self.set_upstream_modal.is_some() {
            self.start_set_upstream(cx);
        } else if self.tracking_checkout_modal.is_some() {
            self.start_tracking_checkout(cx);
        } else if self.merge_modal.is_some() {
            self.start_merge(cx);
        } else if self.branch_plan_modal.is_some() {
            self.start_branch_plan(cx);
        } else if self.delete_branch_modal.is_some() {
            self.confirm_delete_branch();
        } else if self.pop_modal.is_some() {
            self.confirm_pop();
        } else if self.push_modal.is_some() {
            self.confirm_push();
        } else if self.pull_modal.is_some() {
            self.confirm_pull();
        } else if self.plan_modal.is_some() {
            self.confirm_checkout();
        } else if self.smart_commit.modal.is_some() {
            self.confirm_smart_consent(cx);
        } else if self
            .commit_panel
            .as_ref()
            .is_some_and(|p| p.plan_modal.is_some())
        {
            self.start_commit(cx);
        } else if self.update_modal_open || self.menu_overlay.is_some() {
            // Open but no single confirm action — consume Enter (don't check out
            // a commit), but take no action.
            return true;
        } else {
            return false;
        }
        cx.notify();
        true
    }

    /// Esc while a modal is open: cancel/close the active modal (same priority
    /// order as `confirm_active_modal`). Returns `true` if a modal was open.
    fn cancel_active_modal(&mut self, cx: &mut Context<Self>) -> bool {
        if self.discard_modal.is_some() {
            self.cancel_discard_modal();
        } else if self.conflict_continue_modal.is_some() {
            self.cancel_conflict_continue();
        } else if self.history_modal.is_some() {
            self.history_modal = None;
        } else if self.amend_modal.is_some() {
            self.cancel_amend_modal();
        } else if self.undo_modal.is_some() {
            self.cancel_undo_modal();
        } else if self.cherry_pick_modal.is_some() {
            self.cancel_cherry_pick_modal();
        } else if self.revert_modal.is_some() {
            self.cancel_revert_modal();
        } else if self.stash_apply_modal.is_some() {
            self.cancel_stash_apply_modal();
        } else if self.stash_push_modal.is_some() {
            self.cancel_stash_push_modal();
        } else if self.create_worktree_modal.is_some() {
            self.cancel_create_worktree_modal();
        } else if self.create_branch_modal.is_some() {
            self.cancel_create_branch_modal();
        } else if self.rename_branch_modal.is_some() {
            self.cancel_rename_branch_modal();
        } else if self.set_upstream_modal.is_some() {
            self.cancel_set_upstream_modal();
        } else if self.tracking_checkout_modal.is_some() {
            self.cancel_tracking_checkout_modal();
        } else if self.merge_modal.is_some() {
            self.cancel_merge_modal();
        } else if self.branch_plan_modal.is_some() {
            self.cancel_branch_plan_modal();
        } else if self.delete_branch_modal.is_some() {
            self.cancel_delete_branch_modal();
        } else if self.pop_modal.is_some() {
            self.cancel_pop_modal();
        } else if self.push_modal.is_some() {
            self.cancel_push_modal();
        } else if self.pull_modal.is_some() {
            self.cancel_pull_modal();
        } else if self.plan_modal.is_some() {
            self.cancel_modal();
        } else if self.smart_commit.modal.is_some() {
            self.cancel_smart_modal(cx);
        } else if self
            .commit_panel
            .as_ref()
            .is_some_and(|p| p.plan_modal.is_some())
        {
            self.cancel_commit_plan_modal();
        } else if self.update_modal_open {
            self.update_modal_open = false;
        } else if self.menu_overlay.is_some() {
            self.menu_overlay = None;
        } else {
            return false;
        }
        cx.notify();
        true
    }

    /// Enter on a selected commit: open the checkout plan for it
    /// (branch checkout when a local branch points here, otherwise a
    /// detached commit checkout). On a dirty working tree the confirm
    /// stashes the changes first (user request) — surfaced as an extra
    /// plan warning + `stash_first` on the modal.
    pub fn checkout_selected_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::WindowExt as _;
        if !self.root_has_focus(window) {
            return;
        }
        if self.busy_op.is_some() || self.repo_path.is_none() {
            return;
        }
        // Ignore Enter while any overlay / panel / text input is active.
        if self.plan_modal.is_some()
            || self.pull_modal.is_some()
            || self.push_modal.is_some()
            || self.branch_plan_modal.is_some()
            || self.set_upstream_modal.is_some()
            || self.rename_branch_modal.is_some()
            || self.merge_modal.is_some()
            || self.tracking_checkout_modal.is_some()
            || self.undo_modal.is_some()
            || self.history_modal.is_some()
            || self.amend_modal.is_some()
            || self.pop_modal.is_some()
            || self.create_branch_modal.is_some()
            || self.create_worktree_modal.is_some()
            || self.stash_push_modal.is_some()
            || self.stash_apply_modal.is_some()
            || self.cherry_pick_modal.is_some()
            || self.delete_branch_modal.is_some()
            || self.discard_modal.is_some()
            || self.commit_menu.is_some()
            || self.commit_panel_open
        {
            return;
        }
        if window.has_focused_input(cx) {
            return;
        }
        let Some(ix) = self.selected else {
            self.status_footer =
                FooterStatus::Idle(SharedString::from(Msg::CheckoutSelectFirst.t()));
            return;
        };
        let Some(ctx_info) = self.menu_context(ix) else {
            return;
        };
        if ctx_info.is_head {
            self.status_footer = FooterStatus::Idle(SharedString::from(Msg::AlreadyHead.t()));
            return;
        }
        let Some(id) = self.commit_id_for_row(ix) else {
            return;
        };
        let dirty = self.status_summary.is_dirty;

        // Prefer a local branch pointing at the commit; fall back to a
        // detached commit checkout.
        let branch = ctx_info
            .refs_here
            .iter()
            .find(|b| matches!(b.kind, BadgeKind::Branch))
            .map(|b| b.label.to_string());
        match branch {
            Some(name) => self.open_plan_modal(name),
            None => self.open_checkout_commit_modal(id),
        }
        if dirty {
            if let Some(m) = self.plan_modal.as_mut() {
                m.stash_first = true;
                // Surface it in the plan card's warnings.
                let mut plan = (*m.plan).clone();
                plan.warnings
                    .insert(0, Msg::DirtyStashFirst.t().to_string());
                m.plan = std::sync::Arc::new(plan);
            }
        }
        cx.notify();
    }

    /// Move the commit selection up/down by `delta` rows (arrow keys).
    /// No selection yet → selects the first row. Idempotent at the ends.
    pub fn step_commit_selection(&mut self, delta: i64) {
        if self.rows.is_empty() {
            return;
        }
        let next = match self.selected {
            None => 0,
            Some(cur) => {
                let n = cur as i64 + delta;
                n.clamp(0, self.rows.len() as i64 - 1) as usize
            }
        };
        if self.selected != Some(next) {
            self.commit_scroll_handle
                .scroll_to_item(next, ScrollStrategy::Center);
            // `select` toggles on a repeated index; guarded above.
            self.select(next);
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
        // W27-UIPOLISH: apply the global UI zoom by scaling the window's rem
        // size. gpui's `text_*` helpers and rem-based lengths resolve through
        // `rem_size()`, so this zooms virtually all of kagi's text/layout like
        // a web-page zoom. `set_rem_size` persists, but kagi re-asserts it every
        // frame so it self-heals after window re-create / zoom changes.
        window.set_rem_size(px(theme::rem_size_px()));

        // Auto-update (ADR-0082): kick the run-once background version check.
        self.ensure_update_check(cx);

        // W2-STATUS / ADR-0017: resolve the bottom-panel default height on
        // first render, once the viewport size is known (18% of viewport).
        if self.bottom_panel_height <= BOTTOM_PANEL_H_UNSET {
            let viewport_h = f32::from(window.viewport_size().height);
            let h = (viewport_h * BOTTOM_PANEL_DEFAULT_FRAC).max(BOTTOM_PANEL_MIN_H);
            self.bottom_panel_height = h;
            eprintln!(
                "[kagi] bottom-panel: default height={:.0} ({:.0}% of viewport {:.0})",
                h,
                BOTTOM_PANEL_DEFAULT_FRAC * 100.0,
                viewport_h
            );
        }

        // W11-AVATAR: kick off GitHub avatar resolution once per repo (no-op
        // for non-GitHub repos / offline / already-started).
        self.ensure_avatars(cx);

        // W30-CONFLICT-UI: detect Conflict Mode once per repo path (no-op when
        // already detected this cycle).  Covers the startup / tab-switch
        // instant-apply paths where `reload()` did not run; the watcher and
        // post-operation paths force re-detection via `reload()`.
        self.detect_conflict_mode();

        // W3-NOTIFY: keep the auto-dismiss ticker alive while toasts remain. The
        // ticker starts each toast's slide-out at end-of-life; a per-toast timer
        // removes it once it has animated out (see start_toast_exit).
        self.ensure_toast_ticker(cx);

        // Background auto-fetch ticker (periodic `git fetch` so the graph and
        // ahead/behind stay fresh). Lazily spawned; no-op when off / no repo.
        self.ensure_auto_fetch_ticker(cx);

        // ADR-0084: seed the undo/redo history from the reflog once per repo, so
        // Cmd+Z works on a freshly-opened repo (the initial CLI/snapshot path
        // never calls `reload()`). `seed_history_from_reflog` is only-when-empty,
        // so it never clobbers an in-session stack.
        if !self.history_seed_attempted {
            self.history_seed_attempted = true;
            if let Some(repo_path) = self.repo_path.clone() {
                if let Ok(backend) = kagi::git::Backend::open(&repo_path) {
                    self.seed_history_from_reflog(&backend);
                }
            }
        }

        // Modal text inputs: lazy-create + sync (needs Window).
        self.sync_modal_inputs(window, cx);

        if std::env::var("KAGI_DEBUG_RENDER").as_deref() == Ok("1") {
            use std::sync::atomic::{AtomicU64, Ordering as O};
            static N: AtomicU64 = AtomicU64::new(0);
            let n = N.fetch_add(1, O::Relaxed) + 1;
            if n.is_multiple_of(50) {
                eprintln!("[kagi] render: {} frames", n);
            }
        }

        // T-COMMIT-016: a Smart Commit message generated on a background thread
        // is pushed into the commit-message Input here, where `&mut Window` is
        // available (set_value requires it).
        if let Some(msg) = self.pending_smart_msg.take() {
            if let Some(input) = self.commit_input.clone() {
                input.update(cx, |state, cx| {
                    state.set_value(msg, window, cx);
                });
            }
        }

        // Graph horizontal scroll: clamp against the current repo's lane
        // count so the offset self-heals after tab switches and column
        // resizes.
        {
            let lane_count = self.rows.first().map(|r| r.lane_count).unwrap_or(0);
            // W28: clamp against the scaled lane pitch (matches scroll_graph_by).
            let max = (lane_count as f32 * graph_view::lane_w() - self.graph_col_w).max(0.0);
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
            // Merge: keep the platform window shell (Linux titlebar/menu) from
            // our branch AND the bundled UI font from origin.
            return self.platform_window_shell(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .size_full()
                    .font_family(UI_FONT)
                    .bg(rgb(theme().bg_base))
                    .child(
                        div()
                            .text_xl()
                            .text_color(rgb(theme().text_main))
                            .child(err),
                    )
                    .into_any(),
                cx,
            );
        }

        // W4-TABS / ADR-0028: no open tabs → Welcome screen.
        if self.tabs.is_empty() {
            let welcome = self.render_welcome(cx).into_any();
            return self.platform_window_shell(welcome, cx);
        }

        // ── Pre-fetch detail for panel (if any row is selected) ─
        let detail = selected.and_then(|i| self.details.get(i)).cloned();
        // Clone cached changed-files list for the render closure.
        // `None` outer = no selection; `Some(None)` = diff unavailable; `Some(Some(v))` = files.
        let changed_files: Option<Option<Vec<FileStatus>>> =
            selected.map(|i| self.diff_cache.get(&i).cloned().unwrap_or(None));
        // W16-DIFFSTAT: per-file additions/deletions for the selected commit.
        let changed_diffstat: Option<Vec<FileDiffStat>> =
            selected.and_then(|i| self.diffstat_cache.get(&i).cloned());
        // W2-INSPECTOR: badges for the selected commit row and tree-view toggle state.
        let selected_badges: Vec<commit_list::RefBadge> = selected
            .and_then(|i| self.rows.get(i))
            .map(|r| r.badges.clone())
            .unwrap_or_default();
        let inspector_tree_view = self.inspector_tree_view;

        // T-UI-003: Clone main diff state if present.
        let main_diff = self.main_diff.clone();
        let compare_view = self.compare_view.clone();
        let main_diff_scroll_handle = self.main_diff_scroll_handle.clone();

        // Clone modal state for render.
        let is_dirty = self.is_dirty;
        // PERF-SIDEBAR-VIRT: the navigator data (branches/remotes/tags/…) is no
        // longer cloned for render_sidebar — it's flattened into
        // `self.sidebar_rows` below and read by the virtualized list processor.
        let sidebar_filter = self.sidebar_filter.clone();
        // PERF-SIDEBAR-VIRT: flatten the navigator into `self.sidebar_rows`
        // (honouring collapse + filter) so the "sidebar-list" uniform_list can
        // virtualize it. Rebuilt every render; the processor reads the field.
        let sidebar_filter_text: String = self
            .sidebar_filter
            .as_ref()
            .map(|ent| ent.read(cx).value().to_lowercase())
            .unwrap_or_default();
        self.sidebar_rows = sidebar::build_sidebar_rows(
            &self.branches,
            &self.remote_branches,
            &self.tags,
            &self.stashes,
            &self.worktrees,
            &self.sidebar_collapsed,
            &self.branch_groups_collapsed,
            &sidebar_filter_text,
        );
        let sidebar_row_count = self.sidebar_rows.len();
        let sidebar_scroll_handle = self.sidebar_scroll_handle.clone();
        let plan_modal = self.plan_modal.clone();
        let pull_modal = self.pull_modal.clone();
        let undo_modal = self.undo_modal.clone();
        let history_modal = self.history_modal.clone();
        let amend_modal = self.amend_modal.clone();
        let pop_modal = self.pop_modal.clone();
        let stash_drop_modal = self.stash_drop_modal.clone();
        let push_modal = self.push_modal.clone();
        let branch_plan_modal = self.branch_plan_modal.clone();
        let set_upstream_modal = self.set_upstream_modal.clone();
        let rename_branch_modal = self.rename_branch_modal.clone();
        let merge_modal = self.merge_modal.clone();
        let tracking_checkout_modal = self.tracking_checkout_modal.clone();
        let create_branch_modal = self.create_branch_modal.clone();
        let create_worktree_modal = self.create_worktree_modal.clone();
        let delete_branch_modal = self.delete_branch_modal.clone();
        let discard_modal = self.discard_modal.clone();
        let file_menu = self.file_menu;
        let modal_focus = self.modal_focus.clone();
        let stash_push_modal = self.stash_push_modal.clone();
        let stash_push_focus = self.stash_push_focus.clone();
        let stash_apply_modal = self.stash_apply_modal.clone();
        let cherry_pick_modal = self.cherry_pick_modal.clone();
        let revert_modal = self.revert_modal.clone();
        let conflict_continue_modal = self.conflict_continue_modal.clone();
        let status_footer = self.status_footer.clone();
        // W30-CONFLICT-UI: clone the Conflict Mode snapshot for render (free
        // functions in `conflict_view` render from this immutable copy).
        let conflict = self.conflict.clone();
        // T-CONFLICT-FLOW-030: while a continued merge waits for its commit
        // message, show the normal body (commit panel) instead of the conflict
        // resolution body (ADR-0068). Conflict Mode is still active (MERGE_HEAD
        // present) but the editor is hidden behind the commit message panel.
        let conflict_merge_pending = self.conflict_merge_commit_pending;
        // T-CONFLICT-UI: chrome the 3-pane Conflict Editor needs from the app
        // (the editors live on `self`, not on the cloned `ConflictMode`).
        let conflict_chrome = conflict_view::EditorChrome {
            inputs: self
                .conflict_editor_inputs
                .as_ref()
                .map(|i| conflict_view::EditorInputs {
                    path: i.path.clone(),
                    result: i.result.clone(),
                }),
            ab_scroll: self.conflict_ab_scroll_handle.clone(),
            result_editing: self.conflict_result_editing,
            reset_all_armed: self.conflict_reset_all_armed,
            ab_split: self.conflict_ab_split,
            result_split: self.conflict_result_split,
            selected_hunk: self.conflict_selected_hunk,
            geom: self.conflict_geom.clone(),
            ab_geom: self.conflict_ab_geom.clone(),
        };
        let commit_menu_overlay = self
            .commit_menu
            .clone()
            .and_then(|state| self.render_commit_menu_overlay(state, window, cx));
        let branch_menu_overlay = self
            .branch_menu
            .clone()
            .and_then(|state| self.render_branch_menu_overlay(state, window, cx));
        let stash_menu_overlay = self
            .stash_menu
            .clone()
            .and_then(|state| self.render_stash_menu_overlay(state, window, cx));
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
        let divider_drag_move = cx.listener(
            move |this, event: &gpui::DragMoveEvent<DividerDrag>, window, cx| {
                let drag = *event.drag(cx);
                let cursor_x = f32::from(event.event.position.x);
                // W28: sidebar/panel widths are stored UNSCALED (logical px) but
                // rendered via `scaled_px`, so the divider visually sits at
                // `width * zoom`.  The cursor is in raw window px, so convert back
                // to logical space (divide by zoom) before clamping/storing, and
                // interpret the 4px divider's 2px half-offset in scaled space too.
                let z = theme::zoom();
                match drag.kind {
                    DividerKind::Sidebar => {
                        // Divider sits at x = sidebar_width * zoom; centre on cursor.
                        let new_width = ((cursor_x - 2.0 * z) / z).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
                        if (new_width - this.sidebar_width).abs() > 0.5 {
                            this.sidebar_width = new_width;
                            cx.notify();
                        }
                    }
                    DividerKind::Panel => {
                        // Divider sits at x = viewport_width - panel_width * zoom.
                        let viewport_w = f32::from(window.viewport_size().width);
                        let new_width =
                            ((viewport_w - cursor_x - 2.0 * z) / z).clamp(PANEL_MIN, PANEL_MAX);
                        if (new_width - this.panel_width).abs() > 0.5 {
                            this.panel_width = new_width;
                            cx.notify();
                        }
                    }
                    DividerKind::BadgeCol => {
                        // T030/W28: badge column left edge = sidebar_width + INNER_DIV_W, all
                        // rendered scaled, so the on-screen left edge is (..)*z; convert the
                        // raw cursor back to logical space (/z) before clamping/storing.
                        let badge_col_left = this.sidebar_width + INNER_DIV_W; // sidebar divider = 4px
                        let new_w = ((cursor_x / z) - badge_col_left - INNER_DIV_W / 2.0)
                            .clamp(BADGE_COL_MIN, BADGE_COL_MAX);
                        if (new_w - this.badge_col_w).abs() > 0.5 {
                            this.badge_col_w = new_w;
                            theme::set_col_width("badge_col_w", new_w);
                            cx.notify();
                        }
                    }
                    DividerKind::GraphCol => {
                        // T030/W28: graph column left edge = badge_col_left + badge_col_w + INNER_DIV_W,
                        // all rendered scaled; convert the raw cursor back to logical space (/z).
                        let badge_col_left = this.sidebar_width + INNER_DIV_W;
                        let graph_col_left = badge_col_left + this.badge_col_w + INNER_DIV_W;
                        let new_w = ((cursor_x / z) - graph_col_left - INNER_DIV_W / 2.0)
                            .clamp(GRAPH_COL_MIN, GRAPH_COL_MAX);
                        if (new_w - this.graph_col_w).abs() > 0.5 {
                            this.graph_col_w = new_w;
                            theme::set_col_width("graph_col_w", new_w);
                            cx.notify();
                        }
                    }
                    DividerKind::BottomPanel => {
                        // T-BP-002: absolute-coordinate formula from ADR-0007:
                        //   height = viewport_h - cursor_y - status_bar_h(22) - 2
                        // W28: the panel is rendered scaled, so the on-screen gap
                        // between the cursor and the window bottom is the *scaled*
                        // height; divide by zoom to recover the unscaled stored
                        // value. The status bar (also scaled) and divider half are
                        // scaled in screen space too.
                        let viewport_h = f32::from(window.viewport_size().height);
                        let cursor_y = f32::from(event.event.position.y);
                        // max fraction is a screen-space cap → convert to unscaled.
                        let max_h = (viewport_h * BOTTOM_PANEL_MAX_FRAC) / z;
                        let new_h = ((viewport_h - cursor_y - (22.0 + 2.0) * z) / z)
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
                            // Primary path: the canvas measured the real (already
                            // scaled) region bounds in screen px — use as-is.
                            (geom_top, geom_bottom)
                        } else {
                            // Transient fallback before first paint: the layout
                            // chrome is rendered scaled, so scale the constant
                            // offsets into screen space too.
                            let viewport_h = f32::from(window.viewport_size().height);
                            let bottom_taken = if this.bottom_panel_open {
                                STATUS_BAR_H + this.bottom_panel_height + BOTTOM_PANEL_DIVIDER_H
                            } else {
                                STATUS_BAR_H
                            };
                            (INSPECTOR_TOP_OFFSET * z, viewport_h - bottom_taken * z)
                        };
                        // The divider itself occupies INSPECTOR_SPLIT_DIVIDER_H of
                        // the region; the flex split applies to the remainder. The
                        // span is in screen px (scaled), so scale the divider too.
                        let span = bottom - top - inspector::INSPECTOR_SPLIT_DIVIDER_H * z;
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
                    DividerKind::ConflictAB => {
                        // T-CONFLICT-UI-003: A|B vertical divider — ratio of the
                        // measured A·B row width given to A.  The cursor sits on
                        // the divider center, while flex layout assigns the ratio
                        // to the space excluding the scaled divider.
                        let cursor_x = f32::from(event.event.position.x);
                        let (left, right) = this.conflict_ab_geom.get();
                        if let Some(ratio) = conflict_split_ratio_from_cursor(
                            cursor_x,
                            left,
                            right,
                            CONFLICT_SPLIT_DIVIDER * z,
                            CONFLICT_AB_MIN,
                            CONFLICT_AB_MAX,
                        ) {
                            if (ratio - this.conflict_ab_split).abs() > 0.001 {
                                this.conflict_ab_split = ratio;
                                cx.notify();
                            }
                        }
                    }
                    DividerKind::ConflictResult => {
                        // T-CONFLICT-UI-003: A·B / Result horizontal divider — ratio
                        // of the measured editor split region given to the A·B row.
                        // The previous separate hunk-control strip is gone; chunk
                        // controls live inside the A/B lists, so this measured
                        // region now matches the rendered split exactly.
                        let cursor_y = f32::from(event.event.position.y);
                        let (top, bottom) = this.conflict_geom.get();
                        if let Some(ratio) = conflict_split_ratio_from_cursor(
                            cursor_y,
                            top,
                            bottom,
                            CONFLICT_SPLIT_DIVIDER * z,
                            CONFLICT_RESULT_MIN,
                            CONFLICT_RESULT_MAX,
                        ) {
                            if (ratio - this.conflict_result_split).abs() > 0.001 {
                                this.conflict_result_split = ratio;
                                cx.notify();
                            }
                        }
                    }
                }
            },
        );

        // T025/T026: extract commit panel state for render.
        let commit_panel_open = self.commit_panel_open;
        let commit_panel = self.commit_panel.clone();
        let commit_input = self.commit_input.clone();
        // T-COMMIT-009 / W14-TEMPLATE: structured template mode + field inputs.
        let commit_template_mode = self.commit_template_mode;
        let commit_template_inputs = self.commit_template_inputs.clone();

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
            // Esc cancels an open modal first (user request: Esc = cancel).
            if this.cancel_active_modal(cx) {
                return;
            }
            if this.commit_menu.is_some() {
                this.commit_menu = None;
                cx.notify();
            } else if this.branch_menu.is_some() {
                this.branch_menu = None;
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
            .font_family(UI_FONT)
            .bg(rgb(theme().bg_base))
            .children(self.render_platform_titlebar(cx))
            // Key events only dispatch along the focus path, so the root must
            // own (and initially hold) focus for window-wide actions to work.
            .when_some(self.root_focus.clone(), |el, fh| el.track_focus(&fh))
            // T023: capture drag-move for both dividers on the root element.
            .on_drag_move::<DividerDrag>(divider_drag_move)
            // T-BP-002: cmd-j toggle action (window-wide via on_action on root div).
            .on_action(toggle_bottom_panel)
            // T-UI-003: Esc closes the main diff view.
            .on_action(close_main_diff)
            // Arrows: step diff files while the main diff is open, otherwise
            // move the commit selection (user request).
            .on_action(cx.listener(|this, _: &DiffPrevFile, window, cx| {
                if !this.root_has_focus(window) {
                    return;
                }
                if this.main_diff.is_some() {
                    this.main_diff_step(-1);
                } else {
                    this.step_commit_selection(-1);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &DiffNextFile, window, cx| {
                if !this.root_has_focus(window) {
                    return;
                }
                if this.main_diff.is_some() {
                    this.main_diff_step(1);
                } else {
                    this.step_commit_selection(1);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CheckoutSelected, window, cx| {
                this.checkout_selected_commit(window, cx);
            }))
            // ADR-0084: Cmd+Z / Cmd+Shift+Z app history undo/redo. Mirrors the
            // toolbar undo/redo buttons: open the plan→confirm modal when there
            // is something to (un)do, else surface a "nothing to" footer. The
            // keybinding's `!Input && !Terminal` predicate already keeps these
            // off text fields and the terminal.
            .on_action(cx.listener(|this, _: &commands::HistoryUndo, _window, cx| {
                if this.operation_history.can_undo() {
                    this.open_history_undo_modal();
                } else {
                    this.status_footer =
                        FooterStatus::Idle(SharedString::from(Msg::NothingToUndo.t()));
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &commands::HistoryRedo, _window, cx| {
                if this.operation_history.can_redo() {
                    this.open_history_redo_modal();
                } else {
                    this.status_footer =
                        FooterStatus::Idle(SharedString::from(Msg::NothingToRedo.t()));
                }
                cx.notify();
            }))
            // Enter checks out the selected commit. Handled as a raw key on
            // the root (the "enter" KeyBinding never dispatched — its
            // key_char "\n" takes a different path through the keymap than
            // chord keys like the arrows). All overlay/input guards live in
            // checkout_selected_commit.
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if std::env::var("KAGI_DEBUG_KEYS").as_deref() == Ok("1") {
                    eprintln!(
                        "[kagi] key: {:?} char={:?}",
                        e.keystroke.key, e.keystroke.key_char
                    );
                }
                let ks = &e.keystroke;
                if ks.key == "enter"
                    && !ks.modifiers.platform
                    && !ks.modifiers.control
                    && !ks.modifiers.alt
                    && !ks.modifiers.shift
                {
                    // Enter approves an open modal (user request); otherwise it
                    // checks out the selected commit.
                    if !this.confirm_active_modal(cx) {
                        this.checkout_selected_commit(window, cx);
                    }
                    cx.notify();
                }
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
            .child(self.render_header_slot(
                toolbar_state,
                status_summary,
                self.rows.first().map(|r| r.summary.to_string()),
                cx,
            ))
            // ── W30-CONFLICT-UI: persistent conflict banner (under header) ──
            .children(
                conflict
                    .as_ref()
                    .map(|m| conflict_view::render_banner(m, cx)),
            )
            // ── Body slot: in Conflict Mode the conflict resolution pane
            //    replaces the normal sidebar | list | panel body. The center is
            //    the A/B hunk editor + Result Preview; the right is always the
            //    Conflict Dashboard (GitKraken-style — see render_body).
            .when(conflict.is_some() && !conflict_merge_pending, |el| {
                let m = conflict.clone().unwrap();
                el.child(conflict_view::render_body(&m, &conflict_chrome, cx))
            })
            .when(conflict.is_none() || conflict_merge_pending, |el| {
                el.child(self.render_body(
                    row_count,
                    selected,
                    detail,
                    changed_files,
                    changed_diffstat,
                    selected_badges,
                    inspector_tree_view,
                    main_diff,
                    compare_view,
                    main_diff_scroll_handle,
                    sidebar_row_count,
                    sidebar_scroll_handle,
                    sidebar_filter,
                    is_dirty,
                    sidebar_width,
                    panel_width,
                    badge_col_w,
                    graph_col_w,
                    commit_scroll_handle,
                    commit_panel_open,
                    commit_panel.clone(),
                    commit_input.clone(),
                    commit_template_mode,
                    commit_template_inputs.clone(),
                    cx,
                ))
            })
            // ── Bottom panel slot (T-BP-002) ─────────────────
            // Hidden on the conflict-resolution screen (user request): the
            // 3-pane editor + dashboard own the whole body there. The terminal
            // returns once the conflict is resolved / the commit panel shows.
            .when(!(conflict.is_some() && !conflict_merge_pending), |el| {
                el.children(self.render_bottom_panel_slot(
                    bottom_panel_open,
                    bottom_panel_height,
                    bottom_tab,
                    cx,
                ))
            })
            // ── Commit context menu overlay (below modals) ─────
            .children(commit_menu_overlay)
            // ── Branch context menu overlay (below modals) ─────
            .children(branch_menu_overlay)
            // ── Stash context menu overlay (below modals) ──────
            .children(stash_menu_overlay)
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
            // ── Operation Undo / Redo modal (T-UNDOREDO-001) ──
            .when_some(history_modal, |el, modal| {
                el.child(render_history_modal(modal, cx))
            })
            // ── Sequencer conflict-continue confirmation (ADR-0068) ──
            .when_some(conflict_continue_modal, |el, modal| {
                el.child(render_conflict_continue_modal(modal, cx))
            })
            .when_some(amend_modal, |el, modal| {
                el.child(render_amend_modal(modal, cx))
            })
            .when_some(pop_modal, |el, modal| el.child(render_pop_modal(modal, cx)))
            // ── Stash drop modal overlay (ADR-0087) ─────────
            .when_some(stash_drop_modal, |el, modal| {
                el.child(render_stash_drop_modal(modal, cx))
            })
            // ── Push plan modal overlay (T-HT-004) ──────────
            .when_some(push_modal, |el, modal| {
                el.child(render_push_modal(modal, cx))
            })
            .when_some(branch_plan_modal, |el, modal| {
                el.child(render_branch_plan_modal(modal, cx))
            })
            .when_some(set_upstream_modal, |el, modal| {
                el.child(render_set_upstream_modal(modal, cx))
            })
            .when_some(rename_branch_modal, |el, modal| {
                el.child(render_rename_branch_modal(modal, cx))
            })
            .when_some(merge_modal, |el, modal| {
                el.child(render_merge_modal(modal, cx))
            })
            .when_some(tracking_checkout_modal, |el, modal| {
                el.child(render_tracking_checkout_modal(modal, cx))
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
            // ── Discard danger modal overlay (W17-DISCARD) ───
            .when_some(discard_modal, |el, modal| {
                el.child(render_discard_modal(modal, cx))
            })
            // ── Unstaged file context menu (right-click → Discard) ──
            .when_some(file_menu, |el, (fi, pos)| {
                el.child(render_file_menu_overlay(fi, pos, cx))
            })
            // ── Commit plan modal overlay (T025) ─────────────
            .when(
                commit_panel_open
                    && commit_panel
                        .as_ref()
                        .and_then(|p| p.plan_modal.as_ref())
                        .is_some(),
                |el| {
                    if let Some(Some(plan_modal)) =
                        commit_panel.as_ref().map(|p| p.plan_modal.clone())
                    {
                        el.child(render_commit_plan_modal(plan_modal, cx))
                    } else {
                        el
                    }
                },
            )
            // ── Smart Commit modal overlay (T-COMMIT-016) ────
            .when_some(self.smart_commit.modal.clone(), |el, modal| {
                el.child(render_smart_commit_modal(modal, cx))
            })
            // ── Auto-update modal overlay (ADR-0082) ──────────
            .when_some(
                if self.update_modal_open {
                    self.update_available.as_ref().map(|(p, _)| {
                        (
                            p.clone(),
                            self.update_installing,
                            self.update_status.clone(),
                        )
                    })
                } else {
                    None
                },
                |el, (plan, installing, status)| {
                    el.child(render_update_modal(plan, installing, status, window, cx))
                },
            )
            // ── Status bar slot (T017) — last operation result ─
            .child(self.render_status_bar(status_footer, bottom_panel_open, cx))
            // ── W3-NOTIFY: toast stack (above everything) ──────
            .children(self.render_toasts(cx))
            // Linux/FreeBSD in-app menu dropdown (native menu bar is macOS-only).
            .children(self.render_platform_menu_dropdown(cx))
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
        // T-SETTINGS-001: open Settings (menu item + cmd-,).
        let el = menu_act!(el, cmds::OpenSettings, "app.settings");
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
        let el = menu_act!(
            el,
            cmds::CompareWithWorkingTree,
            "commit.compareWorkingTree"
        );
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
        // W22-I18N: language switch actions (always enabled).
        let el = menu_act!(el, cmds::LangEnglish, "lang.english");
        let el = menu_act!(el, cmds::LangJapanese, "lang.japanese");
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
                    Msg::PullBusy.t()
                } else if this.status_summary.is_detached {
                    Msg::PullDetached.t()
                } else if this.status_summary.is_unborn {
                    Msg::PullUnborn.t()
                } else if this.status_summary.no_upstream {
                    Msg::PullNoUpstream.t()
                } else {
                    Msg::PullNothing.t()
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
                    Msg::PushBusy.t()
                } else if this.status_summary.is_detached {
                    Msg::PushDetached.t()
                } else if this.status_summary.is_unborn {
                    Msg::PushUnborn.t()
                } else if this.status_summary.no_upstream && !this.status_summary.has_remote {
                    Msg::PushNoRemote.t()
                } else {
                    Msg::PushNothing.t()
                };
                this.status_footer = FooterStatus::Idle(SharedString::from(reason));
            }
            cx.notify();
        });

        // Branch — always enabled; use selected commit if any, else HEAD.
        let branch_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
            // Resolve target commit: selected row → HEAD commit (first detail).
            let at = this
                .selected
                .and_then(|i| this.details.get(i))
                .map(|d| CommitId(d.full_sha.to_string()))
                .or_else(|| {
                    // Fall back to HEAD commit (first detail entry).
                    this.details
                        .first()
                        .map(|d| CommitId(d.full_sha.to_string()))
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
                this.status_footer = FooterStatus::Idle(SharedString::from(Msg::StashClean.t()));
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
                this.status_footer = FooterStatus::Idle(SharedString::from(Msg::PopEmpty.t()));
            }
            cx.notify();
        });

        // Undo — operation-history undo (T-UNDOREDO-001, ADR-0081). Enabled per
        // the in-session history cursor (can_undo). Click opens the undo plan
        // modal (preview → confirm runs the safe ref move).
        let undo_on = self.operation_history.can_undo();
        let undo_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if this.operation_history.can_undo() {
                this.open_history_undo_modal();
            } else {
                this.status_footer = FooterStatus::Idle(SharedString::from(Msg::NothingToUndo.t()));
            }
            cx.notify();
        });

        // Redo — operation-history redo. Enabled per can_redo().
        let redo_on = self.operation_history.can_redo();
        let redo_click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
            if this.operation_history.can_redo() {
                this.open_history_redo_modal();
            } else {
                this.status_footer = FooterStatus::Idle(SharedString::from(Msg::NothingToRedo.t()));
            }
            cx.notify();
        });

        // Refresh — always enabled.
        let refresh_click = cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
            this.refresh_spin_started = Some(Instant::now());
            // Re-read local .git immediately (instant feedback) …
            this.reload();
            this.status_footer = FooterStatus::Idle(SharedString::from(Msg::Refreshed.t()));
            // W3-NOTIFY: explicit refresh gets a completion toast (the
            // watcher's automatic reloads stay silent to avoid spam).
            this.push_toast(ToastKind::Success, Msg::Refreshed.t());
            // … then also fetch the remote in the background so changes pushed
            // elsewhere (e.g. a GitHub merge) show up. Quiet: success reloads the
            // graph, failure (offline / no remote) is silent — no error spam.
            this.fetch_async(true, cx);
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
            let text_color = if enabled {
                theme().text_main
            } else {
                theme().text_muted
            };
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
                .w(theme::scaled_px(22.0))
                .h(theme::scaled_px(22.0))
                .child(
                    gpui_component::Icon::new(icon)
                        .with_size(gpui_component::Size::Size(theme::scaled_px(20.0)))
                        .text_color(rgb(text_color)),
                );
            if count > 0 {
                let chip_text = if count > 99 {
                    "99+".to_string()
                } else {
                    count.to_string()
                };
                icon_cell = icon_cell.child(
                    div()
                        .absolute()
                        .top(theme::scaled_px(-2.0))
                        .right(theme::scaled_px(-2.0))
                        .min_w(theme::scaled_px(14.0))
                        .h(theme::scaled_px(14.0))
                        .px(theme::scaled_px(3.0))
                        .rounded_full()
                        .bg(rgb(chip_bg))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(chip_fg))
                        .text_size(px(9.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .line_height(theme::scaled_px(14.0))
                        .child(SharedString::from(chip_text)),
                );
            }

            div()
                .id(id)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(theme::scaled_px(1.0))
                .min_w(theme::scaled_px(52.0))
                .px_1()
                .py(theme::scaled_px(2.0))
                .rounded_md()
                .hover(|style| style.bg(rgb(theme().selected)))
                .cursor(if enabled {
                    gpui::CursorStyle::PointingHand
                } else {
                    gpui::CursorStyle::Arrow
                })
                .child(icon_cell)
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(text_color))
                        .child(SharedString::from(label)),
                )
        };

        // ── Undo / Redo tooltips: previewed operation summary (ADR-0081) ────
        // Labels stay the fixed "Undo"/"Redo"; the (possibly long) operation
        // summary is surfaced on hover. Sourced from the operation-history
        // cursor (peek_undo / peek_redo). `undo_summary` (legacy undo-commit
        // tooltip) is no longer used now that the button is generalised.
        let _ = &undo_summary;
        let undo_tooltip_text: Option<SharedString> = self
            .operation_history
            .peek_undo()
            .map(|e| SharedString::from(format!("Undo: {}", e.summary)));
        let redo_tooltip_text: Option<SharedString> = self
            .operation_history
            .peek_redo()
            .map(|e| SharedString::from(format!("Redo: {}", e.summary)));

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
                // 1px hairline kept literal (scaling a hairline blurs it);
                // only the visible height tracks zoom.
                .w(px(1.0))
                .h(theme::scaled_px(16.0))
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
            .h(theme::scaled_px(52.0))
            .flex_shrink_0()
            .bg(rgb(theme().panel))
            .text_color(rgb(theme().text_sub))
            // ── LEFT column (flex_1, equal width to the RIGHT column so the
            // centre cluster is window-centred regardless of side widths).
            // 3-column layout: [LEFT flex_1][centre cluster][RIGHT flex_1]. ──
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    // ── LEFT: Refresh (user request: left of the repo title) ──
                    .child({
                        // Spin for one full turn after a click (user request).
                        const SPIN_MS: u64 = 700;
                        // Spin while any async op is in flight (merge plan/exec,
                        // pull, push, fetch, …) — the user wants the sync icon to
                        // keep turning during async work — and for one rotation
                        // after an explicit Refresh click.
                        if let Some(t) = self.refresh_spin_started {
                            if t.elapsed() >= Duration::from_millis(SPIN_MS) {
                                self.refresh_spin_started = None;
                            }
                        }
                        let spinning =
                            self.busy_op.is_some() || self.refresh_spin_started.is_some();
                        let icon = gpui::svg()
                            .path("icons/refresh-cw.svg")
                            .w(theme::scaled_px(16.0))
                            .h(theme::scaled_px(16.0))
                            .text_color(rgb(theme().text_main));
                        let icon: gpui::AnyElement = if spinning {
                            use gpui::AnimationExt as _;
                            icon.with_animation(
                                "tb-refresh-spin",
                                // Repeat so it spins continuously for the whole
                                // async op (not just one rotation).
                                gpui::Animation::new(Duration::from_millis(SPIN_MS)).repeat(),
                                |svg, delta| {
                                    svg.with_transformation(gpui::Transformation::rotate(
                                        gpui::radians(delta * std::f32::consts::TAU),
                                    ))
                                },
                            )
                            .into_any_element()
                        } else {
                            icon.into_any_element()
                        };
                        div()
                            .id("tb-refresh")
                            .flex_shrink_0()
                            .mr_2()
                            .p_1()
                            .rounded_md()
                            .hover(|st| st.bg(rgb(theme().selected)).cursor_pointer())
                            .on_click(refresh_click)
                            .child(icon)
                    })
                    // ── repo name (top) + current branch (smaller, below) ──
                    // Stacked vertically so a long branch label never competes
                    // horizontally with the repo name (which used to vanish) nor
                    // runs under the centre Pull/Push/Branch cluster. Each line
                    // shrinks + truncates within the left column (user request).
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .mr_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(theme().text_main))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .line_height(theme::scaled_px(16.0))
                                    .w_full()
                                    .overflow_hidden()
                                    .truncate()
                                    .child(SharedString::from(summary.repo_name.clone())),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(theme().text_sub))
                                    .line_height(theme::scaled_px(13.0))
                                    .w_full()
                                    .overflow_hidden()
                                    .truncate()
                                    .child(SharedString::from(branch_label)),
                            ),
                    ),
            ) // ── end LEFT column ──
            // ── CENTRE: window-centred cluster (flex_shrink_0 group) ──
            // Pull Push | Branch Stash Pop | Undo Terminal
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_shrink_0()
                    // Pull (↓N chip when behind>0)
                    .child(
                        make_btn(
                            "tb-pull",
                            "Pull",
                            gpui_component::IconName::ArrowDown,
                            toolbar.pull_on,
                            toolbar.behind,
                        )
                        .on_click(pull_click),
                    )
                    .child(div().w(theme::scaled_px(2.0)))
                    // Push (↑N chip when ahead>0)
                    .child(
                        make_btn(
                            "tb-push",
                            "Push",
                            gpui_component::IconName::ArrowUp,
                            toolbar.push_on,
                            toolbar.ahead,
                        )
                        .on_click(push_click),
                    )
                    .child(sep())
                    // Branch
                    .child(
                        make_btn(
                            "tb-branch",
                            "Branch",
                            gpui_component::IconName::Plus,
                            true,
                            0,
                        )
                        .on_click(branch_click),
                    )
                    .child(div().w(theme::scaled_px(2.0)))
                    // Stash
                    .child(
                        make_btn(
                            "tb-stash",
                            "Stash",
                            gpui_component::IconName::Inbox,
                            toolbar.stash_on,
                            0,
                        )
                        .on_click(stash_click),
                    )
                    .child(div().w(theme::scaled_px(2.0)))
                    // Pop
                    .child(
                        make_btn(
                            "tb-pop",
                            "Pop",
                            gpui_component::IconName::FolderOpen,
                            toolbar.pop_on,
                            0,
                        )
                        .on_click(pop_click),
                    )
                    .child(sep())
                    // Undo — operation-history undo (T-UNDOREDO-001). Label fixed; the
                    // previewed operation summary is shown in the tooltip.
                    .child(
                        make_btn(
                            "tb-undo",
                            Msg::Undo.t(),
                            gpui_component::IconName::Undo2,
                            undo_on,
                            0,
                        )
                        .when_some(undo_tooltip_text, |btn, text| {
                            btn.tooltip(move |window, cx| {
                                Tooltip::new(text.clone()).build(window, cx)
                            })
                        })
                        .on_click(undo_click),
                    )
                    // Redo — operation-history redo (T-UNDOREDO-001).
                    .child(
                        make_btn(
                            "tb-redo",
                            Msg::Redo.t(),
                            gpui_component::IconName::Redo2,
                            redo_on,
                            0,
                        )
                        .when_some(redo_tooltip_text, |btn, text| {
                            btn.tooltip(move |window, cx| {
                                Tooltip::new(text.clone()).build(window, cx)
                            })
                        })
                        .on_click(redo_click),
                    )
                    .child(div().w(theme::scaled_px(2.0)))
                    // Terminal (toggles bottom panel Terminal tab)
                    .child(
                        make_btn(
                            "tb-terminal",
                            "Terminal",
                            gpui_component::IconName::SquareTerminal,
                            terminal_on,
                            0,
                        )
                        .on_click(terminal_click),
                    ),
            ) // ── end CENTRE cluster ──
            // ── RIGHT column (flex_1, equal width to the LEFT column) ──
            // Settings — now a standard toolbar button (icon + "Settings"
            // label) matching Pull/Push (T-SETTINGS-001 / ADR-0080). Opens the
            // Settings overlay; also reachable via the kagi menu and cmd-,.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .flex_1()
                    // Auto-update (ADR-0082): "↑ Update vX.Y.Z" chip when a newer
                    // release is available; click opens the update modal.
                    .when_some(
                        self.update_available.as_ref().map(|(p, _)| p.tag.clone()),
                        |el, tag| {
                            let open = cx.listener(|this, _: &gpui::ClickEvent, _w, cx| {
                                this.update_modal_open = true;
                                cx.notify();
                            });
                            el.child(
                                div()
                                    .id("tb-update")
                                    .flex()
                                    .items_center()
                                    .px(theme::scaled_px(8.0))
                                    .py(theme::scaled_px(4.0))
                                    .mr(theme::scaled_px(8.0))
                                    .rounded_md()
                                    .bg(rgb(theme().color_branch))
                                    .cursor(gpui::CursorStyle::PointingHand)
                                    .hover(|s| s.bg(rgb(theme().color_remote)))
                                    .child(
                                        div()
                                            .text_color(rgb(theme().bg_base))
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child(SharedString::from(format!(
                                                "\u{2191} Update {}",
                                                tag
                                            ))),
                                    )
                                    .on_click(open),
                            )
                        },
                    )
                    .child({
                        let settings_click =
                            cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                                this.menu_overlay = Some(commands::MenuOverlay::Settings);
                                cx.notify();
                            });
                        make_btn(
                            "tb-settings",
                            "Settings",
                            gpui_component::IconName::Settings,
                            true,
                            0,
                        )
                        .on_click(settings_click)
                    }),
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
        changed_diffstat: Option<Vec<FileDiffStat>>,
        selected_badges: Vec<commit_list::RefBadge>,
        inspector_tree_view: bool,
        main_diff: Option<MainDiffView>,
        compare_view: Option<CompareView>,
        main_diff_scroll_handle: UniformListScrollHandle,
        // PERF-SIDEBAR-VIRT: the navigator is now virtualized from
        // `self.sidebar_rows` (built in `render`); render_body only needs the
        // row count + scroll handle + filter input for `render_sidebar`.
        sidebar_row_count: usize,
        sidebar_scroll_handle: UniformListScrollHandle,
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
        commit_template_mode: bool,
        commit_template_inputs: Option<[Entity<InputState>; 6]>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // W11-AVATAR: snapshot the resolved avatar images so the inspector can
        // swap the initial circle for a real image without re-borrowing self.
        let avatar_images = self.avatar_images.clone();
        // Build divider 1: sidebar | main.
        let divider1 = div()
            .id("divider-sidebar")
            .w(theme::scaled_px(4.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(theme().surface))
            .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
            .cursor_col_resize()
            .on_drag(
                DividerDrag {
                    kind: DividerKind::Sidebar,
                },
                |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
            );

        // ── WIP row (shown above the list when working tree is dirty) ──
        let wip_click = cx.listener(move |this, _event: &gpui::ClickEvent, window, cx| {
            this.open_commit_panel(window, cx);
            cx.notify();
        });
        // Row-like background (NOT the header surface colour) so the WIP row
        // reads as the next commit stacking onto the graph, not as part of
        // the column-legend chrome (user feedback).
        let wip_bg = if commit_panel_open {
            theme().selected
        } else {
            theme().bg_row_alt
        };

        // T030: column header row (fixed, above WIP and commit list).
        let col_header = div()
            .id("col-header")
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .px_3()
            .h(theme::scaled_px(COL_HEADER_H))
            .flex_shrink_0()
            .bg(rgb(theme().panel))
            // Badge column label
            .child(
                div()
                    .w(theme::scaled_px(badge_col_w))
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
                    .w(theme::scaled_px(INNER_DIV_W))
                    .flex_shrink_0()
                    .h_full()
                    .bg(rgb(theme().panel))
                    // Subtle centre line so the resize boundary is visible
                    // without hovering (user request).
                    .flex()
                    .justify_center()
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().selected)))
                    .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
                    .cursor_col_resize()
                    .on_drag(
                        DividerDrag {
                            kind: DividerKind::BadgeCol,
                        },
                        |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
                    ),
            )
            // Graph column label + compact toggle button (W2-GRAPH).
            .child({
                let is_compact = self.graph_compact;
                let compact_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
                    this.graph_compact = !this.graph_compact;
                    // T-SETTINGS-001: persist so the Settings window + restart agree.
                    theme::set_compact_graph(this.graph_compact);
                    cx.notify();
                });
                div()
                    .w(theme::scaled_px(graph_col_w))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_1()
                    .on_scroll_wheel(cx.listener(
                        move |this, e: &gpui::ScrollWheelEvent, _w, cx| {
                            this.scroll_graph_by(&e.delta, cx);
                        },
                    ))
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
                            .text_color(rgb(if is_compact {
                                theme().color_branch
                            } else {
                                theme().text_muted
                            }))
                            .hover(|s| s.text_color(rgb(theme().color_branch)))
                            .on_click(compact_click)
                            .child(SharedString::from(if is_compact { "▥" } else { "▤" })),
                    )
            })
            // Handle between graph and message columns
            .child(
                div()
                    .id("divider-graph-col")
                    .w(theme::scaled_px(INNER_DIV_W))
                    .flex_shrink_0()
                    .h_full()
                    .bg(rgb(theme().panel))
                    // Subtle centre line so the resize boundary is visible
                    // without hovering (user request).
                    .flex()
                    .justify_center()
                    .child(div().w(px(1.)).h_full().bg(rgb(theme().selected)))
                    .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
                    .cursor_col_resize()
                    .on_drag(
                        DividerDrag {
                            kind: DividerKind::GraphCol,
                        },
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

        // ADR-0088: stash graph rows, shown below the WIP row.
        let stash_graph_row_els =
            self.render_stash_graph_rows(badge_col_w, graph_col_w, self.graph_scroll_x, cx);

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
                        // Badges column: tinted WIP chip, left-aligned like
                        // the commit-row badges.
                        .child({
                            let (wb, wbd, wt) = theme::badge_style(theme().color_warning);
                            div()
                                .w(theme::scaled_px(badge_col_w))
                                .flex_shrink_0()
                                .overflow_hidden()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_start()
                                .child(
                                    div()
                                        .px_1()
                                        .rounded_sm()
                                        .bg(gpui::rgba(wb))
                                        .border_1()
                                        .border_color(gpui::rgba(wbd))
                                        .text_color(rgb(wt))
                                        .text_sm()
                                        .flex_shrink_0()
                                        .child(SharedString::from("WIP")),
                                )
                        })
                        // Inner divider spacer (badge|graph handle width)
                        .child(
                            div()
                                .w(theme::scaled_px(INNER_DIV_W))
                                .flex_shrink_0()
                                .flex()
                                .justify_center()
                                .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))),
                        )
                        // Graph column: hollow "not yet committed" node on
                        // lane 0 — visually continues the graph upward.
                        .child(
                            div()
                                .w(theme::scaled_px(graph_col_w))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .child(
                                    // W28: centre the 9px hollow node on the
                                    // (zoom-scaled) lane-0 centre so it lines up
                                    // with the graph node drawn in rows below.
                                    div()
                                        .ml(theme::scaled_px(graph_view::LANE_W / 2.0 - 4.5))
                                        .w(theme::scaled_px(9.))
                                        .h(theme::scaled_px(9.))
                                        .rounded_full()
                                        .border_1()
                                        .border_color(rgb(theme().color_warning)),
                                ),
                        )
                        // Inner divider spacer (graph|message handle width)
                        .child(
                            div()
                                .w(theme::scaled_px(INNER_DIV_W))
                                .flex_shrink_0()
                                .flex()
                                .justify_center()
                                .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))),
                        )
                        // Summary area: change counts, styled like a row message.
                        .child({
                            let total = self.status_summary.staged + self.status_summary.unstaged;
                            div()
                                .flex_1()
                                .text_color(rgb(theme().text_muted))
                                .overflow_hidden()
                                .truncate()
                                .child(SharedString::from(i18n::wip_row_note(total)))
                        }),
                )
            })
            // ── Stash graph rows (ADR-0088), below WIP ───────
            .children(stash_graph_row_els)
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
                            render_rows(
                                &this.rows,
                                &this.avatar_images,
                                range,
                                selected,
                                this.badge_col_w,
                                this.graph_col_w,
                                this.graph_compact,
                                this.graph_scroll_x,
                                &this.stash_graph_lanes,
                                cx,
                            )
                        }),
                    )
                    // T028: wire scroll handle so jump_to_branch can scroll the list.
                    .track_scroll(commit_scroll_handle)
                    .flex_1()
                    .min_h(px(0.)),
                    true,
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
                    sidebar_filter,
                    sidebar_width,
                    sidebar_row_count,
                    sidebar_scroll_handle,
                    cx,
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
            .w(theme::scaled_px(4.))
            .flex_shrink_0()
            .h_full()
            .bg(rgb(theme().surface))
            .hover(|style| style.bg(rgb(theme().color_branch)).cursor_col_resize())
            .cursor_col_resize()
            .on_drag(
                DividerDrag {
                    kind: DividerKind::Panel,
                },
                |_drag, _position, _window, cx| cx.new(|_| DividerGhost),
            );

        if commit_panel_open {
            // ── Commit Panel mode (T025) ──────────────
            if let Some(panel_state) = commit_panel.clone() {
                // T-COMMIT-001: staged preview (count / A·M·D / target branch /
                // author). Cached on the panel state (computed in reload_status) —
                // computing it here ran a full working_tree_status *every frame*,
                // which froze the panel to ~6fps on large repos (PERF fix).
                let preview = panel_state.preview.clone();
                body_row = body_row.child(divider2).child(render_commit_panel(
                    panel_state,
                    panel_width,
                    commit_input.clone(),
                    commit_template_mode,
                    commit_template_inputs.clone(),
                    active_wip.clone(),
                    self.smart_commit.clone(),
                    preview,
                    self.cp_unstaged_scroll_handle.clone(),
                    self.cp_staged_scroll_handle.clone(),
                    cx,
                ));
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
                // W16-DIFFSTAT: only the commit-vs-parent view has aggregated
                // diffstat; compare mode is out of scope for this lane.
                let diffstat = if compare_for_panel.is_some() {
                    None
                } else {
                    changed_diffstat.clone()
                };
                el.child(divider2).child(inspector::render_inspector(
                    d,
                    at,
                    selected_badges.clone(),
                    files,
                    diffstat,
                    compare_for_panel,
                    active_commit_file,
                    inspector_tree_view,
                    self.inspector_split,
                    self.inspector_geom.clone(),
                    panel_width,
                    &avatar_images,
                    cx,
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
            .h(theme::scaled_px(BOTTOM_PANEL_DIVIDER_H))
            .flex_shrink_0()
            .bg(rgb(theme().surface))
            .hover(|style| style.bg(rgb(theme().color_branch)).cursor_row_resize())
            .cursor_row_resize()
            .on_drag(
                DividerDrag {
                    kind: DividerKind::BottomPanel,
                },
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
                let text_color = if is_active {
                    theme().text_main
                } else {
                    theme().text_muted
                };
                let bg_color = if is_active {
                    theme().selected
                } else {
                    theme().panel
                };
                div()
                    .px_3()
                    .h(theme::scaled_px(BOTTOM_PANEL_TAB_H))
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
                        .child(make_tab(
                            BottomTab::OperationLog.label(),
                            active_tab == BottomTab::OperationLog,
                        )),
                )
                .child(
                    div()
                        .id("tab-terminal")
                        .flex()
                        .flex_shrink_0()
                        .on_click(tab_terminal_click)
                        .hover(|s| s.cursor_pointer())
                        .child(make_tab(
                            BottomTab::Terminal.label(),
                            active_tab == BottomTab::Terminal,
                        )),
                )
        };

        // ── Body: Operation Log or Terminal ──
        let body = match active_tab {
            BottomTab::OperationLog => self.render_oplog_body(cx),
            BottomTab::Terminal => self.render_terminal_body(cx),
        };

        // ── Panel container (height = fixed, flex_shrink_0) ──
        // `height` is the unscaled, persisted body height; the whole container
        // (body + divider + tab strip) is scaled at render so it tracks zoom.
        // The BottomPanel drag math converts the raw cursor back to this
        // unscaled space (see divider_drag_move).
        let panel_h = height + BOTTOM_PANEL_DIVIDER_H + BOTTOM_PANEL_TAB_H;
        Some(
            div()
                .id("bottom-panel")
                .flex()
                .flex_col()
                .w_full()
                .h(theme::scaled_px(panel_h))
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
                .child(SharedString::from(Msg::NoOperationsYet.t()))
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
                range
                    .filter_map(|i| entries.get(i).cloned().map(|e| (i, e)))
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

                        let row_click =
                            cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
                                this.oplog_expanded = if this.oplog_expanded == Some(i) {
                                    None
                                } else {
                                    Some(i)
                                };
                                cx.notify();
                            });

                        let row_bg = if i % 2 == 0 {
                            theme().panel
                        } else {
                            theme().bg_base
                        };

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
                                    .h(theme::scaled_px(22.))
                                    .child(
                                        div()
                                            .w(theme::scaled_px(60.))
                                            .flex_shrink_0()
                                            .text_xs()
                                            .text_color(rgb(theme().text_muted))
                                            .child(time_label),
                                    )
                                    .child(
                                        div()
                                            .w(theme::scaled_px(100.))
                                            .flex_shrink_0()
                                            .ml(theme::scaled_px(6.))
                                            .text_xs()
                                            .text_color(rgb(theme().text_sub))
                                            .child(op_label),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .ml(theme::scaled_px(6.))
                                            .text_xs()
                                            .text_color(rgb(outcome_color))
                                            .truncate()
                                            .child(outcome_label),
                                    ),
                            );

                        // Expansion detail rows (before + outcome specifics).
                        if is_expanded {
                            let mut detail_lines: Vec<SharedString> = Vec::new();
                            detail_lines.push(SharedString::from(format!(
                                "  before:  {}",
                                entry.before.head
                            )));
                            detail_lines.push(SharedString::from(format!(
                                "  dirty:   {}",
                                entry.before.dirty
                            )));
                            match &entry.outcome {
                                OpOutcome::Success { after } => {
                                    detail_lines.push(SharedString::from(format!(
                                        "  after:   {}",
                                        after.head
                                    )));
                                    detail_lines.push(SharedString::from(format!(
                                        "  dirty:   {}",
                                        after.dirty
                                    )));
                                }
                                OpOutcome::Failed { error } => {
                                    detail_lines
                                        .push(SharedString::from(format!("  error:   {}", error)));
                                }
                                OpOutcome::Refused { blockers } => {
                                    for b in blockers {
                                        detail_lines
                                            .push(SharedString::from(format!("  blocker: {}", b)));
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
                                .children(detail_lines.into_iter().map(|line| div().child(line)));
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

        with_vertical_scrollbar("oplog-list-scroll", &scrollbar_handle, oplog_list, true)
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
                    // Mark this subtree as the "Terminal" key context so global
                    // arrow/escape KeyBindings (scoped `!Terminal`) don't consume
                    // those keys while the terminal is focused — they flow to the
                    // terminal's own on_key_down → PTY (history, vim, etc.).
                    .key_context("Terminal")
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
                    .on_key_down(
                        cx.listener(move |_this, event: &KeyDownEvent, _window, cx| {
                            let ks = &event.keystroke;
                            if ks.modifiers.platform && ks.key == "v" {
                                if let Some(writer) = paste_writer.as_ref() {
                                    if let Some(text) =
                                        cx.read_from_clipboard().and_then(|item| item.text())
                                    {
                                        writer.paste_text(&text);
                                        eprintln!(
                                            "[kagi] terminal: paste {} chars",
                                            text.chars().count()
                                        );
                                    }
                                }
                                cx.stop_propagation();
                            }
                        }),
                    )
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
            .child(SharedString::from(
                "(terminal exited — re-opening will restart)",
            ))
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
                    .ml(theme::scaled_px(4.))
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
                    .ml(theme::scaled_px(4.))
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
                    .ml(theme::scaled_px(4.))
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
                    .ml(theme::scaled_px(4.))
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
                    .ml(theme::scaled_px(4.))
                    .text_color(rgb(theme().text_sub))
                    .flex_shrink_0()
                    .child(SharedString::from(format!(
                        "\u{2691}{}",
                        summary.stash_count
                    ))), // ⚑N
            )
        } else {
            None
        };

        // ── Upstream name (W2-STATUS) ──────────────────────────
        let upstream_name_chip = if !summary.upstream_name.is_empty() {
            Some(
                div()
                    .ml(theme::scaled_px(6.))
                    .text_color(rgb(theme().text_muted))
                    .flex_shrink_0()
                    .child(SharedString::from(format!(
                        "\u{2192} {}",
                        summary.upstream_name
                    ))), // → origin/main
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
                        .ml(theme::scaled_px(6.))
                        .text_color(rgb(theme().text_sub))
                        .flex_shrink_0()
                        .child(SharedString::from(label)),
                )
            }
            _ if summary.no_upstream => Some(
                div()
                    .ml(theme::scaled_px(6.))
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
                    .ml(theme::scaled_px(6.))
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

        let icon_terminal_color = if terminal_active {
            theme().text_main
        } else {
            theme().text_muted
        };
        let icon_oplog_color = if oplog_active {
            theme().text_main
        } else {
            theme().text_muted
        };

        let icon_terminal = div()
            .id("status-icon-terminal")
            .ml(theme::scaled_px(4.))
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
            .ml(theme::scaled_px(2.))
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
            .h(theme::scaled_px(STATUS_BAR_H))
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
                .ml(theme::scaled_px(6.))
                .overflow_hidden()
                .text_color(rgb(footer_color))
                .child(footer_text),
        );

        // Icon buttons at the right end.
        bar.child(icon_terminal).child(icon_oplog)
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
#[allow(clippy::too_many_arguments)]
fn render_rows(
    rows: &[CommitRow],
    avatar_images: &HashMap<String, std::sync::Arc<gpui::Image>>,
    range: std::ops::Range<usize>,
    selected: Option<usize>,
    badge_col_w: f32,
    graph_col_w: f32,
    graph_compact: bool,
    graph_scroll_x: f32,
    stash_lanes: &[usize],
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
            let context_click_handler =
                cx.listener(move |this, event: &gpui::MouseDownEvent, _window, cx| {
                    this.open_commit_menu(ix, event.position);
                    cx.stop_propagation();
                    cx.notify();
                });

            // ── Avatar (T020 / W11-AVATAR) ────────────────────
            let avatar_color = avatar::avatar_color(&row.author_email);
            let avatar_init = SharedString::from(avatar::avatar_initial(&row.author));
            // Convert Hsla to the rgb u32 that gpui's `bg()` accepts via hsla().
            let av_bg = avatar_color;
            // W11-AVATAR: real GitHub avatar if resolved, else initial circle.
            let avatar_image = avatar_images.get(&row.author_email).cloned();

            // W2-GRAPH: badge presence flag for label→node connector line.
            let has_badges = !row.badges.is_empty();
            // Connector colour for the badge→node line (extends into the
            // BRANCH/TAG pane). Matches the node's lane colour; only when the
            // graph isn't scrolled sideways (the canvas connector is gated the
            // same way).
            let connector_color: Option<gpui::Hsla> = if has_badges && graph_scroll_x < 0.5 {
                Some(theme().lane_color(row.lane))
            } else {
                None
            };

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
                    // W28: non-selected rows use px_3 (0.75rem) which scales with
                    // zoom; the selected row must match so the graph column origin
                    // doesn't shift horizontally on selection. Left inset =
                    // scaled px_3 minus the fixed 2px accent bar.
                    el.pl(theme::scaled_px(12.) - px(2.))
                        .border_l_2()
                        .border_color(rgb(theme().color_branch))
                })
                .when(!is_selected, |el| el.px_3())
                .h(px(rh))
                .bg(rgb(row_bg))
                .on_click(click_handler)
                .on_mouse_down(MouseButton::Right, context_click_handler)
                // ── Badges column: user-resizable width (T030) ──
                // T-DNDMERGE-001: thread `cx` so each `BadgeKind::Branch` chip
                // can be made draggable and the HeadBranch chip a drop target.
                // Reborrow `cx` (the `.map()` closure already mutably borrows it
                // for `cx.listener(...)` above) per row.
                .child(render_badges_column(
                    &row.badges,
                    badge_col_w,
                    connector_color,
                    &mut *cx,
                ))
                // ── Inner divider spacer (badge|graph handle width) ──
                // When the row has a badge connector, bridge the 4px gap with a
                // horizontal line so the badge→node connector stays continuous.
                .child(
                    div()
                        .relative()
                        .w(theme::scaled_px(INNER_DIV_W))
                        .h_full()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(div().w(px(1.)).h_full().bg(rgb(theme().surface)))
                        .when_some(connector_color, |el, color| {
                            // Fill height + items_center so the 1px line is
                            // centred exactly like the badge-column and canvas
                            // connectors (no 1px step at the boundary).
                            el.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .child(div().w_full().h(theme::scaled_px(1.)).bg(color)),
                            )
                        }),
                )
                // ── Graph lane area (T030) ────────────────────────
                // Always render the graph column at graph_col_w width.
                // Clip by visible_lanes to prevent bleed into message column.
                .child(
                    div()
                        .w(theme::scaled_px(graph_col_w))
                        .h_full()
                        .flex_shrink_0()
                        .overflow_hidden()
                        // Horizontal wheel/trackpad scroll reveals clipped
                        // lanes. Vertical deltas are left untouched so the
                        // commit list keeps scrolling normally.
                        .on_scroll_wheel(cx.listener(
                            move |this, e: &gpui::ScrollWheelEvent, _w, cx| {
                                this.scroll_graph_by(&e.delta, cx);
                            },
                        ))
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
                                    stash_lanes.to_vec(),
                                )
                                .size_full(),
                            )
                        }),
                )
                // ── Inner divider spacer (graph|message handle width) ──
                .child(
                    div()
                        .w(theme::scaled_px(INNER_DIV_W))
                        .flex_shrink_0()
                        .flex()
                        .justify_center()
                        .child(div().w(px(1.)).h_full().bg(rgb(theme().surface))),
                )
                // ── Author avatar: 18px circle after graph ────────
                // W11-AVATAR: when a GitHub avatar is resolved, show the image
                // clipped to the circle; otherwise the initial-on-colour circle.
                .child({
                    // W28: avatar circle scales with zoom so it stays sized to
                    // the (rem-scaled) row text and aligned with the graph node.
                    let circle = div()
                        .w(theme::scaled_px(18.))
                        .h(theme::scaled_px(18.))
                        .flex_shrink_0()
                        .mr(theme::scaled_px(4.))
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
                            .child(div().text_color(gpui::white()).text_xs().child(avatar_init)),
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
                    // W28: author/date columns scale so the (rem-scaled) text
                    // fits its box at any zoom.
                    div()
                        .w(theme::scaled_px(130.))
                        .flex_shrink_0()
                        .text_color(rgb(theme().text_sub))
                        .truncate()
                        .child(row.author.clone()),
                )
                .child(
                    div()
                        .w(theme::scaled_px(72.))
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
                true,
            )
        })
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
fn render_badges_column(
    badges: &[commit_list::RefBadge],
    badge_col_w: f32,
    // When `Some`, draw a horizontal connector line filling the space between
    // the badges and the right edge of the column, so the badge→node line is
    // continuous *inside* the BRANCH/TAG pane (not stopping at the boundary).
    connector_color: Option<gpui::Hsla>,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
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
            // Stable element id so gpui interactivity (drag/drop) works. Keyed
            // by row position + badge label so a row with multiple branch chips
            // gets distinct ids (a commit can carry several branches).
            .id(SharedString::from(format!(
                "graph-badge-{i}-{}",
                badge.label
            )))
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

        // T-DNDMERGE-001 / ADR-0079: wire drag/drop onto the chip based on kind.
        //   - `BadgeKind::Branch` / `BadgeKind::Remote` → INDEPENDENTLY draggable,
        //     carrying ITS OWN name (= the merge source) in `BranchDrag { name }`.
        //     For a remote chip the name is the full `remote/name` ref, so an
        //     upstream-only branch can be merged directly. Each visible chip
        //     carries its own name, so dragging a specific badge unambiguously
        //     selects that branch even when a commit has several. Tag chips are
        //     NOT draggable.
        //   - `BadgeKind::HeadBranch` (the current branch) → drop TARGET. It
        //     shows a valid-target highlight via `.drag_over::<BranchDrag>` and
        //     dispatches to `start_merge_from_drag` on drop. The drop is a
        //     TRIGGER only — it never calls git from the view (same as sidebar).
        let chip = match badge.kind {
            BadgeKind::Branch | BadgeKind::Remote => {
                if let Some(name) = draggable_branch_name(badge) {
                    chip.cursor_grab().on_drag(
                        BranchDrag { name: name.clone() },
                        move |drag: &BranchDrag, _pos, _window, cx| {
                            let name = SharedString::from(drag.name.clone());
                            cx.new(|_| BranchDragGhost { name })
                        },
                    )
                } else {
                    chip
                }
            }
            BadgeKind::HeadBranch => {
                let drop_handler = cx.listener(
                    move |this: &mut KagiApp, payload: &BranchDrag, _window, cx| {
                        this.start_merge_from_drag(payload.name.clone(), cx);
                        cx.notify();
                    },
                );
                chip.drag_over::<BranchDrag>(|style, _drag, _window, _cx| {
                    style
                        .bg(rgb(theme().selected))
                        .border_color(rgb(theme().color_branch))
                })
                .on_drop::<BranchDrag>(drop_handler)
            }
            BadgeKind::Tag => chip,
        };
        inner = inner.child(chip);

        // "+N" chip directly after the primary chip (never clipped).
        // TODO(T-DNDMERGE-001): badges hidden behind the "+N" overflow are not
        // individually draggable yet (only the up-to-MAX_BADGES visible chips
        // are). Redesigning the overflow into a draggable popover is out of
        // scope for this lane.
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
        .w(theme::scaled_px(badge_col_w))
        .flex_shrink_0()
        .overflow_hidden()
        .flex()
        .flex_row()
        .items_center()
        .justify_start()
        .child(inner)
        // Connector line: fills the remaining width up to the column's right
        // edge so the line reaches into the BRANCH/TAG pane toward the badge.
        .when_some(connector_color, |el, color| {
            el.child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(div().w_full().h(theme::scaled_px(1.)).bg(color)),
            )
        })
}

// ──────────────────────────────────────────────────────────────
// W3-NOTIFY: blocking cores for pull / push
//
// Everything that may take seconds (repo open → preflight → execute →
// verify snapshot) lives here, free of `&mut KagiApp`, so the UI path can
// run it via `cx.background_spawn` while the headless path calls it inline.
// ──────────────────────────────────────────────────────────────

/// Blocking part of stash push (preflight → execute → verify). Stashing
/// copies the working tree (and untracked files) into the stash, which can
/// take a long time on large repos — running it on the UI thread looked
/// like a total freeze (user-reported).
fn stash_push_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    message: Option<String>,
) -> Result<(String, StateSummary), String> {
    let t0 = Instant::now();
    let mut repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.preflight_check_stash(plan, plan.stash_count_at_plan())
        .map_err(|e| format!("Preflight failed: {}", e))?;
    repo.execute_stash_push(message.as_deref(), true)
        .map_err(|e| format!("Stash push failed: {}", e))?;
    let t_stash = t0.elapsed();
    eprintln!(
        "[kagi] executed: stash-push message={:?}",
        message.unwrap_or_default()
    );

    // Light verify: the full reload that follows on the main thread already
    // rebuilds the complete snapshot, so re-walking 10k commits here only
    // doubled the wall-clock (user asked why stash took ~10s). Status + a
    // stash-count check are enough to confirm the operation took effect.
    let t1 = Instant::now();
    let after = match repo.working_tree_status() {
        Ok(status) => {
            if !status.is_dirty() {
                eprintln!("[kagi] verified: working tree clean after stash-push");
            } else {
                eprintln!("[kagi] verify: working tree NOT clean after stash-push");
            }
            let count = repo.stash_count().unwrap_or(0);
            eprintln!("[kagi] verified: stash count={}", count);
            // resolve_head is crate-private; the predicted head from the
            // plan is accurate here (stash does not move HEAD).
            let head = plan.predicted.head.clone();
            StateSummary {
                head,
                dirty: if status.is_dirty() {
                    "dirty".into()
                } else {
                    "clean".into()
                },
            }
        }
        Err(_) => plan.predicted.clone(),
    };
    eprintln!(
        "[kagi] async: stash-push timing stash={:.1}s verify={:.1}s",
        t_stash.as_secs_f32(),
        t1.elapsed().as_secs_f32()
    );
    Ok(("stashed working tree".to_string(), after))
}

/// Blocking part of pull. Returns (human summary, after-state) or an error
/// message suitable for the oplog / modal.
fn pull_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
) -> Result<(String, StateSummary), String> {
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.preflight_check(plan)
        .map_err(|e| format!("Preflight failed: {}", e))?;

    let outcome = repo
        .execute_pull()
        .map_err(|e| format!("Pull failed: {}", e))?;
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
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.preflight_check(plan)
        .map_err(|e| format!("Preflight failed: {}", e))?;

    let outcome = repo
        .execute_push()
        .map_err(|e| format!("Push failed: {}", e))?;
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
    match kagi::git::Backend::open(repo_path) {
        Ok(mut repo2) => match repo2.snapshot(10_000) {
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
// W15-ASYNCOPS: blocking cores for the tree-size-proportional ops
//
// Same shape as the pull/push/stash cores above: repo open → preflight →
// execute → verify snapshot, free of `&mut KagiApp`, so the UI button path can
// run them via `cx.background_spawn`. The headless KAGI_* path keeps calling the
// synchronous `confirm_*` methods (unchanged log文言/order). ref-order rules and
// in-memory semantics are unchanged — only the threading moved.
// ──────────────────────────────────────────────────────────────

/// Blocking part of checkout (branch or commit). `checkout_tree` writes the
/// working tree on disk, which scales with tree size.
fn checkout_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    target: &CheckoutPlanTarget,
) -> Result<(String, StateSummary), String> {
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.preflight_check(plan)
        .map_err(|e| format!("Preflight failed: {}", e))?;

    let execute_result = match target {
        CheckoutPlanTarget::Branch(branch) => repo.execute_checkout(branch),
        CheckoutPlanTarget::Commit(commit_id) => repo.execute_checkout_commit(commit_id),
    };
    execute_result.map_err(|e| format!("Checkout failed: {}", e))?;

    let summary = match target {
        CheckoutPlanTarget::Branch(branch) => {
            eprintln!("[kagi] executed: checkout {}", branch);
            format!("checkout {}", branch)
        }
        CheckoutPlanTarget::Commit(commit_id) => {
            eprintln!("[kagi] executed: checkout-commit {}", commit_id.short());
            format!("detached: {}", commit_id.short())
        }
    };

    // Verify: re-snapshot and confirm HEAD.
    let after = match kagi::git::Backend::open(repo_path) {
        Ok(mut repo2) => match repo2.snapshot(10_000) {
            Ok(snap) => {
                match (target, &snap.head) {
                    (
                        CheckoutPlanTarget::Branch(branch),
                        Head::Attached {
                            branch: actual_branch,
                            ..
                        },
                    ) if actual_branch == branch => {
                        eprintln!("[kagi] verified: HEAD={}", actual_branch);
                    }
                    (CheckoutPlanTarget::Commit(commit_id), Head::Detached { target: t })
                        if t == &commit_id.0 =>
                    {
                        eprintln!("[kagi] verified: detached HEAD={}", commit_id.short());
                    }
                    other => {
                        eprintln!(
                            "[kagi] verify: unexpected HEAD state after checkout: {:?}",
                            other
                        );
                    }
                }
                StateSummary {
                    head: snap.head.display(),
                    dirty: if snap.status.is_dirty() {
                        "dirty".to_string()
                    } else {
                        "clean".to_string()
                    },
                }
            }
            Err(e) => {
                eprintln!("[kagi] verify: snapshot error: {}", e);
                plan.predicted.clone()
            }
        },
        Err(e) => {
            eprintln!("[kagi] verify: repo open error: {}", e);
            plan.predicted.clone()
        }
    };
    Ok((summary, after))
}

fn merge_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    target: &str,
    kind: &MergeKind,
) -> Result<(String, StateSummary), String> {
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.preflight_check(plan)
        .map_err(|e| format!("Preflight failed: {}", e))?;

    match kind {
        MergeKind::Conflicts(_) => {
            // W31: perform the real conflicting merge — leaves markers + index
            // stages + MERGE_HEAD. No commit is created; Conflict Mode takes over
            // on the subsequent reload.
            let files = repo
                .execute_merge_into_conflict(target)
                .map_err(|e| format!("Merge failed: {}", e))?;
            eprintln!(
                "[kagi] executed: merge-into-conflict {} -> {} conflict(s)",
                target,
                files.len()
            );
            let after = verify_after_snapshot(repo_path, plan);
            Ok((
                format!("merge {} (conflicts: {})", target, files.len()),
                after,
            ))
        }
        MergeKind::FastForward | MergeKind::MergeCommit => {
            let new_head = repo
                .execute_merge_branch(target)
                .map_err(|e| format!("Merge failed: {}", e))?;
            eprintln!("[kagi] executed: merge {} -> {}", target, new_head.short());

            let after = verify_after_snapshot(repo_path, plan);
            eprintln!("[kagi] verified: merge after = {}", after.head);
            Ok((format!("merge {}", target), after))
        }
    }
}

fn checkout_tracking_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    remote_branch: &str,
    local_branch: &str,
) -> Result<(String, StateSummary), String> {
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.preflight_check(plan)
        .map_err(|e| format!("Preflight failed: {}", e))?;

    repo.execute_checkout_tracking_branch(remote_branch, local_branch)
        .map_err(|e| format!("Checkout tracking failed: {}", e))?;
    eprintln!(
        "[kagi] executed: checkout-tracking {} -> {}",
        remote_branch, local_branch
    );

    let after = verify_after_snapshot(repo_path, plan);
    eprintln!("[kagi] verified: checkout-tracking after = {}", after.head);
    Ok((format!("checkout {}", local_branch), after))
}

/// Blocking part of cherry-pick (in-memory index merge → commit → safe
/// checkout_head). Scales with the diff size.
fn cherry_pick_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    commit_id: &CommitId,
) -> Result<(String, StateSummary), String> {
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.preflight_check(plan)
        .map_err(|e| format!("Preflight failed: {}", e))?;

    let new_id = repo
        .execute_cherry_pick(commit_id)
        .map_err(|e| format!("Cherry-pick failed: {}", e))?;
    eprintln!(
        "[kagi] executed: cherry-pick {} -> {}",
        commit_id.short(),
        new_id.short()
    );

    let after = verify_new_commit_snapshot(repo_path, plan, &new_id, "cherry-pick");
    Ok((format!("{} applied", commit_id.short()), after))
}

/// Blocking part of revert (in-memory inverse merge → commit). Scales with the
/// diff size.
fn revert_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    commit_id: &CommitId,
) -> Result<(String, StateSummary), String> {
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.preflight_check(plan)
        .map_err(|e| format!("Preflight failed: {}", e))?;

    let new_id = repo
        .execute_revert(commit_id)
        .map_err(|e| format!("Revert failed: {}", e))?;
    eprintln!(
        "[kagi] executed: revert {} -> {}",
        commit_id.short(),
        new_id.short()
    );

    let after = verify_new_commit_snapshot(repo_path, plan, &new_id, "revert");
    Ok((format!("reverted {}", commit_id.short()), after))
}

/// Blocking part of commit (tree-build + write). Scales with the staged tree.
/// Returns the new commit id alongside the after-state so the UI finish step can
/// clear the branch draft on the main thread.
fn commit_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    message: &str,
) -> Result<(String, StateSummary), String> {
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;

    let new_id = repo
        .execute_commit(message)
        .map_err(|e| format!("Commit failed: {}", e))?;
    eprintln!("[kagi] executed: commit {}", new_id.short());

    // Verify: re-snapshot, check HEAD is the new commit, unstaged remain.
    let after = match kagi::git::Backend::open(repo_path) {
        Ok(mut repo2) => match repo2.snapshot(10_000) {
            Ok(snap) => {
                if let Head::Attached { target, branch } = &snap.head {
                    if *target == new_id.0 {
                        eprintln!(
                            "[kagi] verified: commit HEAD={} on {}",
                            new_id.short(),
                            branch
                        );
                    } else {
                        eprintln!("[kagi] verify: HEAD mismatch after commit");
                    }
                }
                let is_dirty = snap.status.is_dirty();
                eprintln!(
                    "[kagi] verified: working tree {} after commit",
                    if is_dirty {
                        "dirty (unstaged remain)"
                    } else {
                        "clean"
                    }
                );
                StateSummary {
                    head: snap.head.display(),
                    dirty: if is_dirty {
                        "dirty".to_string()
                    } else {
                        "clean".to_string()
                    },
                }
            }
            Err(e) => {
                eprintln!("[kagi] verify: snapshot error: {}", e);
                plan.predicted.clone()
            }
        },
        Err(e) => {
            eprintln!("[kagi] verify: repo open error: {}", e);
            plan.predicted.clone()
        }
    };
    Ok((new_id.short().to_string(), after))
}

/// Blocking part of stash-pop (preflight + apply-then-drop). Re-snapshots HEAD
/// for the after-state.
fn stash_pop_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    stash_index: usize,
) -> Result<(String, StateSummary), String> {
    let mut repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.preflight_check(plan)
        .map_err(|e| format!("Preflight failed: {}", e))?;

    repo.execute_stash_pop(stash_index)
        .map_err(|e| format!("Pop failed: {}", e))?;
    eprintln!("[kagi] executed: stash-pop index={}", stash_index);

    let after = StateSummary {
        head: plan.current.head.clone(),
        dirty: "changes restored (stash removed)".to_string(),
    };
    Ok(("applied and dropped".to_string(), after))
}

/// Blocking part of standalone stash drop (ADR-0087). Deletes the stash entry
/// without touching the working tree; returns the dropped stash commit OID as
/// the oplog recovery handle.
fn stash_drop_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    stash_index: usize,
) -> Result<(String, StateSummary), String> {
    let mut repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.preflight_check_stash(plan, plan.stash_count_at_plan())
        .map_err(|e| format!("Preflight failed: {}", e))?;

    let dropped_oid = repo
        .execute_stash_drop(stash_index)
        .map_err(|e| format!("Drop failed: {}", e))?;
    eprintln!(
        "[kagi] executed: stash-drop index={} oid={}",
        stash_index, dropped_oid
    );

    let after = StateSummary {
        head: plan.current.head.clone(),
        dirty: format!("stash@{{{}}} deleted (oid {})", stash_index, dropped_oid),
    };
    Ok(("entry deleted".to_string(), after))
}

/// Blocking part of discard (W17-DISCARD, ADR-0046). Backup-then-discard scales
/// with the working-tree content written, so it runs on the background path.
/// The returned `after` carries the path→blob backup list (the recovery handle)
/// into the oplog entry.
fn discard_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    paths: &[String],
) -> Result<(String, StateSummary), String> {
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;

    let outcome = repo
        .execute_discard(plan, paths)
        .map_err(|e| format!("Discard failed: {}", e))?;
    let summary = outcome.oplog_summary();
    eprintln!("[kagi] executed: {}", summary);

    // Verify: re-read status; targets must have left the unstaged set.
    let dirty = match repo.working_tree_status() {
        Ok(status) => {
            let still: std::collections::HashSet<String> = status
                .unstaged
                .iter()
                .map(|f| f.path.to_string_lossy().replace('\\', "/"))
                .collect();
            let leftover = paths.iter().filter(|p| still.contains(*p)).count();
            if leftover == 0 {
                eprintln!(
                    "[kagi] verified: {} target(s) left the unstaged set",
                    paths.len()
                );
            } else {
                eprintln!("[kagi] verify: {} target(s) still unstaged", leftover);
            }
            // Record the recovery handle (path→blob list) in the oplog after-state.
            summary.clone()
        }
        Err(e) => {
            eprintln!("[kagi] verify: status error: {}", e);
            summary.clone()
        }
    };

    let after = StateSummary {
        head: plan.current.head.clone(),
        dirty,
    };
    let human = if outcome.backups.len() == 1 {
        format!("{} discarded", outcome.backups[0].path)
    } else {
        format!("{} files discarded", outcome.backups.len())
    };
    Ok((human, after))
}

/// Blocking part of amend (history rewrite: tree-build + commit-replace).
/// Returns (summary-suffix, after, old, new) so the UI footer can render the
/// 旧→新 SHA transition and the restore hint.
fn amend_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    mode: AmendMode,
    message: &str,
) -> Result<(StateSummary, CommitId, CommitId), String> {
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.preflight_check(plan)
        .map_err(|e| format!("Preflight failed: {}", e))?;

    let msg_opt = if message.trim().is_empty() {
        None
    } else {
        Some(message)
    };
    let outcome = repo
        .execute_amend(mode, msg_opt)
        .map_err(|e| format!("Amend failed: {}", e))?;
    eprintln!(
        "[kagi] executed: amend {} -> {}",
        outcome.old.short(),
        outcome.new.short()
    );

    let after = StateSummary {
        head: format!(
            "branch @ {} (was {})",
            outcome.new.short(),
            outcome.old.short()
        ),
        dirty: "amended".to_string(),
    };
    Ok((after, outcome.old, outcome.new))
}

/// Blocking part of delete-branch (preflight → ref delete). Lightweight, but
/// kept on the background path for consistency with the other confirm flows.
fn delete_branch_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    branch_name: &str,
) -> Result<StateSummary, String> {
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.preflight_check(plan)
        .map_err(|e| format!("Preflight failed: {}", e))?;

    repo.execute_delete_branch(plan, branch_name)
        .map_err(|e| format!("Delete failed: {}", e))?;
    eprintln!("[kagi] executed: delete-branch {}", branch_name);

    Ok(StateSummary {
        head: plan.current.head.clone(),
        dirty: format!("branch '{}' deleted", branch_name),
    })
}

fn branch_plan_blocking(
    repo_path: &std::path::Path,
    modal: &BranchPlanModal,
) -> Result<StateSummary, String> {
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    match modal.kind {
        BranchPlanKind::PullFfOnly => {
            let outcome = repo
                .execute_pull_branch_ff(&modal.plan, &modal.branch_name)
                .map_err(|e| format!("Pull failed: {}", e))?;
            let dirty = match outcome {
                PullOutcome::UpToDate => {
                    format!("branch '{}' already up to date", modal.branch_name)
                }
                PullOutcome::FastForward { to } => {
                    format!(
                        "branch '{}' fast-forwarded to {}",
                        modal.branch_name,
                        to.short()
                    )
                }
                PullOutcome::Merged { .. } => "unexpected merge outcome".to_string(),
            };
            Ok(StateSummary {
                head: modal.plan.current.head.clone(),
                dirty,
            })
        }
        BranchPlanKind::Push | BranchPlanKind::PushSetUpstream => {
            let set_upstream = modal.kind == BranchPlanKind::PushSetUpstream;
            let outcome = repo
                .execute_push_branch(&modal.plan, &modal.branch_name, set_upstream)
                .map_err(|e| format!("Push failed: {}", e))?;
            Ok(StateSummary {
                head: modal.plan.current.head.clone(),
                dirty: format!(
                    "branch '{}' pushed {} commit(s){}",
                    modal.branch_name,
                    outcome.pushed,
                    if outcome.set_upstream {
                        " and upstream set"
                    } else {
                        ""
                    }
                ),
            })
        }
    }
}

fn set_upstream_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    branch_name: &str,
    upstream: &str,
) -> Result<StateSummary, String> {
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.execute_set_upstream(plan, branch_name, upstream)
        .map_err(|e| format!("Set upstream failed: {}", e))?;
    Ok(StateSummary {
        head: plan.current.head.clone(),
        dirty: format!("branch '{}' upstream set to '{}'", branch_name, upstream),
    })
}

fn rename_branch_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    old_name: &str,
    new_name: &str,
) -> Result<StateSummary, String> {
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.execute_rename_branch(plan, old_name, new_name)
        .map_err(|e| format!("Rename failed: {}", e))?;
    Ok(StateSummary {
        head: plan.predicted.head.clone(),
        dirty: format!("branch '{}' renamed to '{}'", old_name, new_name),
    })
}

/// Blocking part of create-worktree (checks out a full tree into a new linked
/// worktree on disk — scales with tree size).
fn create_worktree_blocking(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    branch_input: &str,
    path_input: &str,
    at: &CommitId,
    allow_existing_branch: bool,
) -> Result<StateSummary, String> {
    let repo =
        kagi::git::Backend::open(repo_path).map_err(|e| format!("Repo open error: {}", e))?;
    repo.preflight_check(plan)
        .map_err(|e| format!("Preflight failed: {}", e))?;

    if allow_existing_branch {
        repo.execute_open_worktree_for_branch(branch_input, path_input)
            .map_err(|e| format!("Open worktree failed: {}", e))?;
    } else {
        repo.execute_create_worktree(branch_input, path_input, at)
            .map_err(|e| format!("Create worktree failed: {}", e))?;
    }
    eprintln!(
        "[kagi] executed: create-worktree '{}' path='{}' @ {}",
        branch_input,
        path_input,
        at.short()
    );

    // Verify: open the linked worktree and log its HEAD.
    let verify_path = {
        let path = std::path::PathBuf::from(path_input);
        if path.is_absolute() {
            path
        } else {
            repo_path.join(path)
        }
    };
    match kagi::git::Backend::open(&verify_path) {
        Ok(linked) => {
            let head = linked.head_shorthand();
            eprintln!(
                "[kagi] verified: worktree '{}' HEAD={}",
                verify_path.display(),
                head.unwrap_or_else(|| "?".to_string())
            );
        }
        Err(e) => eprintln!("[kagi] verify: worktree open error: {}", e),
    }

    Ok(plan.predicted.clone())
}

/// Re-snapshot after a new-commit op (cherry-pick / revert) for the after-state,
/// logging the verified HEAD. Falls back to the plan prediction on failure.
fn verify_new_commit_snapshot(
    repo_path: &std::path::Path,
    plan: &OperationPlan,
    new_id: &CommitId,
    op: &str,
) -> StateSummary {
    match kagi::git::Backend::open(repo_path) {
        Ok(mut repo2) => match repo2.snapshot(10_000) {
            Ok(snap) => {
                if let Head::Attached { target, branch } = &snap.head {
                    if *target == new_id.0 {
                        eprintln!(
                            "[kagi] verified: {} HEAD={} on {}",
                            op,
                            new_id.short(),
                            branch
                        );
                    } else {
                        eprintln!(
                            "[kagi] verify: HEAD={} expected {}",
                            &target[..8.min(target.len())],
                            new_id.short()
                        );
                    }
                    let is_clean = !snap.status.is_dirty();
                    eprintln!(
                        "[kagi] verified: working tree {}",
                        if is_clean {
                            "clean"
                        } else {
                            "dirty (unexpected)"
                        }
                    );
                }
                StateSummary {
                    head: snap.head.display(),
                    dirty: if snap.status.is_dirty() {
                        "dirty".to_string()
                    } else {
                        "clean".to_string()
                    },
                }
            }
            Err(e) => {
                eprintln!("[kagi] verify: snapshot error: {}", e);
                plan.predicted.clone()
            }
        },
        Err(e) => {
            eprintln!("[kagi] verify: repo open error: {}", e);
            plan.predicted.clone()
        }
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
        .h(theme::scaled_px(22.))
        .flex_shrink_0()
        .px_3()
        .bg(rgb(theme().panel))
        .text_xs()
        .text_color(rgb(text_color))
        .overflow_hidden()
        .child(text)
}

/// W12-GCADOPT (§2.10): wrap a virtualized list in a relative flex column and
/// overlay a `gpui_component::scroll::Scrollbar` driven by the list's existing
/// `UniformListScrollHandle`.  The Scrollbar paints itself absolutely-positioned
/// over the container (relative(1.) size), so this is layout-non-destructive —
/// the inner `uniform_list` keeps its own `flex_1().min_h(0)` sizing.  Colours
/// follow the gpui-component scrollbar theme fields, which
/// `sync_gpui_component_theme` keeps in step with kagi's palette.
/// `show_bar` controls whether the overlay scrollbar is rendered. `false` hides
/// it entirely (the list still scrolls via wheel/trackpad) — used for the commit
/// stage/unstage lists, which the user wants free of a visible scrollbar. When
/// `true` the bar follows the theme default (`cx.theme().scrollbar_show`, which
/// honours the macOS "show scroll bars" setting).
pub(super) fn with_vertical_scrollbar(
    id: &'static str,
    handle: &UniformListScrollHandle,
    list: impl IntoElement,
    show_bar: bool,
) -> impl IntoElement {
    let mut container = div()
        .id(id)
        .relative()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .child(list);
    if show_bar {
        container = container.child(Scrollbar::vertical(handle));
    }
    container
}

/// Unstaged file-row context menu (right-click). Single item: Discard.
///
/// Only attached to eligible rows (tracked, non-conflicted), so the item is
/// always actionable. Backdrop click dismisses; backdrop AND card `.occlude()`
/// (click-through bug).
fn render_file_menu_overlay(
    fi: usize,
    pos: gpui::Point<gpui::Pixels>,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let dismiss = cx.listener(|this, _e: &gpui::MouseDownEvent, _window, cx| {
        this.file_menu = None;
        cx.notify();
    });
    let discard_click = cx.listener(move |this, _e: &gpui::ClickEvent, _window, cx| {
        this.file_menu = None;
        this.open_discard_modal_for_index(fi);
        cx.notify();
    });
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(MouseButton::Left, dismiss)
        .child(
            div()
                .absolute()
                .left(pos.x)
                .top(pos.y)
                .w(theme::scaled_px(180.))
                .occlude()
                .bg(rgb(theme().panel))
                .border_1()
                .border_color(rgb(theme().surface))
                .rounded_md()
                .shadow_lg()
                // W27-UIPOLISH: compact (Zed-style) density — tighter vertical
                // padding to match the commit/branch context menus.
                .py(theme::scaled_px(2.))
                .child(
                    div()
                        .id(("file-menu-discard", fi))
                        .px_3()
                        .py(theme::scaled_px(3.))
                        .text_sm()
                        .text_color(rgb(theme().color_blocker))
                        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                        .on_click(discard_click)
                        .child(SharedString::from("Discard changes…")),
                ),
        )
        .into_any_element()
}

// ──────────────────────────────────────────────────────────────
// Commit Panel — virtualized per-row builders (PERF)
// ──────────────────────────────────────────────────────────────
//
// These free functions build a SINGLE file row, reading live data from
// `this.commit_panel` (NOT a captured-by-value clone).  They are invoked from
// the `uniform_list` processors below for only the visible `range`, so the
// commit panel costs O(visible rows) per frame instead of O(all files).

/// PERF: recompute the WIP-highlight target from the open main diff.
/// `Some((staged, path))` when a WIP (unstaged/staged) file is open in the
/// center diff; mirrors the value the old call site passed in by value.
fn cp_active_wip(this: &KagiApp) -> Option<(bool, PathBuf)> {
    match this.main_diff.as_ref().map(|d| &d.source) {
        Some(MainDiffSource::Unstaged { path }) => Some((false, path.clone())),
        Some(MainDiffSource::Staged { path }) => Some((true, path.clone())),
        _ => None,
    }
}

/// PERF: build one unstaged row in flat view (index `fi` into `unstaged`).
fn render_unstaged_flat_row(
    this: &KagiApp,
    fi: usize,
    cx: &mut Context<KagiApp>,
) -> Option<gpui::AnyElement> {
    let panel = this.commit_panel.as_ref()?;
    let f = panel.unstaged.get(fi)?;
    let selected_file = panel.selected_file.clone();
    let active_wip = cp_active_wip(this);

    let name = f
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| f.path.to_string_lossy().into_owned());
    let is_conflicted_file = panel.is_conflicted(&f.path);
    let (badge, badge_color, _) = status_badge(&f.change, is_conflicted_file);
    let is_sel = selected_file == Some(CommitPanelFileRef::Unstaged { index: fi });
    let stat = panel.unstaged_stat(&f.path).cloned();
    let wip_hit = active_wip
        .as_ref()
        .is_some_and(|(st, p)| !*st && &f.path == p);

    let file_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.select_commit_panel_file(CommitPanelFileRef::Unstaged { index: fi });
        cx.notify();
    });
    let stage_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.do_stage_file(fi);
        cx.notify();
    });
    // Row background: conflicted files get red tint
    let row_bg = if is_conflicted_file {
        theme().diff_removed_bg
    } else if is_sel {
        theme().selected
    } else {
        theme().panel
    };
    let mut file_row = div()
        .id(("cp-us-flat-file", fi))
        .when(wip_hit, |el| el.bg(rgb(theme().selected)))
        .w_full()
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
                .w(theme::scaled_px(12.))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(badge_color))
                .child(SharedString::from(badge)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_xs()
                .text_color(rgb(theme().text_main))
                .overflow_hidden()
                .truncate()
                .child(SharedString::from(name)),
        )
        .child(diffstat_bar::diffstat_unit(fi, stat.as_ref()));
    // Stage button only for non-conflicted files
    if !is_conflicted_file {
        // W17-DISCARD / ADR-0083: right-click opens the file context menu
        // (Discard lives there). Tracked rows are restored from the index;
        // untracked rows are deleted (after an ODB backup).
        let menu_click = cx.listener(move |this, e: &gpui::MouseDownEvent, _window, cx| {
            this.file_menu = Some((fi, e.position));
            cx.stop_propagation();
            cx.notify();
        });
        file_row = file_row.on_mouse_down(MouseButton::Right, menu_click);
        file_row = file_row.child(
            div()
                .id(("cp-us-flat-stage-btn", fi))
                .ml_2()
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
                .ml_2()
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
    Some(file_row.into_any_element())
}

/// PERF: build one unstaged tree row (index `row_index` into `unstaged_tree`).
fn render_unstaged_tree_row(
    this: &KagiApp,
    row_index: usize,
    cx: &mut Context<KagiApp>,
) -> Option<gpui::AnyElement> {
    let panel = this.commit_panel.as_ref()?;
    let row = panel.unstaged_tree.get(row_index)?.clone();
    let selected_file = panel.selected_file.clone();
    let active_wip = cp_active_wip(this);

    match row {
        file_tree::TreeRow::Dir { depth, name } => {
            let indent = (depth as f32) * 12.0;
            Some(
                div()
                    .id(SharedString::from(format!("cp-us-dir-{}", name.as_ref())))
                    .pl(theme::scaled_px(8.0 + indent))
                    .py_px()
                    .text_xs()
                    .text_color(rgb(theme().change_dir))
                    .child(name.clone())
                    .into_any_element(),
            )
        }
        file_tree::TreeRow::File {
            depth,
            name,
            file_index,
            change,
        } => {
            let indent = (depth as f32) * 12.0;
            let fi = file_index;
            // Look up the original path to check if conflicted
            let path = panel.unstaged.get(fi).map(|f| f.path.clone());
            let is_conflicted_file = path
                .as_ref()
                .map(|p| panel.is_conflicted(p))
                .unwrap_or(false);
            let (badge, badge_color, _) = status_badge(&change, is_conflicted_file);
            let is_sel = selected_file == Some(CommitPanelFileRef::Unstaged { index: fi });
            let stat = path.as_ref().and_then(|p| panel.unstaged_stat(p)).cloned();
            let wip_hit = active_wip
                .as_ref()
                .zip(path.as_ref())
                .is_some_and(|((st, p), fp)| !*st && fp == p);

            let file_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.select_commit_panel_file(CommitPanelFileRef::Unstaged { index: fi });
                cx.notify();
            });
            let stage_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.do_stage_file(fi);
                cx.notify();
            });
            let row_bg = if is_conflicted_file {
                theme().diff_removed_bg
            } else if is_sel {
                theme().selected
            } else {
                theme().panel
            };
            let mut file_row = div()
                .id(("cp-us-file", fi))
                .when(wip_hit, |el| el.bg(rgb(theme().selected)))
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .pl(theme::scaled_px(8.0 + indent))
                .pr(theme::scaled_px(2.0))
                .py_px()
                .bg(rgb(row_bg))
                .hover(|s| s.bg(rgb(theme().surface)))
                .on_click(file_click)
                .child(
                    div()
                        .w(theme::scaled_px(12.))
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(rgb(badge_color))
                        .child(SharedString::from(badge)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_xs()
                        .text_color(rgb(theme().text_main))
                        .overflow_hidden()
                        .truncate()
                        .child(name.clone()),
                )
                .child(diffstat_bar::diffstat_unit(fi, stat.as_ref()));
            if !is_conflicted_file {
                // W17-DISCARD / ADR-0083: right-click opens the file context menu
                // (Discard lives there). Untracked rows are discardable too —
                // deleted from disk after an ODB backup.
                let menu_click = cx.listener(move |this, e: &gpui::MouseDownEvent, _window, cx| {
                    this.file_menu = Some((fi, e.position));
                    cx.stop_propagation();
                    cx.notify();
                });
                file_row = file_row.on_mouse_down(MouseButton::Right, menu_click);
                file_row = file_row.child(
                    div()
                        .id(("cp-us-stage-btn", fi))
                        .ml_2()
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
                        .ml_2()
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
            Some(file_row.into_any_element())
        }
    }
}

/// PERF: build one staged row in flat view (index `fi` into `staged`).
fn render_staged_flat_row(
    this: &KagiApp,
    fi: usize,
    cx: &mut Context<KagiApp>,
) -> Option<gpui::AnyElement> {
    let panel = this.commit_panel.as_ref()?;
    let f = panel.staged.get(fi)?;
    let selected_file = panel.selected_file.clone();
    let active_wip = cp_active_wip(this);

    let name = f
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| f.path.to_string_lossy().into_owned());
    let (badge, badge_color, _conflicted) = status_badge(&f.change, false);
    let is_sel = selected_file == Some(CommitPanelFileRef::Staged { index: fi });
    let stat = panel.staged_stat(&f.path).cloned();
    let wip_hit = active_wip
        .as_ref()
        .is_some_and(|(st, p)| *st && &f.path == p);

    let file_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.select_commit_panel_file(CommitPanelFileRef::Staged { index: fi });
        cx.notify();
    });
    let unstage_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
        this.do_unstage_file(fi);
        cx.notify();
    });
    Some(
        div()
            .id(("cp-st-flat-file", fi))
            .when(wip_hit, |el| el.bg(rgb(theme().selected)))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .px_2()
            .py_px()
            .bg(rgb(if is_sel {
                theme().selected
            } else {
                theme().panel
            }))
            .hover(|s| s.bg(rgb(theme().surface)))
            .on_click(file_click)
            .child(
                div()
                    .w(theme::scaled_px(12.))
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(rgb(badge_color))
                    .child(SharedString::from(badge)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_xs()
                    .text_color(rgb(theme().text_main))
                    .overflow_hidden()
                    .truncate()
                    .child(SharedString::from(name)),
            )
            .child(diffstat_bar::diffstat_unit(fi + 100_000, stat.as_ref()))
            .child(
                div()
                    .id(("cp-st-flat-unstage-btn", fi))
                    .ml_2()
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
            )
            .into_any_element(),
    )
}

/// PERF: build one staged tree row (index `row_index` into `staged_tree`).
fn render_staged_tree_row(
    this: &KagiApp,
    row_index: usize,
    cx: &mut Context<KagiApp>,
) -> Option<gpui::AnyElement> {
    let panel = this.commit_panel.as_ref()?;
    let row = panel.staged_tree.get(row_index)?.clone();
    let selected_file = panel.selected_file.clone();
    let active_wip = cp_active_wip(this);

    match row {
        file_tree::TreeRow::Dir { depth, name } => {
            let indent = (depth as f32) * 12.0;
            Some(
                div()
                    .id(SharedString::from(format!("cp-st-dir-{}", name.as_ref())))
                    .pl(theme::scaled_px(8.0 + indent))
                    .py_px()
                    .text_xs()
                    .text_color(rgb(theme().change_dir))
                    .child(name.clone())
                    .into_any_element(),
            )
        }
        file_tree::TreeRow::File {
            depth,
            name,
            file_index,
            change,
        } => {
            let indent = (depth as f32) * 12.0;
            let fi = file_index;
            let (badge, badge_color, _conflicted) = status_badge(&change, false);
            let is_sel = selected_file == Some(CommitPanelFileRef::Staged { index: fi });
            let path = panel.staged.get(fi).map(|f| f.path.clone());
            let stat = path.as_ref().and_then(|p| panel.staged_stat(p)).cloned();
            let wip_hit = active_wip
                .as_ref()
                .zip(path.as_ref())
                .is_some_and(|((st, p), fp)| *st && fp == p);

            let file_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.select_commit_panel_file(CommitPanelFileRef::Staged { index: fi });
                cx.notify();
            });
            let unstage_click = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.do_unstage_file(fi);
                cx.notify();
            });
            Some(
                div()
                    .id(("cp-st-file", fi))
                    .when(wip_hit, |el| el.bg(rgb(theme().selected)))
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .pl(theme::scaled_px(8.0 + indent))
                    .pr(theme::scaled_px(2.0))
                    .py_px()
                    .bg(rgb(if is_sel {
                        theme().selected
                    } else {
                        theme().panel
                    }))
                    .hover(|s| s.bg(rgb(theme().surface)))
                    .on_click(file_click)
                    .child(
                        div()
                            .w(theme::scaled_px(12.))
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(rgb(badge_color))
                            .child(SharedString::from(badge)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_xs()
                            .text_color(rgb(theme().text_main))
                            .overflow_hidden()
                            .truncate()
                            .child(name.clone()),
                    )
                    .child(diffstat_bar::diffstat_unit(fi + 100_000, stat.as_ref()))
                    .child(
                        div()
                            .id(("cp-st-unstage-btn", fi))
                            .ml_2()
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
                    )
                    .into_any_element(),
            )
        }
    }
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
    template_mode: bool,
    template_inputs: Option<[Entity<InputState>; 6]>,
    // PERF: WIP highlight is now recomputed per visible row from `this.main_diff`
    // inside the uniform_list processors; this parameter is retained for the
    // stable call-site signature.
    _active_wip: Option<(bool, PathBuf)>,
    smart: smart_commit::SmartCommitState,
    preview: Option<kagi::git::CommitPreview>,
    unstaged_scroll_handle: UniformListScrollHandle,
    staged_scroll_handle: UniformListScrollHandle,
    cx: &mut Context<KagiApp>,
) -> impl IntoElement {
    // theme().change_dir now sourced from theme().change_dir (W9-THEME).

    let tree_view = panel.tree_view;
    let unstaged_count = panel.unstaged.len();
    let staged_count = panel.staged.len();
    // W17-DISCARD: count discard-eligible unstaged files (exclude untracked,
    // which the panel surfaces as `Added` rows, and conflicted files).
    // ADR-0083: untracked (`Added`) rows ARE discardable (deleted with backup),
    // so they count toward enabling "Discard all" — only conflicted rows are
    // excluded. Must mirror `discard_partition`.
    let discard_eligible_count = panel
        .unstaged
        .iter()
        .filter(|f| !panel.is_conflicted(&f.path))
        .count();
    // T026 / T-COMMIT-009: can_commit uses the effective message — in template
    // mode the assembled fields, else the plain Input value (headless: commit_msg).
    let input_msg_nonempty = if template_mode {
        // Non-empty when summary or any field yields a non-empty assembled message.
        template_inputs
            .as_ref()
            .map(|inp| {
                let fields = kagi::git::TemplateFields::new(
                    inp[0].read(cx).value().to_string(),
                    inp[1].read(cx).value().to_string(),
                    inp[2].read(cx).value().to_string(),
                    inp[3].read(cx).value().to_string(),
                    inp[4].read(cx).value().to_string(),
                    inp[5].read(cx).value().to_string(),
                );
                !kagi::git::assemble(&fields).trim().is_empty()
            })
            .unwrap_or(false)
    } else {
        commit_input
            .as_ref()
            .map(|e| !e.read(cx).value().trim().is_empty())
            .unwrap_or(!panel.commit_msg.trim().is_empty())
    };
    let can_commit = !panel.staged.is_empty() && input_msg_nonempty;
    let has_unstaged_warning = !panel.unstaged.is_empty() && staged_count > 0;
    // PERF: selected_file is read per visible row from `this.commit_panel`
    // inside the uniform_list processors, not captured here.

    // ── View switch: segmented [List | Tree] (T-UI-002) ──────
    let list_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
        if let Some(panel) = this.commit_panel.as_mut() {
            panel.tree_view = false;
        }
        cx.notify();
    });
    let tree_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
        if let Some(panel) = this.commit_panel.as_mut() {
            panel.tree_view = true;
        }
        cx.notify();
    });
    let seg = |id: &'static str, label: &'static str, active: bool| {
        div()
            .id(id)
            .px_1p5()
            .py_px()
            .text_xs()
            .bg(rgb(if active {
                theme().selected
            } else {
                theme().surface
            }))
            .text_color(rgb(if active {
                theme().text_main
            } else {
                theme().text_muted
            }))
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
        // W17-DISCARD: "Discard all" — disabled (muted, no handler) at 0 targets.
        .when(unstaged_count > 0, |el| {
            let discard_all_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
                this.open_discard_all_modal();
                cx.notify();
            });
            let enabled = discard_eligible_count > 0;
            let mut btn = div()
                .id("cp-discard-all")
                .mr_2()
                .px_1p5()
                .py_px()
                .rounded_sm()
                .bg(rgb(theme().surface))
                .text_xs()
                .child(SharedString::from("Discard all"));
            if enabled {
                btn = btn
                    .text_color(rgb(theme().color_blocker))
                    .hover(|st| st.bg(rgb(theme().selected)).cursor_pointer())
                    .on_click(discard_all_click);
            } else {
                btn = btn.text_color(rgb(theme().text_muted));
            }
            el.child(btn)
        })
        .child(toggle_btn);

    // PERF: unstaged file rows are virtualized via `uniform_list` (built from
    // free row functions reading `this.commit_panel`), not a prebuilt div.
    let unstaged_row_count = if tree_view {
        panel.unstaged_tree.len()
    } else {
        unstaged_count
    };

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

    // PERF: staged file rows are virtualized via `uniform_list` (built from
    // free row functions reading `this.commit_panel`), not a prebuilt div.
    let staged_row_count = if tree_view {
        panel.staged_tree.len()
    } else {
        staged_count
    };

    // ── plain ⇄ template mode toggle (T-COMMIT-009) ───────────────
    let mode_toggle = {
        let toggle_click = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
            this.toggle_commit_template_mode(window, cx);
        });
        let label = if template_mode {
            "Plain message"
        } else {
            "Template fields"
        };
        div()
            .id("cp-template-toggle")
            .px_1p5()
            .py_px()
            .rounded_sm()
            .text_xs()
            .bg(rgb(theme().surface))
            .text_color(rgb(theme().color_branch))
            .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
            .on_click(toggle_click)
            .child(SharedString::from(format!("⇄ {}", label)))
    };

    // ── Commit message input (T026/T-COMMIT-009) ──────────────────
    // Template mode renders the six structured fields (gpui-component Input for
    // each — no hand-written widgets); plain mode renders the single Input.
    let msg_input_wrapper: gpui::AnyElement = if template_mode {
        if let Some(inp) = template_inputs.clone() {
            let [ty, scope, summary, body, test, risk] = inp;

            // Labeled single-line field.
            let field = |label: &'static str, state: &Entity<InputState>| {
                div()
                    .flex()
                    .flex_col()
                    .gap_px()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().text_label))
                            .child(SharedString::from(label)),
                    )
                    .child(Input::new(state).appearance(true).bordered(true))
            };

            // type quick-pick chips (also free-typeable in the type field above).
            let mut chips = div().flex().flex_row().flex_wrap().gap_1();
            for &choice in kagi::git::TYPE_CHOICES {
                let ty_state = ty.clone();
                let pick = cx.listener(move |_this, _e: &gpui::ClickEvent, window, cx| {
                    ty_state.update(cx, |s, cx| s.set_value(choice.to_string(), window, cx));
                });
                chips = chips.child(
                    div()
                        .id(SharedString::from(format!("cp-type-chip-{}", choice)))
                        .px_1()
                        .py_px()
                        .rounded_sm()
                        .text_xs()
                        .bg(rgb(theme().surface))
                        .text_color(rgb(theme().text_main))
                        .hover(|s| s.bg(rgb(theme().selected)).cursor_pointer())
                        .on_click(pick)
                        .child(SharedString::from(choice)),
                );
            }

            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(field("type", &ty))
                .child(chips)
                .child(field("scope", &scope))
                .child(field("summary", &summary))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_px()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme().text_label))
                                .child(SharedString::from("body")),
                        )
                        .child(Input::new(&body).appearance(true).bordered(true)),
                )
                .child(field("test", &test))
                .child(field("risk", &risk))
                .into_any_element()
        } else {
            // Template mode requested but inputs not yet created (no &mut Window
            // here) — should not occur because the toggle creates them.
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from("(template fields unavailable)"))
                .into_any_element()
        }
    } else if let Some(ref input_entity) = commit_input {
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
            .child(SharedString::from(format!(
                "Commit ({} file{})",
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

    // ── Smart Commit Message toolbar (T-COMMIT-016) ───────────
    // Rule-based "Suggest" is always available; "Generate with Local LLM" is
    // offered only when an Ollama server is detected and the user opted in.
    let staged_empty = panel.staged.is_empty();
    let smart_toolbar = {
        // Small reusable button factory.
        let pill = |id: &'static str, label: SharedString, enabled: bool, accent: u32| {
            let mut b = div()
                .id(id)
                .px_1p5()
                .py_px()
                .rounded_sm()
                .text_xs()
                .bg(rgb(theme().surface))
                .text_color(rgb(if enabled { accent } else { theme().text_muted }))
                .child(label);
            if enabled {
                b = b.hover(|s| s.bg(rgb(theme().selected)).cursor_pointer());
            }
            b
        };

        // Suggest — one button: uses the local LLM when it's usable (green),
        // otherwise the rule-based draft (blue). Shows "Generating…" while the
        // LLM runs. (The separate "Generate with Local LLM" button is gone.)
        let llm_on = smart.llm_offered();
        let suggest_enabled = !staged_empty && !smart.generating;
        let suggest_color = if llm_on {
            theme().color_success
        } else {
            theme().color_branch
        };
        let suggest_btn: gpui::AnyElement = if smart.generating {
            // Animated braille "dots" spinner while the LLM generates (user
            // request — the spinning-dots glyph). The whole panel re-renders each
            // animation frame, so the closure rebuilds a fresh single-child div.
            use gpui::AnimationExt as _;
            const FRAMES: [&str; 10] = [
                "\u{280B}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283C}", "\u{2834}", "\u{2826}",
                "\u{2827}", "\u{2807}", "\u{280F}",
            ];
            let spinner = div()
                .text_xs()
                .text_color(rgb(suggest_color))
                .with_animation(
                    "cp-smart-spinner",
                    gpui::Animation::new(Duration::from_millis(800)).repeat(),
                    |el, delta| {
                        let i = ((delta * FRAMES.len() as f32) as usize).min(FRAMES.len() - 1);
                        el.child(SharedString::from(FRAMES[i]))
                    },
                );
            div()
                .id("cp-smart-suggest")
                .px_1p5()
                .py_px()
                .rounded_sm()
                .text_xs()
                .bg(rgb(theme().surface))
                .text_color(rgb(suggest_color))
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(spinner)
                .child(SharedString::from("Generating…"))
                .into_any_element()
        } else {
            let mut b = pill(
                "cp-smart-suggest",
                SharedString::from("Suggest"),
                suggest_enabled,
                suggest_color,
            );
            if suggest_enabled {
                let suggest_click = cx.listener(move |this, _e: &gpui::ClickEvent, window, cx| {
                    if llm_on {
                        this.smart_generate(window, cx);
                    } else {
                        this.smart_suggest(window, cx);
                    }
                });
                b = b.on_click(suggest_click);
            }
            b.into_any_element()
        };

        // Lang toggle (En / 日本語).
        let lang_label = match smart.lang {
            message_gen::Lang::En => "Lang: EN",
            message_gen::Lang::Ja => "Lang: 日本語",
        };
        let lang_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
            this.smart_commit.toggle_lang();
            cx.notify();
        });
        let lang_btn = pill(
            "cp-smart-lang",
            SharedString::from(lang_label),
            true,
            theme().text_main,
        )
        .on_click(lang_click);

        // Style toggle (Conventional / Plain).
        let style_label = match smart.style {
            message_gen::Style::ConventionalCommits => "Style: CC",
            message_gen::Style::Plain => "Style: Plain",
        };
        let style_click = cx.listener(|this, _e: &gpui::ClickEvent, _window, cx| {
            this.smart_commit.toggle_style();
            cx.notify();
        });
        let style_btn = pill(
            "cp-smart-style",
            SharedString::from(style_label),
            true,
            theme().text_main,
        )
        .on_click(style_click);

        let mut row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .child(suggest_btn)
            .child(lang_btn)
            .child(style_btn);

        // "Generate with Local LLM" is folded into Suggest (above). When the LLM
        // is detected but not yet enabled, offer an opt-in affordance so the user
        // can turn it on (after which Suggest goes green and uses it).
        if smart.ollama_available && !smart.llm_enabled {
            let enable_click = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
                this.smart_generate(window, cx);
            });
            let enable_btn = pill(
                "cp-smart-enable-llm",
                SharedString::from("Enable Local LLM…"),
                !staged_empty,
                theme().color_success,
            )
            .when(!staged_empty, |el| el.on_click(enable_click));
            row = row.child(enable_btn);
        }

        // "Local LLM available" indicator.
        let mut col = div().flex().flex_col().gap_px().child(row);
        if smart.ollama_available {
            col = col.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().color_success))
                    .child(SharedString::from("● Local LLM available")),
            );
        }
        // Transient status line (rule-based inserted / generating / fell back).
        if let Some(ref status) = smart.status {
            col = col.child(
                div()
                    .text_xs()
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(status.clone())),
            );
        }
        col
    };

    // ── Commit preview header (T-COMMIT-001) ──────────────────
    // Shows what the *next* commit contains: staged count, A/M/D summary,
    // target branch (detached/unborn handled), and author.  Pure read from
    // `commit_preview()`; hidden if the preview could not be built.
    let preview_block: gpui::AnyElement = if let Some(ref pv) = preview {
        let count_line = format!(
            "{} file{} staged",
            pv.staged_count,
            if pv.staged_count == 1 { "" } else { "s" }
        );
        let summary = pv.summary();
        let mut col = div()
            .flex()
            .flex_col()
            .gap_px()
            // Line 1: count + A/M/D summary
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().text_main))
                            .child(SharedString::from(count_line)),
                    )
                    .when(!summary.is_empty(), |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme().text_muted))
                                .child(SharedString::from(summary)),
                        )
                    }),
            );
        // Line 2: target branch
        col = col.child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .overflow_hidden()
                .truncate()
                .child(SharedString::from(format!("→ {}", pv.target_branch))),
        );
        // Line 3: author
        col = col.child(
            div()
                .text_xs()
                .text_color(rgb(theme().text_muted))
                .overflow_hidden()
                .truncate()
                .child(SharedString::from(format!("by {}", pv.author))),
        );
        col.into_any_element()
    } else {
        div().into_any_element()
    };

    // ── Assemble panel ───────────────────────────────────────
    // T-UI-003: diff ボックス廃止。Unstaged/Staged 箱が flex_1 で全体を占める(1:1)。
    div()
        // `panel_width` is the unscaled, persisted right-panel width; scale at
        // render so it tracks zoom (the Panel divider drag uses the same space).
        .w(theme::scaled_px(panel_width))
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
                // Unstaged スクロールボックス — PERF: virtualized uniform_list.
                .child(
                    div()
                        .id("cp-unstaged-scroll")
                        .flex_1()
                        .min_h(px(0.))
                        .mx_1()
                        .mb_px()
                        .border_1()
                        .border_color(rgb(theme().surface))
                        .rounded_sm()
                        .flex()
                        .flex_col()
                        .child({
                            let handle = unstaged_scroll_handle.clone();
                            with_vertical_scrollbar(
                                "cp-unstaged-list-scroll",
                                &handle,
                                uniform_list(
                                    "cp-unstaged-list",
                                    unstaged_row_count,
                                    cx.processor(
                                        move |this, range: std::ops::Range<usize>, _window, cx| {
                                            let tree = this
                                                .commit_panel
                                                .as_ref()
                                                .map(|p| p.tree_view)
                                                .unwrap_or(false);
                                            range
                                                .filter_map(|i| {
                                                    if tree {
                                                        render_unstaged_tree_row(this, i, cx)
                                                    } else {
                                                        render_unstaged_flat_row(this, i, cx)
                                                    }
                                                })
                                                .collect::<Vec<_>>()
                                        },
                                    ),
                                )
                                .track_scroll(unstaged_scroll_handle)
                                .flex_1()
                                .min_h(px(0.)),
                                false,
                            )
                        }),
                )
                // Staged ヘッダ (固定)
                .child(staged_header)
                // Staged スクロールボックス — PERF: virtualized uniform_list.
                .child(
                    div()
                        .id("cp-staged-scroll")
                        .flex_1()
                        .min_h(px(0.))
                        .mx_1()
                        .mb_px()
                        .border_1()
                        .border_color(rgb(theme().surface))
                        .rounded_sm()
                        .flex()
                        .flex_col()
                        .child({
                            let handle = staged_scroll_handle.clone();
                            with_vertical_scrollbar(
                                "cp-staged-list-scroll",
                                &handle,
                                uniform_list(
                                    "cp-staged-list",
                                    staged_row_count,
                                    cx.processor(
                                        move |this, range: std::ops::Range<usize>, _window, cx| {
                                            let tree = this
                                                .commit_panel
                                                .as_ref()
                                                .map(|p| p.tree_view)
                                                .unwrap_or(false);
                                            range
                                                .filter_map(|i| {
                                                    if tree {
                                                        render_staged_tree_row(this, i, cx)
                                                    } else {
                                                        render_staged_flat_row(this, i, cx)
                                                    }
                                                })
                                                .collect::<Vec<_>>()
                                        },
                                    ),
                                )
                                .track_scroll(staged_scroll_handle)
                                .flex_1()
                                .min_h(px(0.)),
                                false,
                            )
                        }),
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
                // T-COMMIT-001: staged preview (count / A·M·D / branch / author)
                .child(preview_block)
                // Message label + plain⇄template toggle
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(div().text_xs().text_color(rgb(theme().text_label)).child(
                            SharedString::from(if template_mode {
                                "Commit message (template)"
                            } else {
                                "Commit message"
                            }),
                        ))
                        .child(mode_toggle),
                )
                .child(msg_input_wrapper)
                // Smart Commit Message toolbar (Suggest / Generate / toggles)
                .child(smart_toolbar)
                // Unstaged warning
                .when(has_unstaged_warning, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().color_warning))
                            .child(SharedString::from(i18n::unstaged_not_included(
                                unstaged_count,
                            ))),
                    )
                })
                // Commit button
                .child(commit_btn)
                // T-COMMIT-011: Amend the previous commit (unpushed only —
                // the plan blocks pushed/merge/etc.). Mode follows what the
                // user has provided: staged changes, a new message, or both.
                .child({
                    let amend_click = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
                        let staged = this
                            .commit_panel
                            .as_ref()
                            .map(|p| !p.staged.is_empty())
                            .unwrap_or(false);
                        let msg = this
                            .commit_input
                            .as_ref()
                            .map(|i| !i.read(cx).value().trim().is_empty())
                            .unwrap_or(false);
                        let mode = match (msg, staged) {
                            (true, true) => AmendMode::Both,
                            (false, true) => AmendMode::Staged,
                            (true, false) => AmendMode::MessageOnly,
                            (false, false) => {
                                this.status_footer = FooterStatus::Idle(SharedString::from(
                                    Msg::AmendNeedMessageOrStaged.t(),
                                ));
                                cx.notify();
                                return;
                            }
                        };
                        this.open_amend_modal(mode, cx);
                        cx.notify();
                    });
                    div()
                        .id("cp-amend-btn")
                        .mt_1()
                        .w_full()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(rgb(theme().surface))
                        .text_sm()
                        .text_color(rgb(theme().color_warning))
                        .on_click(amend_click)
                        .hover(|st| st.bg(rgb(theme().selected)))
                        .child(SharedString::from("Amend last commit…"))
                }),
        )
}

// ──────────────────────────────────────────────────────────────
// Application entry point helper
// ──────────────────────────────────────────────────────────────

/// Open the GPUI window and start the event loop.
pub fn run_app(app_state: KagiApp) {
    use gpui::Application;

    // W4-TABS / ADR-0027: the watcher is armed from inside the window context
    // via `arm_watcher` (generation scheme), replacing the fixed spawn that
    // used to live here.  No pre-window watcher is created.

    let application = Application::new().with_assets(assets::KagiAssets);

    // macOS Dock-reopen: clicking the Dock icon after the last window was
    // closed (✕) must bring a window back — the process stays alive, so
    // without this handler the app looks dead while still running.  The
    // previous session's tabs are restored from settings.json.
    application.on_reopen(|cx: &mut App| {
        if cx.windows().is_empty() {
            let mut fresh = KagiApp::with_error("");
            tabs::restore_saved_session(&mut fresh);
            fresh.log_tabs();
            open_main_window(fresh, cx);
        }
        cx.activate(true);
    });

    application.run(move |cx: &mut App| {
        // Bundle fonts so the UI + monospace look identical on every OS. Linux
        // has no "Menlo"/SF and the platform default is inconsistent, which made
        // fonts render broken on Ubuntu (user-reported). OFL: Inter (UI) +
        // JetBrains Mono (terminal / conflict editor / code). The family names
        // here MUST match the fonts' name tables (UI_FONT / MONO_FONT).
        if let Err(e) = cx.text_system().add_fonts(vec![
            std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/Inter-Regular.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/Inter-Bold.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!(
                "../../assets/fonts/JetBrainsMono-Regular.ttf"
            )),
            std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf")),
        ]) {
            eprintln!("[kagi] fonts: add_fonts failed (UI may fall back): {e}");
        } else {
            eprintln!("[kagi] fonts: loaded Inter + JetBrains Mono");
        }

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
        // Scoped `!Terminal` so Escape reaches a focused terminal (vim/less/etc.).
        cx.bind_keys([KeyBinding::new("escape", CloseMainDiff, Some("!Terminal"))]);
        // Arrow keys step through files while the main diff is open
        // (no-ops otherwise; see main_diff_step). Scoped `!Terminal` so up/down
        // reach a focused terminal (shell history) instead of being consumed here.
        cx.bind_keys([
            KeyBinding::new("up", DiffPrevFile, Some("!Terminal")),
            KeyBinding::new("down", DiffNextFile, Some("!Terminal")),
        ]);
        // ADR-0084: app-level Undo/Redo. Scoped `!Input && !Terminal` so a
        // focused text field (gpui-component Input, key_context "Input") keeps
        // OS-standard text undo (OsAction::Undo) and the terminal keeps its own
        // Cmd+Z — the app history move only fires elsewhere (e.g. commit graph).
        // gpui 0.2.2 only accepts `&&`/`||` (single `&` fails to parse).
        cx.bind_keys([
            KeyBinding::new("cmd-z", commands::HistoryUndo, Some("!Input && !Terminal")),
            KeyBinding::new(
                "cmd-shift-z",
                commands::HistoryRedo,
                Some("!Input && !Terminal"),
            ),
        ]);
        // Ctrl+A = Select All in text inputs. gpui-component binds ctrl-a to
        // *both* SelectAll and MoveHome (emacs-style) in the "Input" context,
        // and the later (MoveHome) wins — so on this platform Ctrl+A jumped to
        // line start instead of selecting all. Re-bind it to SelectAll here
        // (registered after gpui_component::init, so it takes precedence).
        // cmd-a (SelectAll) and double-click word-select already work natively.
        cx.bind_keys([KeyBinding::new(
            "ctrl-a",
            gpui_component::input::SelectAll,
            Some("Input"),
        )]);

        // NOTE: a KeyBinding::new("enter", …) here never dispatched (the
        // Return key's key_char "\n" path); Enter is handled as a raw key
        // on the root element instead — see render().

        // W5-MENU / ADR-0029: register the command-registry keystrokes and the
        // native menu bar.  Keystrokes are passed into `set_menus` via the live
        // keymap, so they render next to each menu item automatically.
        commands::register_keybindings(cx);
        cx.set_menus(commands::build_menus());

        open_main_window(app_state, cx);
        cx.activate(true);
    });
}

/// Open (or re-open) the main kagi window hosting `app_state`.
///
/// Factored out of [`run_app`] so the Dock-reopen handler can recreate the
/// window after the user closed it (the one-time init — gpui_component,
/// keybindings, menus — stays in `run_app`).
fn open_main_window(mut app_state: KagiApp, cx: &mut App) {
    use gpui::{size, Bounds, WindowBounds, WindowOptions};

    // KAGI_WINDOW=WxH (dev/testing only): override the initial window size
    // verbatim so layout behaviour at specific sizes can be verified headlessly.
    let (win_w, win_h) = if let Some((w, h)) = std::env::var("KAGI_WINDOW").ok().and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
    }) {
        (w, h)
    } else {
        // Preferred initial size, but clamped to the active display so the window
        // never opens off-screen on small / scaled displays (user-reported). The
        // ideal size is kept on big screens; only the upper bound is a fraction
        // of the display (so 4K/ultrawide don't get a needlessly huge window).
        const PREF_W: f32 = 1440.0;
        const PREF_H: f32 = 920.0;
        const MIN_W: f32 = 900.0;
        const MIN_H: f32 = 600.0;
        match cx.primary_display() {
            Some(display) => {
                let ds = display.bounds().size;
                let max_w = f32::from(ds.width) * 0.92;
                let max_h = f32::from(ds.height) * 0.90;
                // clamp(low, high) with low never above high (tiny displays fill).
                (
                    PREF_W.clamp(MIN_W.min(max_w), max_w),
                    PREF_H.clamp(MIN_H.min(max_h), max_h),
                )
            }
            None => (PREF_W, PREF_H),
        }
    };
    let bounds = Bounds::centered(None, size(px(win_w), px(win_h)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: main_window_titlebar(),
            window_decorations: main_window_decorations(),
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

            // The bottom panel now opens on the Terminal tab by default
            // (user request) — start the shell as soon as a Window
            // context exists. ensure_terminal is a no-op without a repo
            // (welcome screen) and KAGI_TERMINAL=1 stays as an explicit
            // headless trigger.
            if std::env::var("KAGI_TERMINAL").as_deref() == Ok("1")
                || kagi.read(cx).bottom_panel_open
            {
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
}

#[cfg(target_os = "macos")]
fn main_window_titlebar() -> Option<gpui::TitlebarOptions> {
    // Themed title bar: make the native bar transparent so kagi's own top
    // content fills the title-bar area. macOS still draws the traffic lights,
    // positioned over our content; the tab strip reserves space for them.
    Some(gpui::TitlebarOptions {
        title: None,
        appears_transparent: true,
        traffic_light_position: Some(gpui::point(px(9.0), px(9.0))),
    })
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn main_window_titlebar() -> Option<gpui::TitlebarOptions> {
    Some(gpui_component::TitleBar::title_bar_options())
}

#[cfg(not(target_os = "macos"))]
#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn main_window_titlebar() -> Option<gpui::TitlebarOptions> {
    Some(gpui::TitlebarOptions {
        title: Some("Kagi".into()),
        appears_transparent: false,
        traffic_light_position: None,
    })
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn main_window_decorations() -> Option<gpui::WindowDecorations> {
    Some(gpui::WindowDecorations::Client)
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn main_window_decorations() -> Option<gpui::WindowDecorations> {
    None
}

impl KagiApp {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn render_platform_titlebar(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let mut menu_row = div().flex().items_center().h_full().gap_1().pr_3();

        // ADR-0085: drive the heads from the canonical MENU_BAR, skipping
        // `mac_only` sections (the Edit menu — see ADR-0085 §4).  The dropdown
        // uses the same filtered iterator, so `ix` lines up between the head and
        // its panel (and with the left-offset maths below).
        for (ix, section) in commands::linux_menu_sections().enumerate() {
            let open = self.platform_menu_open == Some(ix);
            let click = cx.listener(move |this, _: &gpui::ClickEvent, _window, cx| {
                this.platform_menu_open = if this.platform_menu_open == Some(ix) {
                    None
                } else {
                    Some(ix)
                };
                cx.stop_propagation();
                cx.notify();
            });
            let suppress_drag = |_: &gpui::MouseDownEvent, window: &mut Window, cx: &mut App| {
                window.prevent_default();
                cx.stop_propagation();
            };

            menu_row = menu_row.child(
                div()
                    .id(("platform-menu-head", ix))
                    .h_full()
                    .flex()
                    .items_center()
                    .px_2()
                    .text_sm()
                    .rounded_sm()
                    .text_color(rgb(theme().text_main))
                    .bg(if open {
                        rgb(theme().selected)
                    } else {
                        rgb(theme().panel)
                    })
                    .hover(|s| s.bg(rgb(theme().selected)))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, suppress_drag)
                    .on_click(click)
                    .child(SharedString::from(section.label)),
            );
        }

        Some(
            gpui_component::TitleBar::new()
                .child(menu_row)
                .into_any_element(),
        )
    }

    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    fn render_platform_titlebar(&mut self, _cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        None
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn render_platform_menu_dropdown(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let ix = self.platform_menu_open?;
        // ADR-0085: index into the *filtered* sections (same iterator the heads
        // use), so the open panel matches the head it was launched from and the
        // left offset (computed from `ix`) lines up.
        let section = commands::linux_menu_sections().nth(ix)?;
        let dismiss = cx.listener(|this, _: &gpui::MouseDownEvent, _window, cx| {
            this.platform_menu_open = None;
            cx.stop_propagation();
            cx.notify();
        });

        let mut panel = div()
            // Block mouse events from reaching the dismiss backdrop below —
            // without this, pressing a menu item fires the backdrop's
            // on_mouse_down first, the menu unmounts, and the item's on_click
            // (down+up on the same element) never completes. Same fix as the
            // commit context menu (see context_menu.rs).
            .occlude()
            .absolute()
            .top_1()
            .left(theme::scaled_px(8.0 + ix as f32 * 78.0))
            .w(theme::scaled_px(260.0))
            .py_1()
            .rounded(theme::scaled_px(6.0))
            .border_1()
            .border_color(rgb(theme().selected))
            .bg(rgb(theme().panel))
            .shadow_lg();

        // ADR-0085: one clickable command row, reused for plain `Command` nodes
        // and for the inline-expanded Theme/Language submenu rows.  `row_ix` is
        // only used to build a stable element id.
        let command_row = |this: &Self,
                           cx: &mut Context<Self>,
                           row_ix: usize,
                           id: &'static str|
         -> gpui::AnyElement {
            let command = commands::command(id);
            let state = commands::command_state(this, id);
            let enabled = matches!(state, commands::CommandState::Enabled);
            // `platform_menu_label` adds the "✓ " active marker for the current
            // theme / language (no-op for ordinary commands).
            let label = platform_menu_label(id, command.map(|c| c.label).unwrap_or(id));
            let key = command.and_then(|c| c.keystroke).unwrap_or("");
            let invoke = cx.listener(move |this, _: &gpui::ClickEvent, window, cx| {
                if commands::is_enabled(this, id) {
                    this.platform_menu_open = None;
                    this.handle_menu_command(id, window, cx);
                    cx.notify();
                }
                cx.stop_propagation();
            });
            let disabled_reason = match state {
                commands::CommandState::Disabled(reason) => Some(reason),
                _ => None,
            };

            div()
                .id(SharedString::from(format!(
                    "platform-menu-item-{ix}-{row_ix}"
                )))
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_3()
                .py(theme::scaled_px(5.0))
                .text_sm()
                .text_color(if enabled {
                    rgb(theme().text_main)
                } else {
                    rgb(theme().text_muted)
                })
                .when(enabled, |s| {
                    s.cursor_pointer()
                        .hover(|s| s.bg(rgb(theme().selected)))
                        .on_click(invoke)
                })
                .when_some(disabled_reason, |s, reason| {
                    s.tooltip(move |window, cx| Tooltip::new(reason.to_string()).build(window, cx))
                })
                .child(div().flex_1().truncate().child(SharedString::from(label)))
                .when(!key.is_empty(), |s| {
                    s.child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme().text_muted))
                            .child(SharedString::from(key)),
                    )
                })
                .into_any_element()
        };

        // `row_ix` is a running counter (submenus expand to several rows, so it
        // diverges from `item_ix`) — it only needs to be unique within a panel.
        let mut row_ix = 0usize;
        for (item_ix, node) in section.items.iter().enumerate() {
            match node {
                commands::MenuNode::Separator => {
                    panel = panel.child(
                        div()
                            .id(SharedString::from(format!(
                                "platform-menu-separator-{ix}-{item_ix}"
                            )))
                            .my_1()
                            .h(px(1.0))
                            .bg(rgb(theme().surface)),
                    );
                }
                commands::MenuNode::Command(id) => {
                    panel = panel.child(command_row(self, cx, row_ix, id));
                    row_ix += 1;
                }
                // ADR-0085 §3: the dropdown has no nested-panel support, so the
                // dynamic submenus expand inline as command rows (the "✓ " marker
                // is applied by `platform_menu_label`) — preserving the previous
                // View-menu behaviour on Linux.
                commands::MenuNode::Submenu(commands::DynSubmenu::Theme) => {
                    for id in commands::THEME_COMMAND_IDS {
                        panel = panel.child(command_row(self, cx, row_ix, id));
                        row_ix += 1;
                    }
                }
                commands::MenuNode::Submenu(commands::DynSubmenu::Language) => {
                    for id in commands::LANG_COMMAND_IDS {
                        panel = panel.child(command_row(self, cx, row_ix, id));
                        row_ix += 1;
                    }
                }
                // OsEdit only ever appears in `mac_only` sections, which are
                // filtered out before we get here — but match exhaustively.
                commands::MenuNode::OsEdit(_) => {}
            }
        }

        Some(
            div()
                .absolute()
                .top(gpui_component::TITLE_BAR_HEIGHT)
                .left_0()
                .right_0()
                .bottom_0()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .on_mouse_down(MouseButton::Left, dismiss),
                )
                .child(panel)
                .into_any_element(),
        )
    }

    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    fn render_platform_menu_dropdown(&self, _cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        None
    }

    // `cx` is only used by the Linux/FreeBSD branch (titlebar + menu dropdown);
    // the other-target branch just returns `content`.
    #[cfg_attr(
        not(any(target_os = "linux", target_os = "freebsd")),
        allow(unused_variables)
    )]
    fn platform_window_shell(
        &mut self,
        content: gpui::AnyElement,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        {
            div()
                .flex()
                .flex_col()
                .size_full()
                .bg(rgb(theme().bg_base))
                .children(self.render_platform_titlebar(cx))
                .child(div().flex_1().min_h(px(0.0)).child(content))
                .children(self.render_platform_menu_dropdown(cx))
                .into_any()
        }

        #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
        {
            content
        }
    }
}

// Only the Linux/FreeBSD in-app menu calls this (✓ marker for theme/lang).
#[cfg_attr(not(any(target_os = "linux", target_os = "freebsd")), allow(dead_code))]
fn platform_menu_label(id: &str, fallback: &str) -> String {
    if let Some(slug) = commands::theme_slug_for_command(id) {
        if theme::theme().slug == slug {
            return format!("\u{2713} {fallback}");
        }
    }
    if let Some(lang) = commands::lang_for_command(id) {
        if i18n::lang() == lang {
            return format!("\u{2713} {fallback}");
        }
    }
    fallback.to_string()
}

// ────────────────────────────────────────────────────────────
// W32-CONFLICT-EDITOR: small helpers for the Save oplog record
// ────────────────────────────────────────────────────────────

/// A short stable content hash for the oplog before/after fields.  Reuses the
/// crate's self-contained FNV-1a (no new deps); 16 lowercase hex chars.  This is
/// a log fingerprint only (no security properties).  `chars()`-safe: hashes
/// bytes of a `&str`, never byte-slices it.
fn short_hash(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    format!("{:016x}", h)
}

/// T-CONFLICT-UI-001: cheap FNV-1a content signature for the Conflict Editor's
/// three panes, so the editors only re-`set_value` when something actually
/// changes (avoids clobbering an in-progress manual edit every frame).
fn conflict_content_sig(path: &std::path::Path, result: &str, edit_mode: bool) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |bytes: &[u8]| {
        for byte in bytes {
            h ^= *byte as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        // separator so concatenation is unambiguous.
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    };
    mix(path.to_string_lossy().as_bytes());
    mix(result.as_bytes());
    mix(if edit_mode { b"edit" } else { b"preview" });
    h
}

fn conflict_split_ratio_from_cursor(
    cursor: f32,
    start: f32,
    end: f32,
    divider_size: f32,
    min: f32,
    max: f32,
) -> Option<f32> {
    let span = end - start - divider_size;
    if span <= 1.0 {
        return None;
    }
    Some(((cursor - start - divider_size / 2.0) / span).clamp(min, max))
}

/// Stable slug for a per-hunk choice, for the oplog action summary (T-035).
fn hunk_choice_slug(choice: &kagi::git::resolution::HunkChoice) -> &'static str {
    use kagi::git::resolution::HunkChoice::*;
    match choice {
        AcceptCurrent => "current",
        AcceptIncoming => "incoming",
        BothCurrentFirst => "both-cf",
        BothIncomingFirst => "both-if",
        Manual(_) => "manual",
        Unresolved => "unresolved",
    }
}

#[cfg(test)]
mod conflict_editor_geometry_tests {
    use super::conflict_split_ratio_from_cursor;

    #[test]
    fn split_ratio_uses_divider_center() {
        let ratio =
            conflict_split_ratio_from_cursor(302.0, 100.0, 504.0, 4.0, 0.2, 0.8).expect("ratio");
        assert!((ratio - 0.5).abs() < 0.0001);
    }

    #[test]
    fn split_ratio_clamps_and_rejects_bad_span() {
        assert_eq!(
            conflict_split_ratio_from_cursor(0.0, 100.0, 504.0, 4.0, 0.2, 0.8),
            Some(0.2)
        );
        assert_eq!(
            conflict_split_ratio_from_cursor(900.0, 100.0, 504.0, 4.0, 0.2, 0.8),
            Some(0.8)
        );
        assert_eq!(
            conflict_split_ratio_from_cursor(10.0, 0.0, 4.0, 4.0, 0.2, 0.8),
            None
        );
    }
}

// ── T-DNDMERGE-001 / ADR-0079: drag-merge action validation ────
#[cfg(test)]
mod drag_merge_validation_tests {
    use super::validate_merge_from_drag;

    fn branches() -> Vec<(String, bool)> {
        vec![
            ("main".to_string(), true), // current (HEAD)
            ("feature".to_string(), false),
            ("topic/x".to_string(), false),
        ]
    }

    fn remotes() -> Vec<String> {
        vec!["origin/main".to_string(), "origin/feature".to_string()]
    }

    #[test]
    fn drag_merge_accepts_other_local_branch() {
        assert_eq!(
            validate_merge_from_drag("feature", &branches(), &remotes(), false),
            Ok(())
        );
        assert_eq!(
            validate_merge_from_drag("topic/x", &branches(), &remotes(), false),
            Ok(())
        );
    }

    #[test]
    fn drag_merge_accepts_remote_only_branch() {
        // An upstream-only branch (a remote ref with no local counterpart) is a
        // valid merge source — merged directly via its remote-tracking ref.
        assert_eq!(
            validate_merge_from_drag("origin/feature", &branches(), &remotes(), false),
            Ok(())
        );
    }

    #[test]
    fn drag_merge_rejects_current_branch_onto_itself() {
        let err = validate_merge_from_drag("main", &branches(), &remotes(), false)
            .expect_err("dropping current branch onto itself must be rejected");
        assert!(
            err.contains("main"),
            "reason should name the branch: {}",
            err
        );
        assert!(
            err.contains("current branch"),
            "reason should explain same-branch rejection: {}",
            err
        );
    }

    #[test]
    fn drag_merge_rejects_unknown_branch() {
        let err = validate_merge_from_drag("ghost", &branches(), &remotes(), false)
            .expect_err("a non-existent branch must be rejected");
        assert!(
            err.contains("not a branch"),
            "reason should explain unknown branch: {}",
            err
        );
    }

    #[test]
    fn drag_merge_rejects_when_busy() {
        let err = validate_merge_from_drag("feature", &branches(), &remotes(), true)
            .expect_err("a drag while another op is busy must be rejected");
        assert!(!err.is_empty(), "busy rejection should carry a reason");
    }
}

// ── T-DNDMERGE-001: graph ref-badge → drag payload name extraction ──
#[cfg(test)]
mod draggable_branch_name_tests {
    use super::draggable_branch_name;
    use crate::ui::commit_list::{BadgeKind, RefBadge};

    fn badge(kind: BadgeKind, label: &str) -> RefBadge {
        RefBadge {
            kind,
            label: label.to_string().into(),
        }
    }

    #[test]
    fn branch_badge_yields_its_plain_name() {
        assert_eq!(
            draggable_branch_name(&badge(BadgeKind::Branch, "feature")),
            Some("feature".to_string())
        );
        // A commit with several branches: each Branch chip carries its own name.
        assert_eq!(
            draggable_branch_name(&badge(BadgeKind::Branch, "topic/x")),
            Some("topic/x".to_string())
        );
    }

    #[test]
    fn remote_badge_yields_its_full_ref() {
        // A remote-tracking chip is a draggable merge source: its label is the
        // full `remote/name` ref, resolved directly by the merge backend.
        assert_eq!(
            draggable_branch_name(&badge(BadgeKind::Remote, "origin/feature")),
            Some("origin/feature".to_string())
        );
    }

    #[test]
    fn head_and_tag_badges_are_not_draggable() {
        // HeadBranch is the drop *target* (and its label carries the "✓"
        // indicator), never a drag source. Tags are not merge sources here.
        assert_eq!(
            draggable_branch_name(&badge(BadgeKind::HeadBranch, "main ✓")),
            None
        );
        assert_eq!(
            draggable_branch_name(&badge(BadgeKind::Tag, "v0.1.0")),
            None
        );
    }
}
