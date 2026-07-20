//! UI module — T008: GPUI commit list / T009: commit graph lane / T010: commit selection + detail panel / T011: changed files list / T012: file diff viewer / T013: checkout plan modal + sidebar / T023: pane resize / T-BP-002: bottom panel open/close + resize / T-BP-007: terminal
//!
//! This module lives in the binary crate (`main.rs` does `mod ui;`).
//! It must not be added to `src/lib.rs` so that domain tests stay
//! independent of GPUI.

pub mod activity_view;
pub mod assets;
pub mod avatar;
pub mod avatar_fetch;
mod avatar_lookup;
mod avatar_resolve;
pub mod badges;
pub mod blocking_ops;
pub mod branch_cleanup;
pub mod branch_menu;
pub mod button_style;
pub mod commands;
pub mod commit_list;
pub mod commit_panel;
mod commit_panel_render;
pub mod compare_pane;
pub mod conflict_editor;
pub mod conflict_view;
pub mod context_menu;
pub mod detail_panel;
mod diff_cache;
mod diff_split;
pub mod diff_view;
pub mod diffstat_bar;
pub mod ecosystem;
pub mod editor_fs_ops;
pub mod editor_markdown;
pub mod editor_tree_menu;
pub mod editor_workspace;
pub mod file_history;
mod file_menu;
pub mod file_tree;
mod graph_solo;
pub mod graph_view;
pub mod i18n;
pub mod inspector;
pub mod main_diff_pane;
pub mod menu_overlay;
mod modal_renderers;
mod modal_renderers_commit;
mod modal_renderers_create;
mod modal_renderers_destructive;
mod modal_renderers_editor_fs;
mod modal_renderers_misc;
mod modal_renderers_plan;
mod modal_renderers_stash;
pub mod modals;
mod operations;
pub mod oplog_panel;
mod platform_menu;
mod reload;
pub mod remote_browse;
mod render;
mod render_body;
mod render_bottom;
mod render_divider;
mod render_header;
mod render_helpers;
mod render_overlay;
mod render_status;
mod render_wip;
pub mod settings;
pub mod settings_view;
pub mod sidebar;
pub mod smart_commit;
pub mod stash_menu;
mod tab_view;
pub mod tabs;
pub mod terminal;
pub mod theme;
pub mod toast_stack;
pub mod types;
pub mod view_models;
pub mod watcher;
pub mod workspace;
pub mod worktree_menu;

pub use compare_pane::ComparePane;
pub use diff_view::*;
use i18n::Msg;
pub use main_diff_pane::MainDiffPane;
pub use modals::*;
pub use remote_browse::*;
pub(crate) use render_helpers::with_vertical_scrollbar;
pub use tab_view::{build_tab_view, TabViewState};
use theme::theme;
pub use types::*;

use kagi_git::message_gen;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
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
        CheckoutSelected,
        // T-WS-EDITOR-002: Cmd-S saves the Editor Workspace's dirty buffer.
        SaveEditorFile,
        // T-TERM-INTERACT-001 follow-up: Tab / Shift-Tab for the embedded
        // terminal. gpui_component::Root binds "tab"/"shift-tab" to focus
        // cycling in its "Root" context (root.rs), which is an ancestor of
        // everything — so the raw key never reached the terminal's
        // on_key_down and shell completion was dead (user report). These
        // actions are bound in the deeper "Terminal" context (deeper context
        // wins in gpui's keymap), and their handlers write the terminal
        // bytes straight to the PTY.
        TerminalSendTab,
        TerminalSendShiftTab
    ]
);

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
        // The label may carry the worktree marker ("🌲 name"); the drag
        // payload must be the plain ref name or the merge validator rejects
        // it as "not a branch" (user report: badges of worktree-checked-out
        // branches could not be drag-merged, with no visible feedback).
        BadgeKind::Branch | BadgeKind::Remote => {
            Some(badge.label.trim_start_matches("🌲 ").to_string())
        }
        BadgeKind::HeadBranch | BadgeKind::Tag => None,
    }
}

/// Extract a branch ref name from a graph badge for context-menu actions.
fn context_branch_name(badge: &commit_list::RefBadge) -> Option<String> {
    match badge.kind {
        BadgeKind::HeadBranch => Some(badge.label.trim_end_matches(" ✓").trim_end().to_string()),
        // Strip the worktree marker (see draggable_branch_name).
        BadgeKind::Branch | BadgeKind::Remote => {
            Some(badge.label.trim_start_matches("🌲 ").to_string())
        }
        BadgeKind::Tag => None,
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

// UI_FONT / MONO_FONT moved to kagi-ui-core::theme (ADR-0121 C1); re-exported
// here so `super::UI_FONT` call sites keep working.
pub use kagi_ui_core::theme::{MONO_FONT, UI_FONT};

/// Live window size in whole px (w, h), updated each frame while the window
/// is plain `Windowed` (maximized/fullscreen sizes are not remembered), and
/// persisted as the `window_size` setting on quit. 0 = not seen yet.
pub(crate) static LAST_WIN_W: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub(crate) static LAST_WIN_H: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

// Sidebar / panel width limits.
const SIDEBAR_MIN: f32 = 120.0;
const SIDEBAR_MAX: f32 = 400.0;
const PANEL_MIN: f32 = 240.0;
const PANEL_MAX: f32 = 800.0;

// Default widths (matching the pre-T023 hard-coded values).
const PANEL_DEFAULT: f32 = 360.0;

// T-WS-EDITOR-004: Editor Workspace tree / hunks pane drag-resize limits.
const EDITOR_TREE_MIN: f32 = 160.0;
const EDITOR_TREE_MAX: f32 = 480.0;
const EDITOR_HUNKS_MIN: f32 = 240.0;
const EDITOR_HUNKS_MAX: f32 = 700.0;

// T-BP-004: Operation Log initial load count from disk.
// (Ring-buffer cap moved to oplog_panel::OP_ENTRIES_MAX, ADR-0111.)
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

/// Default number of commits loaded into the graph on first load / tab switch.
/// The graph can grow past this via the bottom-of-list "load more" affordance
/// (see [`KagiApp::load_more_commits`]).
pub const DEFAULT_COMMIT_LIMIT: usize = 10_000;
/// How many additional commits each "load more" click pulls in.
pub const COMMIT_PAGE_STEP: usize = 1_000;

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
pub(crate) const CONFLICT_AB_DEFAULT: f32 = 0.5;
const CONFLICT_AB_MIN: f32 = 0.2;
const CONFLICT_AB_MAX: f32 = 0.8;
/// A·B / Result vertical split ratio (fraction of the editor height given to
/// the A·B row; the remainder is the Result pane).
pub(crate) const CONFLICT_RESULT_DEFAULT: f32 = 0.55;
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
use commit_panel::{
    status_badge, CommitPanelFileRef, CommitPanelState, CommitPanelView, CommitPlanModal,
};
use context_menu::{CommitAction, CommitMenuState, MenuContext};
use detail_panel::CommitDetail;
use graph_view::graph_canvas;
use kagi_git::{
    oplog::{append_oplog, read_oplog_tail, OpLogEntry, OpOutcome},
    ops::{
        default_tracking_branch_name, validate_branch_rename, AmendMode, OperationPlan,
        StateSummary,
    },
    CommitId, FileDiffStat, FileStatus, Head, RepoSnapshot,
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
    /// Number of untracked files. Counted toward the WIP-row change total so the
    /// row's "N changes" matches `is_dirty` (which includes untracked).
    pub untracked: usize,
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
    /// Wall-clock time (Unix seconds) of the last successful `git fetch`
    /// (FETCH_HEAD mtime at snapshot time; updated in place after an in-app
    /// fetch). Drives the status-bar fetch-age indicator (ADR-0127). `None`
    /// when the repo has never fetched.
    pub last_fetch_secs: Option<i64>,
}

impl StatusBarSummary {
    /// Total pending changes for the WIP row's "N changes" label. Counts every
    /// dirty kind (staged + unstaged + untracked + conflicted) so the number
    /// matches `is_dirty` — an untracked-only or conflict-only tree must not
    /// render the WIP row as "0 changes".
    pub fn wip_change_count(&self) -> usize {
        self.staged + self.unstaged + self.untracked + self.conflict_count
    }

    /// Build from a [`RepoSnapshot`] at the current wall clock time.
    pub fn from_snapshot(snap: &kagi_git::RepoSnapshot) -> Self {
        use commit_list::now_unix_secs;
        use kagi_git::Head;

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
            untracked: snap.status.untracked.len(),
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
            last_fetch_secs: snap.last_fetch_secs,
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
            untracked: 0,
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
            last_fetch_secs: None,
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
            untracked: 0,
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
            last_fetch_secs: None,
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

    /// WIP-row change count includes untracked and conflicted, so an
    /// untracked-only (or conflict-only) tree never reads "0 changes" while the
    /// row is shown (regression: row gated on `is_dirty`, count on staged+unstaged).
    #[test]
    fn wip_change_count_covers_all_dirty_kinds() {
        let untracked_only = StatusBarSummary {
            untracked: 3,
            ..Default::default()
        };
        assert_eq!(untracked_only.wip_change_count(), 3);

        let conflict_only = StatusBarSummary {
            conflict_count: 2,
            ..Default::default()
        };
        assert_eq!(conflict_only.wip_change_count(), 2);

        let mixed = StatusBarSummary {
            staged: 1,
            unstaged: 2,
            untracked: 3,
            conflict_count: 4,
            ..Default::default()
        };
        assert_eq!(mixed.wip_change_count(), 10);

        assert_eq!(StatusBarSummary::default().wip_change_count(), 0);
    }
}

/// Format a Unix-epoch timestamp as `"HH:MM:SS"` in the machine's **local**
/// time zone (footer last-refresh clock, oplog overlay timestamps).
///
/// Was UTC-only (`epoch % 86400`), which read 9 h off in JST etc. The local
/// UTC offset is resolved per-instant via chrono (iana-time-zone under the
/// hood — DST-correct and free of the `localtime_r` data race), then the pure
/// civil arithmetic in [`hms_from_epoch`] renders the shifted seconds.
pub fn format_hms(epoch_secs: i64) -> String {
    hms_from_epoch(epoch_secs + local_utc_offset_secs(epoch_secs))
}

/// Local UTC offset (seconds) at `epoch_secs`; `0` if it can't be resolved.
fn local_utc_offset_secs(epoch_secs: i64) -> i64 {
    use chrono::{Offset, TimeZone};
    chrono::Local
        .timestamp_opt(epoch_secs, 0)
        .single()
        .map(|dt| dt.offset().fix().local_minus_utc() as i64)
        .unwrap_or(0)
}

/// Pure `"HH:MM:SS"` from a (possibly offset-adjusted) epoch second count.
/// Machine-independent — unit-tested; the tz shift lives in [`format_hms`].
fn hms_from_epoch(epoch_secs: i64) -> String {
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

#[cfg(test)]
mod hms_tests {
    use super::hms_from_epoch;

    #[test]
    fn formats_time_of_day() {
        assert_eq!(hms_from_epoch(0), "00:00:00");
        assert_eq!(hms_from_epoch(1_705_311_000), "09:30:00");
        assert_eq!(hms_from_epoch(86_399), "23:59:59");
        // wraps across the day boundary (offset can push epoch past a day)
        assert_eq!(hms_from_epoch(86_400 + 3_661), "01:01:01");
    }

    #[test]
    fn negative_epoch_floors() {
        assert_eq!(hms_from_epoch(-60), "23:59:00");
    }
}

// ──────────────────────────────────────────────────────────────
// W3-NOTIFY: toast (snackbar) notifications — timing constants
// (the FooterStatus / ToastKind / Toast types live in `types.rs`)
// ──────────────────────────────────────────────────────────────

/// Snackbar slide animation timings / distance.
const TOAST_ENTER_MS: u64 = 240;
const TOAST_EXIT_MS: u64 = 220;
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
    blockers: &[kagi_git::ops::PlanNote],
    keyed: impl Iterator<Item = (String, String)>,
) -> Vec<SharedString> {
    // ADR-0129 Phase 1: notes are matched via their English rendering — the
    // same strings this shim always keyed on. The whole shim is deleted in
    // Phase 3 once every renderer calls plan_note_text() directly.
    let map: std::collections::HashMap<String, String> = keyed.collect();
    blockers
        .iter()
        .map(|b| b.message_en())
        .map(|b| match map.get(&b) {
            Some(localized) => SharedString::from(localized.clone()),
            None => SharedString::from(b),
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
    /// The active tab's snapshot-derived view data (single source of
    /// truth; ADR-0075 P2). Inactive tabs live in `tab_cache`. Adding a
    /// field to `TabViewState` no longer needs an `apply_tab_view` edit.
    pub active_view: TabViewState,
    /// T-PERF-RENDER-002 (ADR-0116 Wave 2): monotonic counter bumped on every
    /// `active_view` write so the sidebar can cheaply detect that its inputs
    /// (branches/remotes/tags/stashes/worktrees) may have changed without
    /// hashing the full ref lists each frame.  Read into the sidebar-rows
    /// fingerprint in `render`.
    pub view_epoch: u64,
    /// Pre-computed commit rows (built once from the snapshot).
    /// Stash nodes rendered in the graph below the WIP row (ADR-0088).
    /// Lanes used by stash branch lines (passed to the graph painter so those
    /// nodes/edges are drawn in the stash colour).
    /// Pre-computed detail panel data, parallel to `rows`.
    /// Currently selected row index (None = no selection).
    pub selected: Option<usize>,
    /// Error or informational message shown instead of the commit list.
    pub error: Option<SharedString>,
    /// Absolute path to the repository root; used for on-demand diff fetches.
    pub repo_path: Option<PathBuf>,
    /// Per-tab repository session (ADR-0107): owns a `Backend` for the tab
    /// lifetime so read paths don't re-open the repo on every interaction.
    /// `None` when no repo is open (same lifecycle as `repo_path`). Cloning
    /// is cheap (`Rc` bump); the underlying repository handle is opened once.
    /// Mutating ops still open a fresh `Backend` (the `*_blocking` pattern)
    /// because `run()` needs `&mut self` and the session is `Rc`, not
    /// `Arc+Mutex` — that collapses when the worker thread (ADR-0073) lands.
    pub repo_session: Option<kagi_git::session::RepoSession>,
    /// Per-row diff / changed-files cache cluster (T-DECOMP-002, ADR-0118).
    /// The five formerly-flat fields (`changed_files` / `file_content` /
    /// `remote_inflight` / `local_inflight` / `diffstat`) now move and
    /// invalidate as a unit via `diff_caches.clear()`.
    pub diff_caches: diff_cache::DiffCaches,
    /// Aggregated staged + unstaged additions/deletions for the synthetic WIP row.
    pub wip_diffstat: Option<WipDiffStat>,
    /// T-UI-003: When `Some`, the main pane shows this diff (full-width) instead
    /// of the commit graph list.  Cleared when `selected` changes or on reload.
    /// ADR-0121 B2: now a fat entity (`main_diff_pane.rs`) that owns the
    /// `MainDiffView`, the diff-list `ListState`, and the highlight swap-in.
    pub main_diff: Option<Entity<MainDiffPane>>,
    /// Headless-only staging for `KAGI_OPEN_FIRST_FILE` (ADR-0121 B2): the
    /// hook runs before any gpui context exists, so it can't create the
    /// `MainDiffPane` entity. `render` promotes this into `main_diff` on the
    /// first frame. Always `None` in the GUI paths.
    pub pending_headless_diff: Option<MainDiffView>,
    /// ADR-0026: read-only compare mode shown in the inspector changed-files area.
    /// Cleared on selection change or reload to avoid stale path/diff state.
    /// ADR-0121 B2: now an entity (`compare_pane.rs`) that owns the
    /// `CompareView`, registered as `workspace::CompareItem`.
    pub compare_view: Option<Entity<ComparePane>>,
    /// Headless-only staging for `KAGI_COMPARE_HEAD` / `KAGI_COMPARE_WT`
    /// (ADR-0121 B2): the hook runs before any gpui context exists, so it
    /// can't create the `ComparePane` entity. `render` promotes this into
    /// `compare_view` on the first frame. Always `None` in the GUI paths.
    pub pending_headless_compare: Option<CompareView>,
    /// Local branch names from the snapshot, ordered by name.
    /// Used to render the sidebar.  The first element of the tuple is the
    /// branch name; the second is whether it is the current HEAD branch.
    /// The single active modal (ADR-0076 / issue #13 P7). At most one modal is
    /// open at a time; this replaces the ~22 mutually-exclusive `Option<XModal>`
    /// fields that used to live here. Access goes through the generated
    /// accessor methods (`plan_modal()`, `set_plan_modal()`, `clear_plan_modal()`,
    /// `take_plan_modal()`, …) so existing call sites keep their per-modal names.
    pub active_modal: Option<ActiveModal>,
    /// When `Some`, the remote SSH connect / directory-browse modal is visible
    /// (ADR-0089 Phase 1).
    pub remote_browse_modal: Option<RemoteBrowseModal>,
    /// When `Some`, the main views are showing a **remote** repository opened
    /// read-only over SSH (ADR-0089 Phase 2b). `repo_path` is `None` in this
    /// mode, so every local-path operation (checkout/commit/diff/watcher/…)
    /// guards itself off automatically; this marker drives the read-only UI and
    /// keeps the workspace (not the welcome screen) visible with no local tab.
    pub remote_view: Option<RemoteRepoView>,
    /// Focus handle used to receive keyboard events for the create-branch modal.
    /// Allocated on demand when the modal is first opened.
    pub modal_focus: Option<FocusHandle>,
    /// Stash entries from the snapshot, ordered by index (newest = index 0).
    /// Whether the working tree is dirty (used to show/hide the Stash button).
    /// Focus handle for the stash push modal text input.
    pub stash_push_focus: Option<FocusHandle>,
    /// Status footer message (T017): the result of the most recent operation.
    pub status_footer: FooterStatus,
    /// Repository-Navigator (left sidebar) state: width, scroll handle, rows,
    /// collapsed sections, filter input, visibility. Consolidated from six flat
    /// `sidebar_*` fields (ADR-0110 Phase 5 Step 5.1). App-global; preserved
    /// across reloads.
    pub sidebar: sidebar::SidebarState,
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
    /// Time bucketing for the bottom-panel "Activity" chart (Day/Week/Month).
    pub activity_granularity: kagi_domain::activity::Granularity,
    /// Index of the chart bucket the pointer is over (instant hover readout),
    /// or `None`. Reset on leave / granularity change.
    pub activity_hover: Option<usize>,
    // ── T025: Commit Panel ───────────────────────────────────────
    /// Whether the commit panel is currently open (WIP row selected).
    pub commit_panel_open: bool,
    /// ADR-0118 (Phase 5.2) / T-ENTITY-COMMITPANEL-001: the Commit Panel promoted
    /// to its own `Entity<CommitPanelView>` (self-rendering child with its own
    /// notify scope). The entity OWNS the staging lists + the message/template
    /// `InputState`s + the per-branch draft autosave + the queued smart message.
    /// `commit_panel_open` is the visibility gate (set by graph `select`);
    /// `Some(entity)` = cached panel state. Read its data via `e.read(cx).state`.
    pub commit_panel: Option<Entity<commit_panel::CommitPanelView>>,
    // ── T-COMMIT-016: Smart Commit Message (W14-SMART) ───────────
    /// Smart Commit state: rule-based always on, LLM opt-in + detection. Stays on
    /// `KagiApp` (read by the Settings overlay + command palette, written by the
    /// background detection probe — cross-cutting, not commit-panel-private).
    pub smart_commit: smart_commit::SmartCommitState,
    /// Guard so Ollama detection runs at most once per repo path.
    pub smart_commit_detected_for: Option<PathBuf>,
    // ── T028: branch jump (scroll to commit) ─────────────────
    /// Scroll handle for the "commit-list" uniform_list.
    /// Stored in KagiApp so it persists across render frames.
    pub commit_scroll_handle: UniformListScrollHandle,
    /// Current commit-walk limit for the main graph. Starts at
    /// [`DEFAULT_COMMIT_LIMIT`] and grows by [`COMMIT_PAGE_STEP`] each time the
    /// user clicks "load more" at the bottom of the commit list. All paths that
    /// rebuild the main view (`reload`, `reload_external`, tab load) snapshot at
    /// this limit so loaded-more commits survive a refresh.
    pub commit_limit: usize,
    /// Maps local branch name → the CommitId it points to.
    /// Built at snapshot time; used by jump_to_branch.
    /// Maps CommitId → row index in `self.active_view.rows`.
    /// Built at snapshot time; used by jump_to_branch.
    // ── T-BP-003: StatusBar summary ──────────────────────────────
    /// Pre-computed status bar data (branch, ahead/behind, staged, unstaged).
    /// Updated on every reload; rendered by `render_status_bar`.
    // ── T-HT-001: Toolbar state ──────────────────────────────────
    /// Pre-computed toolbar button enabled/disabled flags.
    /// Updated on every reload; rendered by `render_header_slot`.
    // ── T-BP-004: Operation Log entries ─────────────────────────
    /// Operation log panel — an `Entity<OpLogPanel>` (ADR-0110 Phase 5 Step 5.1)
    /// so a push / row-expand re-renders only the panel subtree, not the whole
    /// app. The ring buffer + the expanded-row / scroll UI state live on the
    /// entity. `Option` because the pure constructors have no `cx`; created in
    /// `open_main_window`'s `cx.new` closure from `op_log_seed`.
    pub op_log: Option<Entity<oplog_panel::OpLogPanel>>,
    /// Disk-loaded startup tail (read by the pure constructors), held until
    /// `open_main_window` moves it into the `op_log` entity. Read by the
    /// `KAGI_BOTTOM` headless dump before the window exists; empty afterwards.
    pub op_log_seed: VecDeque<OpLogEntry>,
    // ── T-UNDOREDO-001 / ADR-0081: Operation Undo / Redo history ──
    /// In-session undo/redo stack of ref-moving operations (commit, merge,
    /// cherry-pick, revert, amend, undo-commit). Entries record the branch and
    /// the before/after commit SHAs; undo/redo move the branch ref between them
    /// via the safe pipeline. Lost on quit (reflog is the durable backstop).
    pub operation_history: kagi_git::OperationHistory,
    /// Whether the reflog-seed of `operation_history` has been attempted for the
    /// current repo (ADR-0084). Set on the first render with a repo open so undo
    /// works on a freshly-opened repo (the initial CLI/snapshot path never calls
    /// `reload()`); reset on reload / tab switch so the next repo re-seeds.
    pub history_seed_attempted: bool,
    /// Set while an Undo/Redo plan modal is open; carries the entry being
    /// previewed and whether it is an undo (`true`) or redo (`false`).
    /// (Stored in `active_modal` — see the `history_modal()` accessor.)
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
    /// Settings overlay: the appearance-section theme picker's gpui-component
    /// `Select` state. Built in the window context (needs a `Window`) and `None`
    /// until then / in headless paths; a `SelectEvent::Confirm` subscription
    /// applies the chosen theme via `set_theme`.
    pub theme_select: Option<Entity<settings_view::ThemeSelectState>>,
    /// ADR-0119: multi-line editor backing the Settings → "Analyze ignore"
    /// section (the gitignore-format exclude file). Lazily created when Settings
    /// opens (needs a `Window`).
    pub analyze_ignore_input: Option<Entity<InputState>>,
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
    /// ADR-0089: measured (top, bottom) screen-px bounds of the File History
    /// list+diff split region, recorded by a paint-time canvas so the
    /// list/diff divider drag maps the cursor to the real region (constant
    /// offsets miss the variable-height header).
    pub file_history_geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    // ── W2-SIDEBAR: Repository Navigator ────────────────────────
    /// Remote-tracking branches from the snapshot (for REMOTE BRANCHES section).
    /// Tags from the snapshot (for TAGS section).
    /// Worktrees from the snapshot (for WORKTREES section).
    /// W13-BRANCHTREE: collapsed branch *groups* (the `/`-prefix sub-trees
    /// inside LOCAL / REMOTE BRANCHES). Keys are dynamic strings of the form
    /// `local:feat` / `remote:origin` — hence a separate `HashSet<String>`
    /// rather than the `&'static str` `sidebar.collapsed` set.
    /// Default-expanded (a key present ⇒ that group is collapsed), mirroring
    /// `sidebar.collapsed` semantics. Preserved across reloads.
    pub branch_groups_collapsed: HashSet<String>,
    // ── W3-NOTIFY: snackbar toasts + async-op state ──────────────
    /// Toast notification stack — an `Entity<ToastStack>` (ADR-0110 Phase 5) so
    /// a push/expire re-renders only the overlay subtree, not the whole app.
    /// The cards render via `impl Render for ToastStack`; the ticker + slide-out
    /// timers live on the entity. `Option` because the pure `KagiApp`
    /// constructors have no `cx`; it is created in `open_main_window`'s
    /// `cx.new` closure and is `None` only before the window exists.
    pub toast_stack: Option<Entity<toast_stack::ToastStack>>,
    /// True while a background fetch is in flight (refresh / auto-fetch),
    /// so we never stack concurrent fetches.
    pub fetch_in_flight: bool,
    /// True while the periodic background auto-fetch ticker task is alive
    /// (spawned lazily from render; see `ensure_auto_fetch_ticker`).
    pub auto_fetch_ticker_alive: bool,
    /// When `Some`, the refresh icon spins (set on click; cleared after one
    /// full rotation in render).
    pub refresh_spin_started: Option<Instant>,
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
    /// Commit row context menu state (right-click anchor + target row).
    pub commit_menu: Option<CommitMenuState>,
    /// Branch sidebar context menu state (right-click anchor + target branch).
    pub branch_menu: Option<BranchMenuState>,
    pub stash_menu: Option<stash_menu::StashMenuState>,
    pub worktree_menu: Option<worktree_menu::WorktreeMenuState>,
    /// Unstaged file-row context menu (right-click): (unstaged index, anchor).
    /// Offers Discard for eligible (tracked, non-conflicted) rows.
    pub file_menu: Option<(usize, gpui::Point<gpui::Pixels>)>,
    // ── W5-MENU: command registry / menu bar ─────────────────
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
    /// Resolved-avatar cache (memory images + per-repo fetch guard), grouped
    /// into one cohesive sub-struct (ADR-0118 Phase 5.2).
    pub avatars: avatar::AvatarStore,
    // ── W30-CONFLICT-UI: Conflict Mode (ADR-0056) ────────────────
    /// Conflict-resolution panel, promoted to its own `Entity<ConflictView>`
    /// (ADR-0118 Phase 5.2 / T-ENTITY-CONFLICT-001, mirroring the ADR-0117
    /// FileHistory fat-entity template). `Some` while a conflict/merge is in
    /// progress; the entity owns the detected mode, the open editor file, the
    /// A/B/Result inputs, splits/geometry, and its own `cx.notify()` scope.
    /// Built / dropped by `apply_conflict_detect`; cleared on reload / abort /
    /// tab switch. The per-repo run-once guard (`detected_for`),
    /// `conflict_merge_pending`, the `conflict_count` badge, and
    /// `merge_commit_ready` stay on `KagiApp` (separate concerns).
    pub conflict: Option<Entity<conflict_view::ConflictView>>,
    /// Per-repo run-once guard for conflict detection (was `ConflictState.
    /// detected_for`). Holds the repo path whose conflict state has been detected
    /// this cycle; invalidated on reload / repo change. Parent-owned because it
    /// must survive an entity rebuild and be readable without leasing the entity.
    pub conflict_detected_for: Option<PathBuf>,
    /// T-CONFLICT-FLOW-030/031 (ADR-0068): showing the merge commit panel
    /// (every file saved + staged, MERGE_HEAD still present). Cleared on commit /
    /// abort / reload. Parent-owned (read by the body-gate render and the FS
    /// watcher) — kept off `ConflictState` so the upcoming `ConflictView` entity
    /// flip never has to be leased just to test the gate (ADR-0118 Mechanism B).
    pub conflict_merge_pending: bool,
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
    pub last_working_status: Option<kagi_git::WorkingTreeStatus>,
    /// T-CONFLICT-FLOW-032 (ADR-0068): sequencer `<op> --continue` confirmation
    /// modal, shown when Continue routes a rebase / cherry-pick / revert.
    /// (Stored in `active_modal` — see the `conflict_continue_modal()` accessor.)
    /// ADR-0089 / ADR-0117: File History view, promoted to its own
    /// `Entity<FileHistoryView>` (Phase 5.1). `Some` while the dedicated
    /// single-file history view occupies the center+right area; `None` shows the
    /// normal commit graph / diff body. The entity owns its loads + row menu.
    pub file_history: Option<Entity<file_history::FileHistoryView>>,
    /// HEAD OID the open File History view was last loaded at. On a reload the
    /// view is reloaded in place only when this differs from the new HEAD —
    /// an auto-fetch (remote refs only) leaves it unchanged, so the view is
    /// neither closed nor reloaded.
    pub file_history_head: Option<String>,
    /// ADR-0119: Code Ecosystem / hot-spot view. `Some` while the full-screen
    /// read-only analysis view occupies the center+right area; `None` shows the
    /// normal body. Its own `Entity<EcosystemView>` owns the mining + ranking.
    pub ecosystem: Option<Entity<ecosystem::EcosystemView>>,
    /// ADR-0128: Branch Cleanup takeover open flag. The table data itself
    /// is per-tab (`active_view.cleanup_rows`), so a bool is the whole gate.
    pub branch_cleanup_open: bool,
    /// ADR-0128: Branch Cleanup table column widths (persisted).
    pub cleanup_cols: branch_cleanup::CleanupCols,
    /// ADR-0128: scroll position of the Branch Cleanup uniform list.
    pub cleanup_scroll: UniformListScrollHandle,
    /// ADR-0119: cached completed mine so reopening the Ecosystem view reuses
    /// the slow `git log` scan. Invalidated on reload / repo switch.
    pub ecosystem_cache: ecosystem::EcosystemCache,
    /// ADR-0119: repo whose Analyze mine is currently running (app-owned, so it
    /// survives the view being closed). `None` when idle.
    pub ecosystem_inflight: Option<std::path::PathBuf>,
    /// Monotonic token identifying the *current* Analyze mine. A completing
    /// background task only wins if this still equals the value it captured at
    /// start — so a stale same-repo mine (e.g. one started before a reload
    /// superseded it) can't cache/seed its result over a newer one.
    pub ecosystem_gen: u64,
    /// T-WS-EDITOR-001 / ADR-0120: the Editor workspace view — `Some` while
    /// Graph ⇄ Editor mode is `Editor` (T-WS-EDITOR-005 finding #11: mode is
    /// derived as `editor_workspace.is_some()` rather than tracked in a
    /// separate `workspace_mode` field, so the two can't diverge). Its own
    /// `Entity<EditorWorkspaceView>` owns the working-tree file tree, the
    /// selected file's read-only code viewer, and its WIP hunks (ADR-0117
    /// fat-entity template).
    pub editor_workspace: Option<Entity<editor_workspace::EditorWorkspaceView>>,
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

/// Marks the workspace as showing a remote repository opened read-only over SSH
/// (ADR-0089 Phase 2b). Holds what's needed to identify/refresh it; the rendered
/// data lives in the normal `rows`/`branches`/… fields (applied from a remote
/// `RepoSnapshot` via [`KagiApp::apply_tab_view`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRepoView {
    /// The connected host.
    pub host: kagi_domain::remote::RemoteHost,
    /// Absolute path of the repository on the remote.
    pub root: String,
}

#[derive(Clone)]
pub struct BranchSolo {
    pub name: String,
    pub target: CommitId,
    pub visible_commits: HashSet<CommitId>,
    /// Full (un-soloed) row/detail/index data, restored on "Exit Solo".
    /// Solo now HIDES non-history rows (user request; was opacity-dimming),
    /// which requires re-running the lane layout on the filtered sub-DAG —
    /// the ancestry closure is parent-complete, so the layout stays valid.
    pub saved_rows: Vec<CommitRow>,
    pub saved_details: Vec<CommitDetail>,
    pub saved_row_index: HashMap<CommitId, usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WipDiffStat {
    pub additions: usize,
    pub deletions: usize,
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

        KagiApp {
            // Created in `open_main_window`'s `cx.new` closure from this seed.
            op_log: None,
            op_log_seed: op_entries,
            root_focus: None,
            active_view: view,
            view_epoch: 0,
            selected: None,
            error: None,
            repo_path: None,
            repo_session: None,
            diff_caches: diff_cache::DiffCaches::default(),
            wip_diffstat: None,
            main_diff: None,
            pending_headless_diff: None,
            compare_view: None,
            pending_headless_compare: None,
            active_modal: None,
            remote_browse_modal: None,
            remote_view: None,
            modal_focus: None,
            stash_push_focus: None,
            status_footer: FooterStatus::Idle(SharedString::from("Ready")),
            sidebar: sidebar::SidebarState::new(),
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
            activity_granularity: kagi_domain::activity::Granularity::Week,
            activity_hover: None,
            commit_panel_open: false,
            commit_panel: None,
            smart_commit: smart_commit::SmartCommitState::load(),
            smart_commit_detected_for: None,
            commit_scroll_handle: UniformListScrollHandle::new(),
            commit_limit: DEFAULT_COMMIT_LIMIT,
            operation_history: kagi_git::OperationHistory::new(),
            history_seed_attempted: false,
            terminal_sessions: HashMap::new(),
            tabs: Vec::new(),
            active_tab: 0,
            watcher_generation: 0,
            inspector_tree_view: true,
            inspector_split: INSPECTOR_SPLIT_DEFAULT,
            inspector_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            file_history_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            graph_compact: theme::compact_graph(),
            theme_select: None,
            analyze_ignore_input: None,
            graph_scroll_x: 0.0,
            // W2-SIDEBAR
            branch_groups_collapsed: HashSet::new(),
            // W3-NOTIFY
            // Created in `open_main_window`'s `cx.new` closure (needs `cx`).
            toast_stack: None,
            fetch_in_flight: false,
            auto_fetch_ticker_alive: false,
            busy_op: None,
            modal_replan_gen: 0,
            refresh_spin_started: None,
            // W2-DELETE
            commit_menu: None,
            branch_menu: None,
            stash_menu: None,
            worktree_menu: None,
            file_menu: None,
            // W5-MENU
            inspector_visible: true,
            menu_overlay: None,
            platform_menu_open: None,
            // W6-TABSPEED
            tab_cache: HashMap::new(),
            switch_generation: 0,
            loading_tab: None,
            // W11-AVATAR
            avatars: avatar::AvatarStore::default(),
            // W30-CONFLICT-UI
            conflict: None,
            conflict_detected_for: None,
            conflict_merge_pending: false,
            merge_commit_ready: false,
            update_available: None,
            update_checked: false,
            update_modal_open: false,
            update_installing: false,
            update_status: None,
            last_working_status: None,
            file_history: None,
            file_history_head: None,
            ecosystem: None,
            branch_cleanup_open: false,
            cleanup_cols: branch_cleanup::CleanupCols::load(),
            cleanup_scroll: UniformListScrollHandle::new(),
            ecosystem_cache: ecosystem::EcosystemCache::new(),
            ecosystem_inflight: None,
            ecosystem_gen: 0,
            editor_workspace: None,
        }
    }

    /// Construct a placeholder for the no-argument / error case.
    pub fn with_error(message: impl Into<String>) -> Self {
        KagiApp {
            root_focus: None,
            active_view: TabViewState {
                header: SharedString::from("kagi"),
                ..Default::default()
            },
            view_epoch: 0,
            selected: None,
            error: Some(SharedString::from(message.into())),
            repo_path: None,
            repo_session: None,
            diff_caches: diff_cache::DiffCaches::default(),
            wip_diffstat: None,
            main_diff: None,
            pending_headless_diff: None,
            compare_view: None,
            pending_headless_compare: None,
            active_modal: None,
            remote_browse_modal: None,
            remote_view: None,
            modal_focus: None,
            stash_push_focus: None,
            status_footer: FooterStatus::Idle(SharedString::from("Ready")),
            sidebar: sidebar::SidebarState::new(),
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
            activity_granularity: kagi_domain::activity::Granularity::Week,
            activity_hover: None,
            commit_panel_open: false,
            commit_panel: None,
            smart_commit: smart_commit::SmartCommitState::load(),
            smart_commit_detected_for: None,
            commit_scroll_handle: UniformListScrollHandle::new(),
            commit_limit: DEFAULT_COMMIT_LIMIT,
            op_log: None,
            op_log_seed: VecDeque::new(),
            operation_history: kagi_git::OperationHistory::new(),
            history_seed_attempted: false,
            terminal_sessions: HashMap::new(),
            tabs: Vec::new(),
            active_tab: 0,
            watcher_generation: 0,
            inspector_tree_view: true,
            inspector_split: INSPECTOR_SPLIT_DEFAULT,
            inspector_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            file_history_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            graph_compact: theme::compact_graph(),
            theme_select: None,
            analyze_ignore_input: None,
            graph_scroll_x: 0.0,
            // W2-SIDEBAR
            branch_groups_collapsed: HashSet::new(),
            // W3-NOTIFY
            // Created in `open_main_window`'s `cx.new` closure (needs `cx`).
            toast_stack: None,
            fetch_in_flight: false,
            auto_fetch_ticker_alive: false,
            busy_op: None,
            modal_replan_gen: 0,
            refresh_spin_started: None,
            // W2-DELETE
            commit_menu: None,
            branch_menu: None,
            stash_menu: None,
            worktree_menu: None,
            file_menu: None,
            // W5-MENU
            inspector_visible: true,
            menu_overlay: None,
            platform_menu_open: None,
            // W6-TABSPEED
            tab_cache: HashMap::new(),
            switch_generation: 0,
            loading_tab: None,
            // W11-AVATAR
            avatars: avatar::AvatarStore::default(),
            // W30-CONFLICT-UI
            conflict: None,
            conflict_detected_for: None,
            conflict_merge_pending: false,
            merge_commit_ready: false,
            update_available: None,
            update_checked: false,
            update_modal_open: false,
            update_installing: false,
            update_status: None,
            last_working_status: None,
            file_history: None,
            file_history_head: None,
            ecosystem: None,
            branch_cleanup_open: false,
            cleanup_cols: branch_cleanup::CleanupCols::load(),
            cleanup_scroll: UniformListScrollHandle::new(),
            ecosystem_cache: ecosystem::EcosystemCache::new(),
            ecosystem_inflight: None,
            ecosystem_gen: 0,
            editor_workspace: None,
        }
    }

    // ── W30-CONFLICT-UI: Conflict Mode (ADR-0056) ────────────────────────

    /// T-PERF-RENDER-001 (ADR-0116 Wave 2): arm the per-repo background I/O that
    /// used to run synchronously in `render()` — conflict detection, undo/redo
    /// reflog seeding, and the auto-fetch ticker.  Called from the reload /
    /// tab-switch / app-init commit points (`switch_repo`, `open_main_window`)
    /// rather than every frame.  Each sub-task carries its own run-once guard
    /// (`conflict.detected_for`, `history_seed_attempted`, `auto_fetch_ticker_alive`)
    /// — reset on tab switch / reload exactly as before — so repeated calls are
    /// cheap no-ops and the emitted `[kagi]` contract lines fire once per repo.
    pub fn ensure_startup_repo_io(&mut self, cx: &mut Context<Self>) {
        // W30-CONFLICT-UI: detect Conflict Mode once per repo path (no-op when
        // already detected this cycle). The watcher / post-operation paths force
        // re-detection via the synchronous `reload()`.
        self.detect_conflict_mode_async(cx);

        // ADR-0084: seed the undo/redo history from the reflog once per repo so
        // Cmd+Z works on a freshly-opened repo (the initial CLI/snapshot path
        // never calls `reload()`). Only-when-empty, so it never clobbers an
        // in-session stack.
        if !self.history_seed_attempted {
            self.history_seed_attempted = true;
            self.seed_history_from_reflog_async(cx);
        }

        // Background auto-fetch ticker (periodic `git fetch` so the graph and
        // ahead/behind stay fresh). Lazily spawned; no-op when off / no repo.
        self.ensure_auto_fetch_ticker(cx);
    }

    /// Detect (or clear) Conflict Mode for the currently-open repository.
    ///
    /// Runs at most once per `repo_path` per cycle (the `conflict_detected_for`
    /// guard, reset by `reload()` / tab switch / the watcher).  Opens the repo
    /// read-only, calls `detect_conflict_session`, and on a hit builds a fresh
    /// `ResolutionBuffer` from the index (preferring a previously autosaved
    /// buffer so a partial resolution survives a restart), recomputes each
    /// file's status from the buffer, and stores the `ConflictMode` (via
    /// `apply_conflict_detect`, which builds / updates the `ConflictView` entity).
    /// On a miss it drops the entity (`self.conflict = None`). The repository is
    /// never mutated here.
    pub fn detect_conflict_mode(&mut self, cx: &mut Context<Self>) {
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

        // Snapshot the preservation inputs the I/O step needs (prev selection /
        // editing index), then run the read-only Git/index/file I/O synchronously.
        let (prev_selected, prev_editing_file) = self
            .conflict
            .as_ref()
            .map(|e| {
                let v = e.read(cx);
                (
                    v.mode.as_ref().and_then(|c| c.selected_file),
                    v.mode.as_ref().and_then(|c| c.editing_file),
                )
            })
            .unwrap_or((None, None));
        let current_branch = self.active_view.status_summary.branch.clone();
        let outcome = Self::detect_conflict_payload(
            &repo_path,
            prev_selected,
            prev_editing_file,
            current_branch,
        );
        self.apply_conflict_detect(outcome, cx);
    }

    /// T-PERF-RENDER-001: async sibling of [`detect_conflict_mode`].
    ///
    /// Runs the same read-only Backend / index / `ResolutionBuffer` I/O on a
    /// background thread (`cx.background_spawn`), then marshals the result back to
    /// [`apply_conflict_detect`] on the UI thread.  The run-once `detected_for`
    /// guard is armed up-front (mirroring the sync path's "set guard, then do
    /// I/O" ordering) so repeated calls — including the per-frame render path that
    /// used to live here — never re-launch the work.  Used by the startup /
    /// tab-switch commit points where `reload()` (which calls the sync variant)
    /// did not run.
    pub fn detect_conflict_mode_async(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => {
                self.conflict = None;
                return;
            }
        };
        // Run-once guard per repo path (armed before the I/O launches).
        if self.conflict_detected_for.as_deref() == Some(repo_path.as_path()) {
            return;
        }
        self.conflict_detected_for = Some(repo_path.clone());

        let (prev_selected, prev_editing_file) = self
            .conflict
            .as_ref()
            .map(|e| {
                let v = e.read(cx);
                (
                    v.mode.as_ref().and_then(|c| c.selected_file),
                    v.mode.as_ref().and_then(|c| c.editing_file),
                )
            })
            .unwrap_or((None, None));
        let current_branch = self.active_view.status_summary.branch.clone();

        // codex Q5: capture the repo path the task ran against so a repo switch
        // mid-task discards the stale result at apply time (the `detected_for`
        // guard alone is insufficient — the guard is set for the NEW repo too).
        let task_repo = repo_path.clone();
        let task = cx.background_spawn(async move {
            Self::detect_conflict_payload(
                &repo_path,
                prev_selected,
                prev_editing_file,
                current_branch,
            )
        });
        cx.spawn(async move |this, acx| {
            let outcome = task.await;
            let _ = this.update(acx, |app, cx| {
                // Repo-match check: drop the result if the repo switched while the
                // read-only I/O was in flight.
                if app.repo_path.as_deref() != Some(task_repo.as_path()) {
                    return;
                }
                app.apply_conflict_detect(outcome, cx);
                cx.notify();
            });
        })
        .detach();
    }

    // ────────────────────────────────────────────────────────────
    // W32-CONFLICT-EDITOR: hunk-level editor open/close + hunk dispatch
    // ────────────────────────────────────────────────────────────

    /// Cancel and close the checkout plan modal without making any changes.
    pub fn cancel_modal(&mut self) {
        self.clear_plan_modal();
    }

    // ── Create-branch modal (T014) ───────────────────────────

    // ── Create-worktree modal (T-CM-023) ─────────────────────

    // ── Stash push modal (T015) ──────────────────────────────

    // ── Stash apply modal (T015) ─────────────────────────────

    // ── Cherry-pick modal (T016) ─────────────────────────────

    // ── Revert modal (T-CM-034) ─────────────────────────────

    // ── Oplog + footer helper (T017) ────────────────────────

    /// Record an operation to the oplog and update the status footer.
    ///
    /// Write failures are non-fatal: they emit a stderr warning only.
    // ── W3-NOTIFY: toast helpers ──────────────────────────────

    /// Queue a snackbar toast (bottom-left). Delegates to the `ToastStack`
    /// entity, which (re)starts the auto-dismiss ticker and re-renders only the
    /// overlay subtree (ADR-0110 Phase 5). No-op before the window exists.
    pub(crate) fn push_toast(
        &mut self,
        kind: ToastKind,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        if let Some(stack) = self.toast_stack.clone() {
            stack.update(cx, |stack, cx| stack.push_notify(kind, message, cx));
        }
    }

    /// Debounced live re-plan for the open modal(s): waits 250ms of input
    /// silence before doing git work, so typing stays fluid.
    fn schedule_modal_replan(&mut self, cx: &mut Context<Self>) {
        self.modal_replan_gen = self.modal_replan_gen.wrapping_add(1);
        let gen = self.modal_replan_gen;
        cx.spawn(async move |this, acx| {
            acx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
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
        if self.create_branch_modal().is_some() {
            self.replan_create_branch();
        }
        if self.create_worktree_modal().is_some() {
            self.replan_create_worktree();
        }
        if self.stash_push_modal().is_some() {
            self.replan_stash_push();
        }
        if self.set_upstream_modal().is_some() {
            self.replan_set_upstream();
        }
        if self.rename_branch_modal().is_some() {
            self.replan_rename_branch();
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
        let lane_count = self
            .active_view
            .rows
            .first()
            .map(|r| r.lane_count)
            .unwrap_or(0);
        // W28: scroll content extent uses the scaled lane pitch so a fully
        // zoomed graph can still be scrolled to reveal its rightmost lanes.
        let max = (lane_count as f32 * graph_view::lane_w() - self.graph_col_w).max(0.0);
        let next = (self.graph_scroll_x - dx).clamp(0.0, max);
        if (next - self.graph_scroll_x).abs() > 0.1 {
            self.graph_scroll_x = next;
            cx.notify();
        }
    }

    /// Read the current HEAD branch name + commit SHA from the open repo.
    /// Returns `None` for detached/unborn HEAD or any open/read failure — used
    /// to capture before/after snapshots for the operation-history recording
    /// (T-UNDOREDO-001). The view never holds git2 directly: this goes through
    /// the `kagi_git::Backend`.
    fn head_branch_and_sha(&self) -> Option<(String, kagi_git::CommitId)> {
        let repo_path = self.repo_path.clone()?;
        let backend = kagi_git::Backend::open(&repo_path).ok()?;
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
        cx: &mut Context<Self>,
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
        self.push_toast(toast_kind, footer_msg.clone(), cx);

        // T-BP-004: auto-open bottom panel on Failed.
        let is_failed = matches!(outcome, OpOutcome::Failed { .. });

        let repo_str = repo_path.display().to_string();
        let entry = OpLogEntry::new(op, &repo_str, before, outcome);

        if let Err(e) = append_oplog(&entry) {
            klog!("oplog: write failed (non-fatal): {}", e);
        }

        // T-BP-004: push to in-memory ring-buffer (newest at front) and collapse
        // any expanded row. Scoped to the op-log entity (ADR-0110 Phase 5).
        if let Some(panel) = self.op_log.clone() {
            panel.update(cx, |panel, cx| {
                panel.push(entry);
                panel.collapse();
                cx.notify();
            });
        }

        // T-BP-004: auto-open panel on failure.
        if is_failed {
            self.bottom_panel_open = true;
            self.bottom_tab = BottomTab::OperationLog;
            klog!("bottom-panel: open (Failed auto-open)");
        }

        if footer_ok {
            klog!("footer: {}", footer_msg);
            self.status_footer = FooterStatus::Success(footer_msg);
        } else {
            klog!("footer: {}", footer_msg);
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
                klog!("terminal: no repo_path — cannot start terminal");
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
            use kagi_git::oplog::OpOutcome;
            use kagi_git::ops::StateSummary;
            self.record_op(
                "terminal-start",
                StateSummary {
                    head: "n/a".to_string(),
                    dirty: "n/a".to_string(),
                },
                OpOutcome::Failed { error: err },
                &repo_path,
                cx,
            );
        }
    }

    // ── T-HT-003: Pull ────────────────────────────────────────

    // ── T-HT-004: Push ────────────────────────────────────────

    // ── T-BCM-030/T-BCM-061: Branch menu plans ───────────────

    // ── T-HT-009: Undo Commit / T-HT-007: Stash Pop ──────────

    // ── Operation Undo / Redo (T-UNDOREDO-001, ADR-0081) ─────

    // ── Amend (T-COMMIT-011, ADR-0040) ───────────────────────

    // ── W2-DELETE: Delete-branch modal ───────────────────────

    // ── W17-DISCARD: discard danger modal (ADR-0046) ─────────

    // ── T025: Commit Panel ────────────────────────────────────

    // ── T-COMMIT-009 / W14-TEMPLATE: structured message template ──

    // ── T-COMMIT-016: Smart Commit Message (W14-SMART) ───────────

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
        if settings::read_setting("update_auto_check").as_deref() == Some("false") {
            return;
        }
        let skipped = settings::read_setting("update_skipped");
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
                Ok(None) => klog!("update: up to date"),
                Err(e) => klog!("update: check failed (ignored): {e}"),
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
            kagi::update::install(&plan, &release, &|m| klog!("update: {m}"))
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| match result {
                Ok(relaunch) => {
                    klog!("update: installed — relaunching");
                    relaunch.spawn_and_exit();
                }
                Err(e) => {
                    klog!("update: failed: {e}");
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
            settings::write_setting("update_skipped", Some(&plan.tag));
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

        if let Some(detail) = self.active_view.details.get(index) {
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

        // Changed files + diffstat for this row load lazily OFF the UI thread
        // via the render trigger (`load_local_changed_files`, or
        // `load_remote_changed_files` for a remote view), so selecting a row
        // never blocks the frame on a git diff. The headless harness runs no
        // render loop, so it drives `select_headless` instead, which performs
        // the synchronous load + emits the `[kagi] changed files:` / `tree:`
        // contract deterministically.
    }

    /// Headless-only: select row `index` and SYNCHRONOUSLY load its changed
    /// files + diffstat, emitting the `[kagi] changed files: N` contract and,
    /// under `KAGI_SELECT_FIRST=1`, the `[kagi] tree:` structure dump. The
    /// interactive UI defers this load to the async render trigger
    /// (`load_local_changed_files`), which the headless harness does not run —
    /// so this preserves the old synchronous `select` behaviour, and its exact
    /// stderr ordering, for deterministic verification.
    pub fn select_headless(&mut self, index: usize) {
        self.select(index);
        // `select` toggles off when re-selecting the same row; only a freshly
        // selected row needs the on-demand load.
        if self.selected != Some(index) {
            return;
        }
        if !self.diff_caches.changed_files.contains_key(&index) {
            let files_opt = self.fetch_changed_files(index);
            let n = files_opt.as_ref().map(|v| v.len()).unwrap_or(0);
            klog!("changed files: {}", n);
            self.diff_caches.changed_files.insert(index, files_opt);
            // W16-DIFFSTAT: aggregate per-file additions/deletions alongside.
            if let Some(stats) = self.fetch_diffstat(index) {
                self.diff_caches.diffstat.insert(index, stats);
            }
        } else {
            // Already cached — still emit the log (matches the old select()).
            let n = self
                .diff_caches
                .changed_files
                .get(&index)
                .and_then(|v| v.as_ref())
                .map(|v| v.len())
                .unwrap_or(0);
            klog!("changed files: {}", n);
        }

        // T018: emit tree structure log when KAGI_SELECT_FIRST=1.
        if std::env::var("KAGI_SELECT_FIRST").as_deref() == Ok("1") {
            const MAX_FILES: usize = 100;
            if let Some(Some(files)) = self.diff_caches.changed_files.get(&index) {
                let truncated: Vec<_> = files.iter().take(MAX_FILES).cloned().collect();
                let rows = file_tree::build_file_tree(&truncated);
                for row in &rows {
                    match row {
                        file_tree::TreeRow::Dir { depth, name } => {
                            klog!("tree: {}DIR  {}", "  ".repeat(*depth), name);
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

    // ──────────────────────────────────────────────────────────────
    // ADR-0089: File History view
    // ──────────────────────────────────────────────────────────────

    // `open_file_history` lives in `file_history.rs` (bin glue) since ADR-0121
    // C3: the pane crate is Git-free, so the app owns the loads there.
    /// Move the file-history entry selection up/down by `delta` (arrow keys),
    /// clamped to the entry list. Drives the `FileHistoryView` entity directly
    /// (ADR-0117) so the row highlight AND the diff pane both update. The entity
    /// is not leased here (this runs in `KagiApp`'s root key handler), so the
    /// `update` is safe.
    pub fn step_file_history_selection(&mut self, delta: i64, cx: &mut Context<Self>) {
        if let Some(fh) = self.file_history.clone() {
            fh.update(cx, |v, cx| v.step(delta, cx));
        }
    }

    /// Open File History for a changed-file row in the commit inspector
    /// (导线 #2).  Resolves the path the same way `open_main_diff_commit` /
    /// `open_main_diff_compare` does (commit diff cache or compare view).
    pub fn open_file_history_inspector_file(&mut self, file_index: usize, cx: &mut Context<Self>) {
        if let Some(pane) = self.compare_view.as_ref() {
            // ADR-0121 B2: the view lives inside the ComparePane entity now.
            let path = pane
                .read(cx)
                .view
                .files
                .get(file_index)
                .map(|f| f.path.clone());
            if let Some(path) = path {
                self.open_file_history(path, None, cx);
            }
            return;
        }
        let Some(selected) = self.selected else {
            return;
        };
        let origin = self.commit_id_for_row(selected);
        let path = self
            .diff_caches
            .changed_files
            .get(&selected)
            .and_then(|v| v.as_ref())
            .and_then(|files| files.get(file_index))
            .map(|f| f.path.clone());
        if let Some(path) = path {
            self.open_file_history(path, origin, cx);
        }
    }

    /// Open File History for the file currently shown in the main diff view
    /// (diff-header "History" button, 导线 #3).  Resolves the repo-relative
    /// path from the open `MainDiffView`'s source.
    pub fn open_file_history_from_main_diff(&mut self, cx: &mut Context<Self>) {
        // ADR-0121 B2: the view lives inside the pane entity now.
        let Some(source) = self
            .main_diff
            .as_ref()
            .map(|p| p.read(cx).view.source.clone())
        else {
            return;
        };
        let (path, origin) = match &source {
            MainDiffSource::Unstaged { path } | MainDiffSource::Staged { path } => {
                (path.clone(), None)
            }
            MainDiffSource::Commit {
                row_index,
                file_index,
            } => {
                let path = self
                    .diff_caches
                    .changed_files
                    .get(row_index)
                    .and_then(|v| v.as_ref())
                    .and_then(|files| files.get(*file_index))
                    .map(|f| f.path.clone());
                let origin = self.commit_id_for_row(*row_index);
                match path {
                    Some(p) => (p, origin),
                    None => return,
                }
            }
            MainDiffSource::Compare { file_index, .. } => {
                // ADR-0121 B2: the view lives inside the ComparePane entity now.
                let path = self
                    .compare_view
                    .as_ref()
                    .and_then(|p| p.read(cx).view.files.get(*file_index).cloned())
                    .map(|f| f.path);
                match path {
                    Some(p) => (p, None),
                    None => return,
                }
            }
        };
        self.close_main_diff();
        self.open_file_history(path, origin, cx);
    }

    /// Close the File History view (Back → returns to the commit graph).
    /// ADR-0117: dropping the `Entity<FileHistoryView>` tears down its state and
    /// row menu; an in-flight load no-ops on the dropped entity. Refresh /
    /// follow-toggle / select now live on the entity (`reload` / `step` /
    /// `select`).
    pub fn close_file_history(&mut self) {
        self.file_history = None;
    }

    /// Load the selected remote commit's changed files over SSH (ADR-0089 Phase
    /// 2c) and cache them under `index`. Runs off the UI thread; idempotent via
    /// `diff_caches.remote_inflight`.
    fn load_remote_changed_files(&mut self, index: usize, cx: &mut Context<Self>) {
        // Idempotent: skip if already loaded or a load is in flight, so it is
        // safe to call from both the click handler and the render trigger.
        if self.diff_caches.changed_files.contains_key(&index)
            || self.diff_caches.remote_inflight.contains(&index)
        {
            return;
        }
        let (host, root) = match &self.remote_view {
            Some(v) => (v.host.clone(), v.root.clone()),
            None => return,
        };
        let sha = match self.active_view.details.get(index) {
            Some(d) => d.full_sha.as_ref().to_string(),
            None => return,
        };
        self.diff_caches.remote_inflight.insert(index);

        let task = cx.background_spawn(async move {
            kagi::remote::remote_commit_changed_files(&host, &root, &sha).map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.diff_caches.remote_inflight.remove(&index);
                match result {
                    Ok(files) => {
                        app.diff_caches.changed_files.insert(index, Some(files));
                    }
                    Err(e) => {
                        klog!("remote changed-files error: {e}");
                        app.diff_caches.changed_files.insert(index, None);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Load the selected LOCAL commit's changed files + per-file diffstat OFF the
    /// UI thread and cache them under `index`. Idempotent via `diff_caches.local_inflight`
    /// so it is safe to call every frame from the render trigger; `select` only
    /// records the selection. Emits the `[kagi] changed files: N` contract on
    /// completion — the interactive counterpart of `select_headless`.
    ///
    /// The repo is re-opened inside the background task (a `git2` handle is
    /// `!Send`); only the plain `kagi_git` result data crosses back. The result
    /// is dropped if a history reload has remapped the row to a different commit
    /// (the captured SHA no longer matches the row), so a late load can't show
    /// the wrong commit's files.
    fn load_local_changed_files(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.diff_caches.changed_files.contains_key(&index)
            || self.diff_caches.local_inflight.contains(&index)
        {
            return;
        }
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        let Some(detail) = self.active_view.details.get(index) else {
            return;
        };
        let sha = detail.full_sha.as_ref().to_string();
        let sha_guard = sha.clone();
        self.diff_caches.local_inflight.insert(index);

        let task = cx.background_spawn(async move {
            let repo = kagi_git::Backend::open(&repo_path).ok()?;
            let id = kagi_git::CommitId(sha);
            let files = repo.commit_changed_files(&id).ok();
            let stats = repo.commit_diffstat(&id).ok();
            Some((files, stats))
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                app.diff_caches.local_inflight.remove(&index);
                // Drop the result if a reload remapped this row to another commit.
                let still_current = app
                    .active_view
                    .details
                    .get(index)
                    .is_some_and(|d| d.full_sha.as_ref() == sha_guard);
                if !still_current {
                    return;
                }
                let (files, stats) = result.unwrap_or((None, None));
                let n = files.as_ref().map(|v| v.len()).unwrap_or(0);
                klog!("changed files: {}", n);
                app.diff_caches.changed_files.insert(index, files);
                if let Some(stats) = stats {
                    app.diff_caches.diffstat.insert(index, stats);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Fetch changed files for the commit at `index`.  Returns `None` on
    /// failure (so the UI can show "(diff unavailable)").
    fn fetch_changed_files(&self, index: usize) -> Option<Vec<FileStatus>> {
        use kagi_git::CommitId;

        // Early-exit if no repo is open (the session is None in that case too).
        if self.repo_session.is_none() {
            return None;
        }
        let detail = self.active_view.details.get(index)?;
        let id = CommitId(detail.full_sha.as_ref().to_string());

        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let repo = self.repo_session.as_ref()?.backend();
        repo.commit_changed_files(&id).ok()
    }

    /// W16-DIFFSTAT: aggregate per-file additions/deletions for the commit at
    /// `index`.  Returns `None` on failure (the UI simply omits the bar).
    fn fetch_diffstat(&self, index: usize) -> Option<Vec<FileDiffStat>> {
        use kagi_git::CommitId;

        let repo_path = self.repo_path.as_ref()?;
        let detail = self.active_view.details.get(index)?;
        let id = CommitId(detail.full_sha.as_ref().to_string());

        let repo = kagi_git::Backend::open(repo_path).ok()?;
        repo.commit_diffstat(&id).ok()
    }

    fn wip_diffstat_from_backend(repo: &kagi_git::Backend) -> WipDiffStat {
        let mut out = WipDiffStat::default();
        for stat in repo.staged_diffstat().unwrap_or_default() {
            out.additions += stat.additions;
            out.deletions += stat.deletions;
        }
        for stat in repo.unstaged_diffstat().unwrap_or_default() {
            out.additions += stat.additions;
            out.deletions += stat.deletions;
        }
        out
    }

    pub fn refresh_wip_diffstat(&mut self) {
        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        // When no repo is open (session is None), wip_diffstat is cleared.
        self.wip_diffstat = self
            .repo_session
            .as_ref()
            .map(|s| Self::wip_diffstat_from_backend(s.backend()));
    }

    pub fn close_compare_view(&mut self) {
        self.compare_view = None;
        // ADR-0121 B2: also drop a not-yet-promoted headless staging view.
        self.pending_headless_compare = None;
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
        } else if !self.diff_caches.changed_files.contains_key(&row_index) {
            let files_opt = self.fetch_changed_files(row_index);
            let n = files_opt.as_ref().map(|v| v.len()).unwrap_or(0);
            klog!("changed files: {}", n);
            self.diff_caches.changed_files.insert(row_index, files_opt);
            if let Some(stats) = self.fetch_diffstat(row_index) {
                self.diff_caches.diffstat.insert(row_index, stats);
            }
        }
    }

    /// ADR-0121 B2: `cx` is `None` only on the headless path (pre-window, no
    /// gpui context) — the built view is then staged in
    /// `pending_headless_compare` and promoted by `render` on the first frame,
    /// mirroring `set_commit_main_diff`.
    pub fn open_compare_with_head(&mut self, target: CommitId, cx: Option<&mut Context<Self>>) {
        let row_index = match self.row_for_commit_id(&target) {
            Some(ix) => ix,
            None => return,
        };
        if self.selected != Some(row_index) {
            self.select(row_index);
        }

        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let Some(session) = self.repo_session.as_ref() else {
            return;
        };
        let repo = session.backend();
        let head = match repo.head_commit_id() {
            Some(id) => id,
            None => {
                klog!("compare: HEAD unavailable");
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
                let view = CompareView {
                    base: target,
                    target: CompareTarget::Head,
                    files,
                    title,
                };
                match cx {
                    Some(cx) => self.show_compare(view, cx),
                    None => self.pending_headless_compare = Some(view),
                }
            }
            Err(e) => {
                klog!("compare: error: {}", e);
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("Compare failed: {}", e)));
            }
        }
    }

    /// ADR-0121 B2: `cx` — see [`Self::open_compare_with_head`].
    pub fn open_compare_with_working_tree(
        &mut self,
        target: CommitId,
        cx: Option<&mut Context<Self>>,
    ) {
        let row_index = match self.row_for_commit_id(&target) {
            Some(ix) => ix,
            None => return,
        };
        if self.selected != Some(row_index) {
            self.select(row_index);
        }

        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let Some(session) = self.repo_session.as_ref() else {
            return;
        };
        let repo = session.backend();
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
                klog!("compare: status error: {}", e);
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
                let view = CompareView {
                    base: target,
                    target: CompareTarget::WorkingTree,
                    files,
                    title,
                };
                match cx {
                    Some(cx) => self.show_compare(view, cx),
                    None => self.pending_headless_compare = Some(view),
                }
            }
            Err(e) => {
                klog!("compare: error: {}", e);
                self.status_footer =
                    FooterStatus::Failed(SharedString::from(format!("Compare failed: {}", e)));
            }
        }
    }

    pub fn open_compare_with_head_row(&mut self, row_index: usize) {
        match self.commit_id_for_row(row_index) {
            // Headless-only entry point (KAGI_COMPARE_HEAD) — no cx yet.
            Some(target) => self.open_compare_with_head(target, None),
            None => klog!("compare: row={} out of range", row_index),
        }
    }

    pub fn open_compare_with_working_tree_row(&mut self, row_index: usize) {
        match self.commit_id_for_row(row_index) {
            // Headless-only entry point (KAGI_COMPARE_WT) — no cx yet.
            Some(target) => self.open_compare_with_working_tree(target, None),
            None => klog!("compare: row={} out of range", row_index),
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
        let target = match self.active_view.branch_targets.get(branch_name) {
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
        let row_ix = match self.active_view.commit_row_index.get(&target) {
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

        klog!("jump: {} -> row {}", branch_name, row_ix);

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
        let row_ix = match self.active_view.commit_row_index.get(target) {
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
        klog!("jump: commit {} -> row {}", target.short(), row_ix);
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
        if self.active_view.rows.get(row_index).is_none() {
            return;
        }
        if self.selected != Some(row_index) {
            self.select(row_index);
        }
        self.commit_menu = Some(CommitMenuState {
            row_index,
            position,
        });
        klog!("context-menu: open row={}", row_index);
        self.log_commit_menu(row_index);
    }

    /// Headless path for KAGI_CONTEXT_MENU=<row>.
    pub fn open_commit_menu_headless(&mut self, row_index: usize) {
        if self.active_view.rows.get(row_index).is_none() {
            klog!("context-menu: row={} out of range", row_index);
            return;
        }
        if self.selected != Some(row_index) {
            self.select(row_index);
        }
        klog!("context-menu: open row={}", row_index);
        self.log_commit_menu(row_index);
    }

    fn commit_id_for_row(&self, row_index: usize) -> Option<CommitId> {
        self.active_view
            .details
            .get(row_index)
            .map(|detail| CommitId(detail.full_sha.as_ref().to_string()))
    }

    fn row_for_commit_id(&self, target: &CommitId) -> Option<usize> {
        self.active_view
            .commit_row_index
            .get(target)
            .copied()
            .or_else(|| {
                self.active_view
                    .details
                    .iter()
                    .position(|detail| detail.full_sha.as_ref() == target.0)
            })
    }

    fn menu_context(&self, row_index: usize) -> Option<MenuContext> {
        let row = self.active_view.rows.get(row_index)?;
        let target = self.commit_id_for_row(row_index)?;
        let is_ancestor_of_head = if row.is_head {
            true
        } else {
            self.repo_path
                .as_ref()
                .and_then(|repo_path| kagi_git::Backend::open(repo_path).ok())
                .and_then(|repo| repo.is_ancestor_of_head(&target).ok())
                .unwrap_or(false)
        };

        Some(MenuContext {
            is_head: row.is_head,
            is_ancestor_of_head,
            is_merge: row.is_merge,
            dirty: self.active_view.is_dirty,
            detached: self.active_view.status_summary.is_detached,
            has_local_changes: self.active_view.is_dirty,
            refs_here: row.badges.clone(),
            local_branches: self
                .active_view
                .branches
                .iter()
                .map(|(n, _)| n.clone())
                .collect(),
        })
    }

    fn log_commit_menu(&self, row_index: usize) {
        if let Some(ctx) = self.menu_context(row_index) {
            let groups = context_menu::build_commit_menu(&ctx);
            context_menu::log_commit_menu(row_index, &groups);
        }
    }

    pub fn open_local_branch_menu(
        &mut self,
        branch_name: String,
        position: gpui::Point<gpui::Pixels>,
    ) {
        let target = match self.active_view.branch_targets.get(&branch_name) {
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
        klog!("branch-menu: open local {}", branch_name);
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
        klog!("branch-menu: open remote {}", display_name);
    }

    fn branch_menu_context(&self, state: &BranchMenuState) -> BranchMenuContext {
        let upstream = if matches!(state.kind, BranchKind::Local) {
            self.active_view.branch_upstream_info.get(&state.name)
        } else {
            None
        };
        let is_current = matches!(state.kind, BranchKind::Local)
            && self
                .active_view
                .branches
                .iter()
                .any(|(name, current)| name == &state.name && *current);
        let current_branch = self
            .active_view
            .branches
            .iter()
            .find_map(|(name, current)| current.then(|| name.clone()));
        let checked_out_worktree_path = if matches!(state.kind, BranchKind::Local) {
            self.active_view
                .worktrees
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
            dirty: self.active_view.status_summary.is_dirty,
            conflict_mode: if self.active_view.status_summary.conflict_count > 0 {
                BranchConflictMode::Conflicted
            } else {
                BranchConflictMode::None
            },
            protected: branch_menu::is_protected_branch(&state.name),
            checked_out_in_other_worktree: checked_out_worktree_path.is_some(),
            checked_out_worktree_path,
            merged_into_current: false,
            is_pushed: upstream.is_some(),
            detached_head: self.active_view.status_summary.is_detached,
            busy: self.busy_op.is_some(),
            current_branch,
            is_soloed: self
                .active_view
                .branch_solo
                .as_ref()
                .is_some_and(|solo| solo.name == state.name && solo.target == state.target),
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
        if self.editor_fs_prompt_modal().is_some() {
            self.confirm_editor_fs_prompt(cx);
        } else if self.editor_delete_confirm_modal().is_some() {
            self.confirm_editor_delete(cx);
        } else if self.editor_dirty_guard_modal().is_some() {
            self.confirm_editor_dirty_guard(cx);
        } else if self.discard_modal().is_some() {
            self.start_discard(cx);
        } else if self.conflict_continue_modal().is_some() {
            self.confirm_conflict_continue(cx);
        } else if self.history_modal().is_some() {
            self.confirm_history(cx);
        } else if self.amend_modal().is_some() {
            self.confirm_amend(cx);
        } else if self.undo_modal().is_some() {
            self.confirm_undo(cx);
        } else if self.cherry_pick_modal().is_some() {
            self.start_cherry_pick(cx);
        } else if self.revert_modal().is_some() {
            self.start_revert(cx);
        } else if self.stash_apply_modal().is_some() {
            self.confirm_stash_apply(cx);
        } else if self.stash_push_modal().is_some() {
            self.confirm_stash_push(cx);
        } else if self.unlock_worktree_modal().is_some() {
            self.confirm_unlock_worktree(cx);
        } else if self.create_worktree_modal().is_some() {
            self.start_create_worktree(cx);
        } else if self.create_branch_modal().is_some() {
            self.confirm_create_branch(cx);
        } else if self.rename_branch_modal().is_some() {
            self.start_rename_branch(cx);
        } else if self.set_upstream_modal().is_some() {
            self.start_set_upstream(cx);
        } else if self.tracking_checkout_modal().is_some() {
            self.start_tracking_checkout(cx);
        } else if self.switch_to_latest_modal().is_some() {
            self.start_switch_to_latest(cx);
        } else if self.merge_modal().is_some() {
            self.start_merge(cx);
        } else if self.branch_plan_modal().is_some() {
            self.start_branch_plan(cx);
        } else if self.branch_cleanup_modal().is_some() {
            self.confirm_branch_cleanup(cx);
        } else if self.delete_branch_modal().is_some() {
            self.confirm_delete_branch(cx);
        } else if self.pop_modal().is_some() {
            self.confirm_pop(cx);
        } else if self.push_modal().is_some() {
            self.confirm_push(cx);
        } else if self.pull_modal().is_some() {
            self.confirm_pull(cx);
        } else if self.plan_modal().is_some() {
            self.confirm_checkout(cx);
        } else if self.smart_commit.modal.is_some() {
            self.confirm_smart_consent(cx);
        } else if self
            .commit_panel
            .as_ref()
            .is_some_and(|e| e.read(cx).state.plan_modal.is_some())
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
        if self.editor_fs_prompt_modal().is_some() {
            self.cancel_editor_fs_prompt();
        } else if self.editor_delete_confirm_modal().is_some() {
            self.cancel_editor_delete_confirm();
        } else if self.editor_dirty_guard_modal().is_some() {
            self.cancel_editor_dirty_guard();
        } else if self.discard_modal().is_some() {
            self.cancel_discard_modal();
        } else if self.conflict_continue_modal().is_some() {
            self.cancel_conflict_continue();
        } else if self.history_modal().is_some() {
            self.clear_history_modal();
        } else if self.amend_modal().is_some() {
            self.cancel_amend_modal();
        } else if self.undo_modal().is_some() {
            self.cancel_undo_modal();
        } else if self.cherry_pick_modal().is_some() {
            self.cancel_cherry_pick_modal();
        } else if self.revert_modal().is_some() {
            self.cancel_revert_modal();
        } else if self.stash_apply_modal().is_some() {
            self.cancel_stash_apply_modal();
        } else if self.stash_push_modal().is_some() {
            self.cancel_stash_push_modal();
        } else if self.unlock_worktree_modal().is_some() {
            self.cancel_unlock_worktree_modal();
        } else if self.create_worktree_modal().is_some() {
            self.cancel_create_worktree_modal();
        } else if self.create_branch_modal().is_some() {
            self.cancel_create_branch_modal();
        } else if self.rename_branch_modal().is_some() {
            self.cancel_rename_branch_modal();
        } else if self.set_upstream_modal().is_some() {
            self.cancel_set_upstream_modal();
        } else if self.tracking_checkout_modal().is_some() {
            self.cancel_tracking_checkout_modal();
        } else if self.switch_to_latest_modal().is_some() {
            self.cancel_switch_to_latest_modal();
        } else if self.merge_modal().is_some() {
            self.cancel_merge_modal();
        } else if self.branch_plan_modal().is_some() {
            self.cancel_branch_plan_modal();
        } else if self.branch_cleanup_modal().is_some() {
            self.cancel_branch_cleanup_modal();
        } else if self.delete_branch_modal().is_some() {
            self.cancel_delete_branch_modal();
        } else if self.pop_modal().is_some() {
            self.cancel_pop_modal();
        } else if self.push_modal().is_some() {
            self.cancel_push_modal();
        } else if self.pull_modal().is_some() {
            self.cancel_pull_modal();
        } else if self.plan_modal().is_some() {
            self.cancel_modal();
        } else if self.smart_commit.modal.is_some() {
            self.cancel_smart_modal(cx);
        } else if self
            .commit_panel
            .as_ref()
            .is_some_and(|e| e.read(cx).state.plan_modal.is_some())
        {
            self.cancel_commit_plan_modal(cx);
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

    /// Move the commit selection up/down by `delta` rows (arrow keys).
    /// No selection yet → selects the first row. Idempotent at the ends.
    pub fn step_commit_selection(&mut self, delta: i64) {
        if self.active_view.rows.is_empty() {
            return;
        }
        let next = match self.selected {
            None => 0,
            Some(cur) => {
                let n = cur as i64 + delta;
                n.clamp(0, self.active_view.rows.len() as i64 - 1) as usize
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
        if self.sidebar.filter.is_none() {
            let input_entity = cx.new(|cx| InputState::new(window, cx).placeholder("filter…"));
            self.sidebar.filter = Some(input_entity);
        }
        // Focus the input after creation (or if already exists).
        if let Some(ref ent) = self.sidebar.filter {
            ent.update(cx, |state, cx| {
                state.focus(window, cx);
            });
        }
    }
}

// ──────────────────────────────────────────────────────────────
// Application entry point helper
// ──────────────────────────────────────────────────────────────

/// Open the GPUI window and start the event loop.
pub fn run_app(app_state: KagiApp) {
    // W4-TABS / ADR-0027: the watcher is armed from inside the window context
    // via `arm_watcher` (generation scheme), replacing the fixed spawn that
    // used to live here.  No pre-window watcher is created.

    let application = gpui_platform::application().with_assets(assets::KagiAssets);

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
            klog!("fonts: add_fonts failed (UI may fall back): {e}");
        } else {
            klog!("fonts: loaded Inter + JetBrains Mono");
        }

        // Persist the last plain-Windowed size so the next launch restores it
        // (open_main_window validates against a floor and falls back to the
        // default). A debounced background loop writes size changes every 2s
        // — a quit-time-only hook missed any exit that skips the platform
        // terminate path (Ctrl-C on a `cargo run`, kill, crash;
        // user-reported) — and the on_app_quit hook still captures the very
        // last value on a graceful quit. Skipped under KAGI_WINDOW so
        // headless test runs never clobber the user's remembered size.
        if std::env::var("KAGI_WINDOW").is_err() {
            let persist = |w: u32, h: u32, last: &mut (u32, u32)| {
                if w > 0 && h > 0 && (w, h) != *last {
                    settings::write_setting("window_size", Some(&format!("{w}x{h}")));
                    *last = (w, h);
                }
            };
            cx.background_spawn({
                let executor = cx.background_executor().clone();
                async move {
                    let mut last = (0u32, 0u32);
                    loop {
                        executor.timer(std::time::Duration::from_secs(2)).await;
                        let w = LAST_WIN_W.load(std::sync::atomic::Ordering::Relaxed);
                        let h = LAST_WIN_H.load(std::sync::atomic::Ordering::Relaxed);
                        persist(w, h, &mut last);
                    }
                }
            })
            .detach();
            cx.on_app_quit(|_cx| {
                let w = LAST_WIN_W.load(std::sync::atomic::Ordering::Relaxed);
                let h = LAST_WIN_H.load(std::sync::atomic::Ordering::Relaxed);
                async move {
                    if w > 0 && h > 0 {
                        settings::write_setting("window_size", Some(&format!("{w}x{h}")));
                    }
                }
            })
            .detach();
        }

        // T025: initialize gpui-component (registers key bindings, themes, etc.)
        gpui_component::init(cx);

        // W12-GCADOPT: gpui_component::init runs `sync_system_appearance`, which
        // seeds the gpui-component palette from the OS light/dark setting.  Push
        // kagi's active theme (already resolved by `theme::init_active` in main)
        // on top so adopted components (Input, Tooltip, Scrollbar, Checkbox…)
        // render in kagi's colours rather than the system default.
        theme::sync_gpui_component_theme(cx);

        // T-BP-002: register secondary-j (Cmd-J on macOS / Ctrl-J elsewhere) as
        // the toggle key for the bottom panel. context = None means the binding
        // fires regardless of focus context. GUI-CLICK: was `cmd-j`, which on
        // Linux is Super-J — so Ctrl-J never toggled the panel.
        cx.bind_keys([KeyBinding::new("secondary-j", ToggleBottomPanel, None)]);
        // T-UI-003: Esc closes the main diff view (no-op when main_diff is None).
        // Scoped `!Terminal` so Escape reaches a focused terminal (vim/less/etc.).
        cx.bind_keys([KeyBinding::new("escape", CloseMainDiff, Some("!Terminal"))]);
        // T-TERM-INTERACT-001 follow-up: Tab completion in the embedded
        // terminal. Deeper "Terminal" context outranks gpui_component Root's
        // "tab" → focus-cycling binding; handlers live on the terminal
        // wrapper div in render_bottom.rs and write \t / ESC[Z to the PTY.
        cx.bind_keys([
            KeyBinding::new("tab", TerminalSendTab, Some("Terminal")),
            KeyBinding::new("shift-tab", TerminalSendShiftTab, Some("Terminal")),
        ]);
        // Arrow keys step through files while the main diff is open
        // (no-ops otherwise; see main_diff_step). Scoped `!Terminal` so up/down
        // reach a focused terminal (shell history), and `!Input` so they reach
        // a focused text field / code editor: these bindings register AFTER
        // gpui_component::init, so at equal context depth they would shadow
        // Input's own MoveUp/MoveDown — the editor cursor stopped moving
        // vertically while left/right (unbound here) still worked
        // (user-reported).
        cx.bind_keys([
            KeyBinding::new("up", DiffPrevFile, Some("!Terminal && !Input")),
            KeyBinding::new("down", DiffNextFile, Some("!Terminal && !Input")),
        ]);
        // T-WS-EDITOR-002: Cmd-S saves the Editor Workspace's dirty buffer.
        // No context predicate — gpui-component 0.5.1's "Input" context binds
        // no `secondary-s` (verified: no cmd-s/ctrl-s/secondary-s binding in
        // its src/input/state.rs), so this fires even while the code editor
        // has focus. `save_editor_file` no-ops when there is nothing to save.
        cx.bind_keys([KeyBinding::new("secondary-s", SaveEditorFile, None)]);
        // ADR-0084: app-level Undo/Redo. Scoped `!Input && !Terminal` so a
        // focused text field (gpui-component Input, key_context "Input") keeps
        // OS-standard text undo (OsAction::Undo) and the terminal keeps its own
        // Cmd+Z — the app history move only fires elsewhere (e.g. commit graph).
        // gpui 0.2.2 only accepts `&&`/`||` (single `&` fails to parse).
        cx.bind_keys([
            KeyBinding::new(
                "secondary-z",
                commands::HistoryUndo,
                Some("!Input && !Terminal"),
            ),
            KeyBinding::new(
                "secondary-shift-z",
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
        // Last session's size when it's sane, else the preferred default —
        // either way clamped to the active display so the window never opens
        // off-screen on small / scaled displays (user-reported). The ideal
        // size is kept on big screens; only the upper bound is a fraction of
        // the display (so 4K/ultrawide don't get a needlessly huge window).
        const PREF_W: f32 = 1440.0;
        const PREF_H: f32 = 920.0;
        const MIN_W: f32 = 900.0;
        const MIN_H: f32 = 600.0;
        // Restore floor: only guards against corrupt/absurd remembered values
        // (a deliberately small window — half a laptop screen — must restore
        // as-is; 900x600 here forced such sizes back up, user-reported).
        const RESTORE_MIN_W: f32 = 400.0;
        const RESTORE_MIN_H: f32 = 300.0;
        let restored = settings::Settings::load()
            .window_size()
            .filter(|(w, h)| *w >= RESTORE_MIN_W && *h >= RESTORE_MIN_H);
        // A restored size keeps its own lower bound; only a fresh default
        // gets pushed up to the preferred minimum.
        let ((pref_w, pref_h), (min_w, min_h)) = match restored {
            Some(wh) => (wh, (RESTORE_MIN_W, RESTORE_MIN_H)),
            None => ((PREF_W, PREF_H), (MIN_W, MIN_H)),
        };
        match cx.primary_display() {
            Some(display) => {
                let ds = display.bounds().size;
                let max_w = f32::from(ds.width) * 0.92;
                let max_h = f32::from(ds.height) * 0.90;
                // clamp(low, high) with low never above high (tiny displays fill).
                (
                    pref_w.clamp(min_w.min(max_w), max_w),
                    pref_h.clamp(min_h.min(max_h), max_h),
                )
            }
            None => (pref_w, pref_h),
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
                // ADR-0110 Phase 5: the toast stack is a child entity so a
                // push/expire re-renders only the overlay. Created here because
                // the pure `KagiApp` constructors have no `cx`.
                app_state.toast_stack = Some(cx.new(|_| toast_stack::ToastStack::new()));
                // Likewise the op-log panel: seeded from the disk-loaded tail.
                let seed = std::mem::take(&mut app_state.op_log_seed);
                app_state.op_log = Some(cx.new(|_| oplog_panel::OpLogPanel::from_entries(seed)));
                app_state
            });
            if let Some(fh) = kagi.read(cx).root_focus.clone() {
                window.focus(&fh, cx);
            }

            // Settings appearance theme picker: the gpui-component `Select` is an
            // Entity that needs a `Window`, so it's built here rather than in
            // KagiApp::new. A `Confirm` subscription applies + persists the chosen
            // theme via set_theme (mirrors the old inline-dropdown click handler).
            let theme_select = cx.new(|cx| {
                settings_view::ThemeSelectState::new(
                    settings_view::theme_options(),
                    Some(settings_view::current_theme_index()),
                    window,
                    cx,
                )
            });
            kagi.update(cx, |app, cx| {
                cx.subscribe(
                    &theme_select,
                    |this,
                     _state,
                     event: &gpui_component::select::SelectEvent<
                        Vec<settings_view::ThemeOption>,
                    >,
                     cx| {
                        if let gpui_component::select::SelectEvent::Confirm(Some(slug)) = event {
                            this.set_theme(slug, cx);
                            cx.notify();
                        }
                    },
                )
                .detach();
                app.theme_select = Some(theme_select);
            });
            // Regression coverage for the Root::read crash: with
            // KAGI_COMMIT_PANEL=1, open the panel through the real
            // window-context path so the InputState + Input element
            // actually render during headless verification (the
            // pre-window env path in main.rs cannot create them).
            if std::env::var("KAGI_COMMIT_PANEL").as_deref() == Ok("1") {
                kagi.update(cx, |app, cx| app.open_commit_panel(window, cx));
            }

            // T-WS-EDITOR-001: KAGI_EDITOR_WS=1 opens the Editor workspace
            // through the real window-context path — `open_editor_workspace`
            // needs an entity `Context<KagiApp>`, which (like `KAGI_COMMIT_PANEL`
            // above) only exists once the window is open, not in main.rs's
            // pre-window env-hook path. The entity auto-selects the first
            // changed file once its working-tree load resolves, emitting the
            // `editor-ws: …` klog lines for headless verification.
            if std::env::var("KAGI_EDITOR_WS").as_deref() == Ok("1") {
                kagi.update(cx, |app, cx| app.open_editor_workspace(cx));
            }

            // T-WS-EDITOR-007: KAGI_EDITOR_WS_NEWFILE=<name> opens the Editor
            // workspace (if not already open) and creates `<name>` at the
            // repo root through the REAL confirm path (`open_editor_fs_prompt`
            // + `confirm_editor_fs_prompt`) — one headlessly-verifiable fs op
            // for this ticket's tree context-menu operations. `confirm_
            // editor_fs_prompt` only reads `modal.input` (not `input_state`,
            // which needs a render cycle to exist), so this is safe to fire
            // synchronously right here, before the first frame.
            if let Ok(name) = std::env::var("KAGI_EDITOR_WS_NEWFILE") {
                kagi.update(cx, |app, cx| {
                    if app.editor_workspace.is_none() {
                        app.open_editor_workspace(cx);
                    }
                    app.open_editor_fs_prompt(
                        EditorFsPromptKind::NewFile,
                        std::path::PathBuf::new(),
                        name.clone(),
                        cx,
                    );
                    app.confirm_editor_fs_prompt(cx);
                });
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
                    // T-PERF-RENDER-001 (ADR-0116 Wave 2): app-init commit point
                    // for the per-repo background I/O (conflict detect + reflog
                    // seed + auto-fetch ticker). The CLI-launch initial tab is
                    // built by hand (main.rs) and never goes through
                    // `switch_repo`, so render used to do this work on the first
                    // frame; arm it here off the UI thread instead.
                    app.ensure_startup_repo_io(cx);
                }
            });

            // ADR-0102: drain the single-instance accept channel on the UI
            // thread (opens forwarded repos as tabs + raises the window). No-op
            // when no listener was bound (bind failed, or headless test mode).
            kagi.update(cx, |app, cx| app.arm_single_instance_listener(cx));

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
    // Escape hatch for the Wayland client-side-decoration (CSD) click-offset
    // issue: on some native-Wayland + fractional-scaling setups, gpui's CSD
    // inset shifts the hit-test geometry so toolbar clicks land low (a stray
    // resize cursor appears and buttons don't fire). `KAGI_NO_CSD=1` requests
    // server-side decorations instead, which drops the inset and realigns input
    // (Kagi draws its own title bar, so the only loss is the drop shadow / round
    // corners). Documented in docs/linux-development.md. Default stays Client.
    if std::env::var("KAGI_NO_CSD").as_deref() == Ok("1") {
        return Some(gpui::WindowDecorations::Server);
    }
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
pub(crate) fn conflict_content_sig(path: &std::path::Path, result: &str, edit_mode: bool) -> u64 {
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
fn hunk_choice_slug(choice: &kagi_git::resolution::HunkChoice) -> &'static str {
    use kagi_git::resolution::HunkChoice::*;
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
    use super::{context_branch_name, draggable_branch_name};
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
            draggable_branch_name(&badge(BadgeKind::Branch, "🌲 feat-wt")),
            Some("feat-wt".to_string()),
            "worktree marker must not leak into the drag payload"
        );
        assert_eq!(
            context_branch_name(&badge(BadgeKind::Branch, "🌲 feat-wt")),
            Some("feat-wt".to_string())
        );
        assert_eq!(
            draggable_branch_name(&badge(BadgeKind::HeadBranch, "main ✓")),
            None
        );
        assert_eq!(
            draggable_branch_name(&badge(BadgeKind::Tag, "v0.1.0")),
            None
        );
    }

    #[test]
    fn context_menu_branch_names_include_head_and_remote_refs() {
        assert_eq!(
            context_branch_name(&badge(BadgeKind::HeadBranch, "main ✓")),
            Some("main".to_string())
        );
        assert_eq!(
            context_branch_name(&badge(BadgeKind::Branch, "feature")),
            Some("feature".to_string())
        );
        assert_eq!(
            context_branch_name(&badge(BadgeKind::Remote, "origin/feature")),
            Some("origin/feature".to_string())
        );
        assert_eq!(context_branch_name(&badge(BadgeKind::Tag, "v0.1.0")), None);
    }
}
