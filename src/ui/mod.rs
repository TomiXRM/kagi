//! UI module — T008: GPUI commit list / T009: commit graph lane / T010: commit selection + detail panel / T011: changed files list / T012: file diff viewer / T013: checkout plan modal + sidebar / T023: pane resize / T-BP-002: bottom panel open/close + resize / T-BP-007: terminal
//!
//! This module lives in the binary crate (`main.rs` does `mod ui;`).
//! It must not be added to `src/lib.rs` so that domain tests stay
//! independent of GPUI.

pub mod activity_view;
pub mod assets;
pub mod avatar;
pub mod avatar_fetch;
pub mod badges;
pub mod blocking_ops;
pub mod branch_menu;
pub mod button_style;
pub mod commands;
pub mod commit_list;
pub mod commit_panel;
mod commit_panel_render;
pub mod conflict_editor;
pub mod conflict_view;
pub mod context_menu;
pub mod detail_panel;
mod diff_cache;
pub mod diff_view;
pub mod diffstat_bar;
pub mod ecosystem;
pub mod file_history;
mod file_history_render;
mod file_menu;
pub mod file_tree;
pub mod graph_view;
pub mod i18n;
pub mod inspector;
pub mod menu_overlay;
mod modal_renderers;
mod modal_renderers_commit;
mod modal_renderers_create;
mod modal_renderers_destructive;
mod modal_renderers_misc;
mod modal_renderers_plan;
mod modal_renderers_stash;
pub mod modals;
mod operations;
pub mod oplog_panel;
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
pub mod tabs;
pub mod terminal;
pub mod theme;
pub mod toast_stack;
pub mod types;
pub mod view_models;
pub mod watcher;

pub use diff_view::*;
use i18n::Msg;
pub use modals::*;
pub use remote_browse::*;
pub(crate) use render_helpers::with_vertical_scrollbar;
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
        CheckoutSelected
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
        BadgeKind::Branch | BadgeKind::Remote => Some(badge.label.to_string()),
        BadgeKind::HeadBranch | BadgeKind::Tag => None,
    }
}

/// Extract a branch ref name from a graph badge for context-menu actions.
fn context_branch_name(badge: &commit_list::RefBadge) -> Option<String> {
    match badge.kind {
        BadgeKind::HeadBranch => Some(badge.label.trim_end_matches(" ✓").trim_end().to_string()),
        BadgeKind::Branch | BadgeKind::Remote => Some(badge.label.to_string()),
        BadgeKind::Tag => None,
    }
}

fn collect_history_commits(
    target: &CommitId,
    parents_by_id: &HashMap<CommitId, Vec<CommitId>>,
) -> HashSet<CommitId> {
    let mut visible = HashSet::new();
    let mut stack = vec![target.clone()];

    while let Some(id) = stack.pop() {
        if !visible.insert(id.clone()) {
            continue;
        }
        if let Some(parents) = parents_by_id.get(&id) {
            stack.extend(parents.iter().cloned());
        }
    }

    visible
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
const PANEL_DEFAULT: f32 = 360.0;

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
use detail_panel::{build_commit_details, CommitDetail};
use graph_view::graph_canvas;
use kagi_git::{
    oplog::{append_oplog, read_oplog_tail, OpLogEntry, OpOutcome},
    ops::{
        default_tracking_branch_name, validate_branch_rename, AmendMode, OperationPlan,
        StateSummary,
    },
    CommitId, DiffLineKind, FileDiff, FileDiffStat, FileStatus, Head, RemoteBranch, RepoSnapshot,
    Stash, Tag, UpstreamInfo, Worktree,
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
    pub main_diff: Option<MainDiffView>,
    /// Pending tree-sitter highlight result from a background task (ADR-0109).
    /// When the off-thread `highlight_diff_rows_send` completes, the result is
    /// stored here and applied to `main_diff` on the next `apply_pending_highlights`
    /// call (from render or a notify). `None` = no pending highlight.
    pub pending_diff_highlight: Option<(
        usize,
        usize,
        Vec<(usize, Vec<(std::ops::Range<usize>, gpui::HighlightStyle)>)>,
    )>,
    /// ADR-0026: read-only compare mode shown in the inspector changed-files area.
    /// Cleared on selection change or reload to avoid stale path/diff state.
    pub compare_view: Option<CompareView>,
    /// T-UI-003: Scroll handle for the "main-diff-list" uniform_list.
    pub main_diff_scroll_handle: UniformListScrollHandle,
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
    /// ADR-0119: Code Ecosystem / hot-spot view. `Some` while the full-screen
    /// read-only analysis view occupies the center+right area; `None` shows the
    /// normal body. Its own `Entity<EcosystemView>` owns the mining + ranking.
    pub ecosystem: Option<Entity<ecosystem::EcosystemView>>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchSolo {
    pub name: String,
    pub target: CommitId,
    pub visible_commits: HashSet<CommitId>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WipDiffStat {
    pub additions: usize,
    pub deletions: usize,
}

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
    }
}

/// Wall-clock now in Unix epoch seconds (right edge of the Activity windows).
fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// T-PERF-RENDER-001: the `Send` result of the read-only conflict-detection I/O
/// (`KagiApp::detect_conflict_payload`), applied to `KagiApp` on the UI thread by
/// `apply_conflict_detect`.  Splitting the I/O out of the state mutation lets the
/// same detection run either synchronously (`reload`) or off the UI thread
/// (`detect_conflict_mode_async`) without changing the emitted `[kagi]` lines.
enum ConflictDetectOutcome {
    /// `Backend::open` failed — leave `merge_commit_ready` untouched, clear mode.
    OpenFailed,
    /// No conflict session — clear Conflict Mode (emits `conflict-mode: cleared`
    /// only when a mode was previously open).
    Cleared,
    /// A merge with MERGE_HEAD but no unmerged entries — resolved, ready to commit.
    MergeResolvedReady,
    /// An active conflict/merge with files to resolve.  Boxed: the session +
    /// resolution buffer are large, and this variant is the rare case.
    Detected(Box<ConflictDetected>),
}

/// Payload of [`ConflictDetectOutcome::Detected`] — the assembled conflict state
/// the UI-thread apply moves into `self.conflict`.
struct ConflictDetected {
    session: kagi_git::conflicts::ConflictSession,
    buffer: kagi_git::resolution::ResolutionBuffer,
    current_branch: String,
    selected_file: Option<usize>,
    editing_file: Option<usize>,
    /// Selected content file whose hunks were materialized; set as the open
    /// editor file on apply.
    editing_path: Option<PathBuf>,
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
            pending_diff_highlight: None,
            compare_view: None,
            main_diff_scroll_handle: UniformListScrollHandle::new(),
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
            ecosystem: None,
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
            pending_diff_highlight: None,
            compare_view: None,
            main_diff_scroll_handle: UniformListScrollHandle::new(),
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
            ecosystem: None,
        }
    }

    /// Reload all display data from the repository at `repo_path`.
    ///
    /// Called after a successful checkout to update the commit list, header,
    /// branch list, and badges without restarting the application.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let _ = self.reload_checked(cx);
    }

    /// Pre-launch reload (headless `init_tab` / session restore). Runs before the
    /// gpui window exists, so there is no `Context` and no `Entity<KagiApp>` to
    /// hand a `ConflictView` — the conflict panel cannot be built here. Does the
    /// snapshot/view rebuild (so the commit list / header are populated and the
    /// `build_tab_view` `[kagi]` lines fire) but SKIPS conflict detection: the
    /// `conflict_detected_for` guard is left UNSET so the first cx-bearing detect
    /// at launch (`ensure_startup_repo_io` → `detect_conflict_mode_async`) builds
    /// the entity and emits the `conflict-mode:` line. ADR-0118 /
    /// T-ENTITY-CONFLICT-001.
    pub fn reload_prelaunch(&mut self) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let mut repo = match kagi_git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                klog!("reload: repo open error: {}", e);
                return;
            }
        };
        let snap = match repo.snapshot(self.commit_limit) {
            Ok(s) => s,
            Err(e) => {
                klog!("reload: snapshot error: {}", e);
                return;
            }
        };
        let wip_diffstat = Self::wip_diffstat_from_backend(&repo);
        let repo_name = repo_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| repo_path.display().to_string());
        let view = build_tab_view(&snap, &repo_name);
        self.selected = None;
        self.diff_caches.clear();
        self.wip_diffstat = Some(wip_diffstat);
        self.main_diff = None;
        self.compare_view = None;
        self.tab_cache.insert(repo_path.clone(), view.clone());
        self.apply_tab_view(view);
        self.seed_history_from_reflog(&repo);
        self.last_working_status = Some(snap.status.clone());
        // Conflict detection intentionally deferred to the launch-time
        // cx-bearing path (see the doc comment).
    }

    /// Like [`reload`] but reports failure. Returns `Err(msg)` when the repo
    /// can't be reopened or snapshotted (the current view is left intact), so a
    /// user-initiated refresh can surface the error instead of falsely reporting
    /// success. `Ok(())` also covers "no repo open" (nothing to refresh). The
    /// passive FS-watcher path uses [`reload_external`], which stays silent.
    pub fn reload_checked(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return Ok(()),
        };

        // Re-open and snapshot.
        let mut repo = match kagi_git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                klog!("reload: repo open error: {}", e);
                return Err(e.to_string());
            }
        };
        let snap = match repo.snapshot(self.commit_limit) {
            Ok(s) => s,
            Err(e) => {
                klog!("reload: snapshot error: {}", e);
                return Err(e.to_string());
            }
        };
        let wip_diffstat = Self::wip_diffstat_from_backend(&repo);

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
        self.diff_caches.clear();
        self.wip_diffstat = Some(wip_diffstat);
        self.main_diff = None;
        self.compare_view = None;
        // ADR-0089 / ADR-0117: drop any open File History view — its `Entity`
        // and any in-flight load tear down. `reload()` has no `cx` to re-spawn;
        // the `reload_external` path re-opens fresh when needed.
        self.file_history = None;
        // ADR-0119: reload() has no `cx` to re-spawn the async mine; drop the
        // Ecosystem view like File History (reopened fresh on demand).
        self.ecosystem = None;
        self.clear_plan_modal();
        self.clear_pull_modal();
        self.clear_undo_modal();
        self.clear_amend_modal();
        self.clear_pop_modal();
        self.clear_stash_drop_modal();
        self.clear_branch_plan_modal();
        self.clear_set_upstream_modal();
        self.clear_rename_branch_modal();
        self.clear_discard_modal();
        self.clear_create_branch_modal();
        self.clear_create_worktree_modal();
        self.modal_focus = None;
        self.clear_stash_push_modal();
        self.clear_stash_apply_modal();
        self.stash_push_focus = None;
        self.clear_cherry_pick_modal();
        self.clear_revert_modal();
        self.clear_conflict_continue_modal();
        // A merge that has been continued to the commit panel triggers its own
        // FS-watcher reload (staging writes the working tree + index). Preserve
        // the commit panel + merge message across that self-induced reload so the
        // user is not bounced out of the commit screen; the post-detect block
        // below confirms the merge is still pending (else it resets everything).
        let was_merge_commit_pending = self.conflict_merge_pending;
        self.commit_menu = None;
        self.file_menu = None;
        self.stash_menu = None;
        if !was_merge_commit_pending {
            // ADR-0068: a reload after commit / abort ends any continued-merge flow.
            self.conflict_merge_pending = false;
            // T025/T026: drop the commit-panel entity (state + inputs + template)
            // so it reflects fresh status after reload (ADR-0118: one entity).
            self.commit_panel_open = false;
            self.commit_panel = None;
        }
        // commit_scroll_handle is preserved so the existing Rc<RefCell<...>> reference
        // wired into the uniform_list continues to work after reload.
        // status_footer is intentionally preserved across reloads so the last
        // operation result remains visible after the commit list refreshes.
        // sidebar_width / panel_width are also preserved so the user's resize
        // is not lost on checkout/reload (T023).
        // T-BP-004: the op_log entity (entries + expanded row + scroll handle)
        // persists across reloads/tab switches so the Operation Log keeps its
        // contents and UI state.
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
        // Mode.  Force re-detection by invalidating the run-once guard.
        self.conflict_detected_for = None;
        self.detect_conflict_mode(cx);

        // Re-resolve the continued-merge flow after detection.
        if was_merge_commit_pending {
            if self.merge_commit_ready {
                // Still a resolved merge awaiting its commit: keep the commit
                // panel up (refresh the staged list from the index) and keep the
                // pre-filled / user-edited merge message entity untouched.
                // ADR-0118: update the entity's `state` IN PLACE so its inputs /
                // template mode (the pre-filled merge message) survive the reload.
                let mut panel = CommitPanelState::from_repo(&repo_path);
                if let Some(entity) = self.commit_panel.clone() {
                    entity.update(cx, |v, _| {
                        panel.tree_view = v.state.tree_view;
                        v.state = panel;
                    });
                } else {
                    let weak_app = cx.weak_entity();
                    let entity =
                        cx.new(|_| CommitPanelView::new(panel, weak_app, repo_path.clone()));
                    self.commit_panel = Some(entity);
                }
                self.commit_panel_open = true;
                self.conflict = None;
                self.conflict_merge_pending = true;
            } else {
                // The merge commit was created (MERGE_HEAD gone) or aborted — end
                // the flow and drop the commit-panel entity.
                self.conflict_merge_pending = false;
                self.commit_panel_open = false;
                self.commit_panel = None;
            }
        }
        Ok(())
    }

    /// Grow the commit graph by [`COMMIT_PAGE_STEP`] and re-snapshot.
    ///
    /// Triggered by the "load more" row at the bottom of the commit list, which
    /// only appears once the graph holds at least `commit_limit` commits (i.e.
    /// the walk may have been truncated). Unlike [`reload`], this is a
    /// view-only refresh: it rebuilds `active_view` (and the tab cache) at the
    /// new limit but leaves selection, scroll position, open panels and modals
    /// untouched. Existing rows keep their indices because the additional
    /// commits are older and append at the bottom of the topological order.
    pub fn load_more_commits(&mut self, cx: &mut Context<Self>) {
        let repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        self.commit_limit = self.commit_limit.saturating_add(COMMIT_PAGE_STEP);

        let mut repo = match kagi_git::Backend::open(&repo_path) {
            Ok(r) => r,
            Err(e) => {
                klog!("load more: repo open error: {}", e);
                return;
            }
        };
        let snap = match repo.snapshot(self.commit_limit) {
            Ok(s) => s,
            Err(e) => {
                klog!("load more: snapshot error: {}", e);
                return;
            }
        };
        let repo_name = repo_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| repo_path.display().to_string());

        let view = build_tab_view(&snap, &repo_name);
        self.tab_cache.insert(repo_path.clone(), view.clone());
        self.apply_tab_view(view);
        klog!(
            "load more: limit={} rows={}",
            self.commit_limit,
            self.active_view.rows.len()
        );
        cx.notify();
    }

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
            .and_then(|idx| self.active_view.details.get(idx))
            .map(|detail| CommitId(detail.full_sha.to_string()));

        // ADR-0104 / performance: an external git event (HEAD/refs change from
        // a terminal, sibling worktree, or auto-fetch) is NOT user-initiated,
        // so freezing the UI frame for a full repo snapshot (topological walk,
        // full working-tree status scan, ahead/behind for every branch) is the
        // worst kind of jank — the user didn't ask for anything. Move the
        // heavy git2 work (open + snapshot + wip diffstat) onto a background
        // thread, then build the view data and apply it on the UI thread.
        // (The synchronous `reload()` is still used by user-initiated paths
        // where a short, expected wait is acceptable.)
        let bg_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        // Capture the switch generation so the background result is discarded
        // if the user switched tabs while the snapshot was in flight — without
        // this guard the snapshot would overwrite the NEW tab's freshly-loaded
        // view with the OLD tab's data (cross-review N4).
        let gen_at_spawn = self.switch_generation;
        let commit_limit = self.commit_limit;
        let task = cx.background_spawn(async move {
            let mut backend = kagi_git::Backend::open(&bg_path).ok()?;
            let snap = backend.snapshot(commit_limit).ok()?;
            let wip = KagiApp::wip_diffstat_from_backend(&backend);
            // RepoSnapshot is pure domain (Send); build_tab_view constructs
            // SharedString-bearing TabViewState, so we return the raw pieces
            // and build the view on the UI thread.
            let repo_name = bg_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| bg_path.display().to_string());
            Some((snap, wip, repo_name))
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                // Generation guard (cross-review N4): if the user switched tabs
                // while the snapshot was in flight, drop the result — applying
                // it would clobber the now-current tab's view.
                if app.switch_generation != gen_at_spawn {
                    return;
                }
                let Some((snap, wip, repo_name)) = result else {
                    // Open or snapshot failed — log and bail without nuking
                    // the existing view (better to show stale data than none).
                    klog!("reload_external: snapshot failed (non-fatal)");
                    app.status_footer = FooterStatus::Idle(SharedString::from(
                        "[kagi] refresh skipped (snapshot failed)",
                    ));
                    cx.notify();
                    return;
                };
                // Build the view data on the UI thread (cheap; heavy git2 work
                // already done in the background) and apply it.
                let view = build_tab_view(&snap, &repo_name);
                app.apply_tab_view(view);
                app.diff_caches.clear();
                app.wip_diffstat = Some(wip);
                app.main_diff = None;
                app.compare_view = None;
                app.file_history = None;

                // Attempt to restore selection by CommitId.
                app.selected = None;
                if let Some(ref cid) = prev_commit_id {
                    if let Some(&new_idx) = app.active_view.commit_row_index.get(cid) {
                        app.selected = Some(new_idx);
                    }
                    // If the commit is no longer present, selected stays None.
                }

                // Emit the required log line and update the footer.
                klog!("refreshed (external change)");
                app.status_footer =
                    FooterStatus::Idle(SharedString::from("[kagi] refreshed (external change)"));
                cx.notify();
            });
        })
        .detach();
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
            let backend = kagi_git::Backend::open(&bg_path).ok()?;
            let status = backend.working_tree_status().ok()?;
            let wip_diffstat = KagiApp::wip_diffstat_from_backend(&backend);
            Some((status, wip_diffstat))
        });
        cx.spawn(async move |this, acx| {
            let refreshed = task.await;
            let _ = this.update(acx, |app, cx| {
                let Some((new_status, wip_diffstat)) = refreshed else {
                    return;
                };
                if app.last_working_status.as_ref() == Some(&new_status) {
                    if app.wip_diffstat != Some(wip_diffstat) {
                        app.wip_diffstat = Some(wip_diffstat);
                        cx.notify();
                    }
                    return; // working-tree status unchanged → nothing to do.
                }
                klog!("watcher: working-tree changed — refreshing WIP");
                // In-place WIP/status update — do NOT full-reload (that re-snapshots
                // the graph and closes the commit panel). Branch / ahead-behind are
                // unchanged by a working-tree edit, so only the dirty/count fields
                // and the commit panel's file lists need refreshing.
                app.active_view.status_summary.is_dirty = new_status.is_dirty();
                app.active_view.status_summary.staged = new_status.staged.len();
                app.active_view.status_summary.unstaged = new_status.unstaged.len();
                app.active_view.status_summary.untracked = new_status.untracked.len();
                app.active_view.status_summary.conflict_count = new_status.conflicted.len();
                app.active_view.is_dirty = new_status.is_dirty();
                app.last_working_status = Some(new_status);
                app.wip_diffstat = Some(wip_diffstat);
                // Refresh the open commit panel's lists in place (keeps it open).
                // ADR-0118 (correction #6c): update the entity, never rebuild via
                // a parent render read.
                if let (Some(entity), Some(rp)) = (app.commit_panel.clone(), app.repo_path.clone())
                {
                    entity.update(cx, |v, _| v.state.reload_status(&rp));
                }
                cx.notify();
            });
        })
        .detach();
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

    /// Read-only conflict detection: opens the repo, detects the session, builds
    /// the resolution buffer, recomputes per-file status, auto-selects a file, and
    /// materializes zdiff3 markers for the selected content file.  This is the
    /// **entire I/O half** of conflict detection — pure inputs/outputs (no `self`)
    /// so it runs either synchronously (`detect_conflict_mode`) or on a background
    /// thread (`detect_conflict_mode_async`).  `current_branch` and the `prev_*`
    /// preservation indices are captured by the caller from `self`.
    fn detect_conflict_payload(
        repo_path: &Path,
        prev_selected: Option<usize>,
        prev_editing_file: Option<usize>,
        current_branch: String,
    ) -> ConflictDetectOutcome {
        let repo = match kagi_git::Backend::open(repo_path) {
            Ok(r) => r,
            Err(_) => return ConflictDetectOutcome::OpenFailed,
        };

        let session = match repo.detect_conflict_session() {
            Some(s) => s,
            None => return ConflictDetectOutcome::Cleared,
        };

        // A merge with MERGE_HEAD present but no remaining unmerged index entries
        // is not a conflict to resolve — it is a resolved merge ready to commit.
        if matches!(session.op, kagi_git::ConflictOp::Merge { .. }) && session.files.is_empty() {
            return ConflictDetectOutcome::MergeResolvedReady;
        }

        // Build / reload the resolution buffer.  A previously-autosaved buffer
        // (e.g. from before a restart) is preferred so partial work survives;
        // otherwise materialize a fresh buffer from the index conflicts.
        let mut buffer = kagi_git::ResolutionBuffer::load(repo_path)
            .or_else(|| repo.resolution_buffer_from_repo().ok())
            .unwrap_or_else(|| kagi_git::ResolutionBuffer::new(repo_path));

        // Recompute per-file status from the buffer (detection seeds Unresolved).
        let mut session = session;
        let residue = buffer.files_with_marker_residue();
        for f in &mut session.files {
            if buffer.has_resolution(&f.path) {
                f.status = if residue.contains(&f.path) {
                    kagi_git::ConflictStatus::NeedsReview
                } else {
                    kagi_git::ConflictStatus::Resolved
                };
            } else {
                f.status = kagi_git::ConflictStatus::Unresolved;
            }
        }

        // Preserve the previously-selected file across re-detections; otherwise
        // open the first unresolved file (KDiff3-style "land on work to do").
        let selected_file = prev_selected
            .filter(|&i| i < session.files.len())
            .or_else(|| {
                session
                    .files
                    .iter()
                    .position(|f| f.status == kagi_git::ConflictStatus::Unresolved)
            })
            .or_else(|| (!session.files.is_empty()).then_some(0));

        // W33: preserve the dashboard editing-file index across re-detection.
        let editing_file = prev_editing_file.filter(|&i| i < session.files.len());

        // The center A/B editor renders from the hunk model, which needs the repo
        // to materialize zdiff3 markers.  With auto-selection the user never
        // clicked, so build the hunk model for the selected content file here.
        let mut editing_path = None;
        if let Some(idx) = selected_file {
            if let Some(f) = session.files.get(idx) {
                if f.kind == kagi_git::ConflictKind::Content {
                    let path = f.path.clone();
                    if let Some(markers) = repo.materialized_markers(&buffer, &path) {
                        buffer.ensure_hunks(&path, &markers);
                    }
                    editing_path = Some(path);
                }
            }
        }

        ConflictDetectOutcome::Detected(Box::new(ConflictDetected {
            session,
            buffer,
            current_branch,
            selected_file,
            editing_file,
            editing_path,
        }))
    }

    /// Foreground half of conflict detection: apply a [`ConflictDetectOutcome`]
    /// computed by [`detect_conflict_payload`] to `self`, emitting the same
    /// `[kagi]` contract lines in the same order as the original synchronous
    /// implementation. ADR-0118: this is the single point that builds / updates /
    /// drops the `Entity<ConflictView>` — `Detected` updates an existing entity in
    /// place (preserving its splits / editor inputs / before-text) or creates a
    /// new one; `Cleared` / `MergeResolvedReady` / `OpenFailed` drop it. The
    /// "was a conflict open?" (Cleared) and editor-close (Detected) checks read
    /// the entity here because they must reflect the current UI state at apply
    /// time. Needs `cx` (entity create / read / update).
    fn apply_conflict_detect(&mut self, outcome: ConflictDetectOutcome, cx: &mut Context<Self>) {
        match outcome {
            ConflictDetectOutcome::OpenFailed => {
                // Mirrors the original early-return on `Backend::open` failure,
                // which happened before `merge_commit_ready` was reset — so that
                // flag is intentionally left untouched here.
                self.conflict = None;
            }
            ConflictDetectOutcome::Cleared => {
                self.merge_commit_ready = false;
                if self
                    .conflict
                    .as_ref()
                    .is_some_and(|e| e.read(cx).mode.is_some())
                {
                    klog!("conflict-mode: cleared");
                }
                // Drop the entity (clears mode + editing + splits + before-text;
                // the accepted Stage-1 reset delta on re-entry).
                self.conflict = None;
            }
            ConflictDetectOutcome::MergeResolvedReady => {
                self.merge_commit_ready = false;
                klog!("conflict-mode: merge resolved — ready to commit");
                self.merge_commit_ready = true;
                self.conflict = None;
            }
            ConflictDetectOutcome::Detected(detected) => {
                let ConflictDetected {
                    session,
                    buffer,
                    current_branch,
                    selected_file,
                    editing_file,
                    editing_path,
                } = *detected;
                self.merge_commit_ready = false;
                eprintln!(
                    "[kagi] conflict-mode: {} {} file(s)",
                    session.op.slug(),
                    session.files.len()
                );

                let mode = conflict_view::ConflictMode {
                    session,
                    buffer,
                    current_branch,
                    selected_file,
                    editing_file,
                    abort_armed: false,
                };
                let files = mode.session.files.clone();

                match self.conflict.clone() {
                    // Re-detect: update the existing entity in place so its splits
                    // / editor inputs / before-text / scroll survive the reload.
                    Some(entity) => {
                        entity.update(cx, |v, _| {
                            // W32: close the editor if the edited file is no longer
                            // conflicted (reads the entity's current `editing`).
                            if let Some(editing) = v.editing.clone() {
                                if !files.iter().any(|f| f.path == editing) {
                                    v.editing = None;
                                }
                            }
                            v.mode = Some(mode);
                            if let Some(path) = editing_path {
                                v.editing = Some(path);
                            }
                        });
                    }
                    // Fresh conflict: build the entity, capturing the repo path +
                    // a weak back-ref for its deferred parent callbacks.
                    None => {
                        let weak_app = cx.weak_entity();
                        let repo_path = self.repo_path.clone().unwrap_or_default();
                        let entity = cx.new(|_| {
                            let mut v = conflict_view::ConflictView::new(weak_app, repo_path);
                            v.mode = Some(mode);
                            v.editing = editing_path;
                            v
                        });
                        self.conflict = Some(entity);
                    }
                }
            }
        }
    }

    // ────────────────────────────────────────────────────────────
    // W32-CONFLICT-EDITOR: hunk-level editor open/close + hunk dispatch
    // ────────────────────────────────────────────────────────────

    /// W11-AVATAR (ADR-0037): start GitHub avatar resolution for the current
    /// repo, at most once per repository path.
    ///
    /// Resolution runs entirely on a background thread (`cx.background_spawn`):
    /// it determines the GitHub `(owner, repo)` from the repo's remotes, then
    /// resolves each distinct author email to an avatar image (noreply parse →
    /// Commits API batch → disk/network fetch).  When it completes the resolved
    /// images are merged into `self.avatars.images` on the main thread and a
    /// `cx.notify()` repaints rows/inspector with real avatars.
    ///
    /// No-op for non-GitHub repos, `KAGI_OFFLINE=1`, or a repo already started.
    /// The required startup log line is emitted exactly once per repo.
    fn ensure_avatars(&mut self, cx: &mut Context<Self>) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };

        // Run at most once per repository path.
        if self.avatars.fetch_for.as_deref() == Some(repo_path.as_path()) {
            return;
        }
        self.avatars.fetch_for = Some(repo_path.clone());

        // Distinct author emails across the loaded commit rows.
        let mut seen: HashSet<String> = HashSet::new();
        let mut emails: Vec<String> = Vec::new();
        for row in &self.active_view.rows {
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
                    app.avatars.images.insert(email, img);
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
        if let Some(m) = self.create_branch_modal_mut() {
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

        // ── Remote SSH connect form (host / port / identity) ─
        if let Some(m) = self.remote_browse_modal.as_mut() {
            if m.host_state.is_none() {
                let st = cx.new(|cx| InputState::new(window, cx).placeholder("user@host"));
                st.update(cx, |s, cx| s.focus(window, cx));
                m.host_state = Some(st);
            }
            if m.port_state.is_none() {
                m.port_state =
                    Some(cx.new(|cx| InputState::new(window, cx).placeholder("22 (optional)")));
            }
            if m.identity_state.is_none() {
                m.identity_state = Some(cx.new(|cx| {
                    InputState::new(window, cx).placeholder("~/.ssh/id_ed25519 (optional)")
                }));
            }
            let hv = m
                .host_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            if hv != m.host_input {
                m.host_input = hv;
                m.error = None;
            }
            let pv = m
                .port_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            if pv != m.port_input {
                m.port_input = pv;
            }
            let iv = m
                .identity_state
                .as_ref()
                .map(|st| st.read(cx).value().to_string())
                .unwrap_or_default();
            if iv != m.identity_input {
                m.identity_input = iv;
            }
        }

        // ── Create-worktree (branch + path fields) ──────────
        // Auto-path: while the user has not touched the path field, it
        // follows the branch name (same behaviour as before).
        let mut set_path: Option<String> = None;
        if let Some(m) = self.create_worktree_modal_mut() {
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
            if let Some(m) = self.create_worktree_modal_mut() {
                m.path_input = auto.clone();
                if let Some(st) = m.path_state.clone() {
                    st.update(cx, |s, cx| s.set_value(auto, window, cx));
                }
            }
            self.schedule_modal_replan(cx);
        }

        // ── Commit-message draft autosave (T-COMMIT-007 / T-COMMIT-009) ──
        // ADR-0118 (Phase 5.2) / T-ENTITY-COMMITPANEL-001 (correction #1): moved
        // ONTO the `CommitPanelView` entity (`sync_inputs`), so the parent never
        // reads the child's commit input each frame (the re-entrancy-in-render
        // surface ADR-0118 forbids).

        // ── Stash push (message) ────────────────────────────
        if let Some(m) = self.stash_push_modal_mut() {
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

        if let Some(m) = self.set_upstream_modal_mut() {
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

        if let Some(m) = self.rename_branch_modal_mut() {
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
        // ADR-0118: the sync logic moved into the entity (it owns `editor_inputs`
        // + `editing` + `mode`). Drive it via `update_in` (it needs a `Window` to
        // create `InputState`). Safe here: this runs on the parent render-sync
        // path, NOT a leased `ConflictView` listener.
        if let Some(entity) = self.conflict.clone() {
            entity.update(cx, |v, cx| v.sync_editor_inputs(window, cx));
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

    /// T-UI-003: Open the diff for the file at `file_index` in the currently
    /// selected commit in the full-width main pane.
    ///
    /// Emits the legacy `[kagi] diff:` log (headless compat) plus
    /// `[kagi] main-diff: open <path> rows=N`.
    /// No-op if no commit is selected.
    /// Step the open main diff to the previous/next file (arrow keys).
    /// No-op when no diff is open or already at the list edge.
    pub fn main_diff_step(&mut self, delta: i64, cx: &mut Context<Self>) {
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
                    .diff_caches
                    .changed_files
                    .get(&row_index)
                    .and_then(|o| o.as_ref())
                    .map(|v| v.len())
                    .unwrap_or(0);
                if len == 0 {
                    return;
                }
                let next = (file_index as i64 + delta).clamp(0, len as i64 - 1) as usize;
                if next != file_index {
                    self.open_main_diff_commit(next, cx);
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
                    Some(e) => {
                        let p = &e.read(cx).state;
                        (
                            p.unstaged.iter().position(|f| f.path == path),
                            p.unstaged.len(),
                        )
                    }
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
                    self.open_main_diff_wip(
                        commit_panel::CommitPanelFileRef::Unstaged { index: next },
                        cx,
                    );
                }
            }
            MainDiffSource::Staged { path } => {
                let (cur, len) = match self.commit_panel.as_ref() {
                    Some(e) => {
                        let p = &e.read(cx).state;
                        (p.staged.iter().position(|f| f.path == path), p.staged.len())
                    }
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
                    self.open_main_diff_wip(
                        commit_panel::CommitPanelFileRef::Staged { index: next },
                        cx,
                    );
                }
            }
        }
    }

    pub fn open_main_diff_inspector_file(&mut self, file_index: usize, cx: &mut Context<Self>) {
        if self.remote_view.is_some() {
            // Remote read-only view (ADR-0089 Phase 2c): the file diff is an SSH
            // round-trip, loaded off-thread.
            self.open_remote_main_diff(file_index, cx);
        } else if self.compare_view.is_some() {
            self.open_main_diff_compare(file_index);
        } else {
            self.open_main_diff_commit(file_index, cx);
        }
    }

    // ──────────────────────────────────────────────────────────────
    // ADR-0089: File History view
    // ──────────────────────────────────────────────────────────────

    /// Open the File History view for `rel_path` (repo-relative). ADR-0117: this
    /// builds the `Entity<FileHistoryView>` (in Loading state) and kicks off its
    /// own async history load (read-only — no `busy_op` gate). The entity owns
    /// the load + diff logic (it holds `repo_path`); `KagiApp` only constructs it
    /// and stores the handle. Callers: the inspector / main-diff "History" entry
    /// points.
    pub fn open_file_history(
        &mut self,
        rel_path: PathBuf,
        origin: Option<CommitId>,
        cx: &mut Context<Self>,
    ) {
        let Some(repo_path) = self.repo_path.clone() else {
            return;
        };
        klog!("file-history: open {}", rel_path.display());

        let branch = SharedString::from(self.active_view.status_summary.branch.clone());
        let state = file_history::FileHistoryState {
            rel_path,
            branch,
            follow_renames: true,
            history: None,
            error: None,
            selected: 0,
            diff: None,
            diff_scroll: UniformListScrollHandle::new(),
            split: 0.25,
            generation: 0,
            diff_req: 0,
        };

        // The entity holds a weak back-ref (for close / jump-to-commit), a shared
        // clone of the geom cell (the divider-drag reads it), and `panel_width`.
        let weak = cx.weak_entity();
        let geom = self.file_history_geom.clone();
        let panel_width = self.panel_width;
        let view = cx
            .new(|_| file_history::FileHistoryView::new(state, weak, geom, panel_width, repo_path));
        // Kick off the initial load on the (now fully-constructed) entity.
        view.update(cx, |v, cx| v.start_load(origin, true, cx));
        self.file_history = Some(view);
    }

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
        if let Some(view) = self.compare_view.as_ref() {
            if let Some(f) = view.files.get(file_index) {
                let path = f.path.clone();
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
        let Some(view) = self.main_diff.as_ref() else {
            return;
        };
        let (path, origin) = match &view.source {
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
                let path = self
                    .compare_view
                    .as_ref()
                    .and_then(|v| v.files.get(*file_index))
                    .map(|f| f.path.clone());
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

    /// Open the first changed file's diff in the main pane (headless path).
    /// Calls the synchronous highlight variant since headless has no cx and is
    /// test-only (no UI latency concern).
    pub fn open_main_diff_commit_headless(&mut self, file_index: usize) {
        // Delegate to the shared open path with a dummy sync-highlight by
        // calling set_commit_main_diff_sync directly after acquiring the diff.
        self.open_main_diff_commit_inner(file_index, None);
    }

    /// Open the main diff with async highlight (UI path).
    pub fn open_main_diff_commit(&mut self, file_index: usize, cx: &mut Context<Self>) {
        self.open_main_diff_commit_inner(file_index, Some(cx));
    }

    fn open_main_diff_commit_inner(
        &mut self,
        file_index: usize,
        mut cx: Option<&mut Context<Self>>,
    ) {
        use kagi_git::CommitId;

        let selected = match self.selected {
            Some(s) => s,
            None => return,
        };
        let _repo_path = match self.repo_path.as_ref() {
            Some(p) => p.clone(),
            None => return,
        };
        let detail = match self.active_view.details.get(selected) {
            Some(d) => d,
            None => return,
        };
        let files = match self
            .diff_caches
            .changed_files
            .get(&selected)
            .and_then(|v| v.as_ref())
        {
            Some(f) => f,
            None => return,
        };
        let file_status = match files.get(file_index) {
            Some(f) => f,
            None => return,
        };

        let id = CommitId(detail.full_sha.as_ref().to_string());
        let path = file_status.path.clone();

        // T-REARCH-031: per-(row, file) content cache. Clicking between two
        // commits to compare the same file previously recomputed the full git2
        // tree-diff + hunk extraction on every toggle. Hit the cache first.
        if let Some(cached) = self
            .diff_caches
            .file_content
            .get(&(selected, file_index))
            .cloned()
        {
            self.set_commit_main_diff(&cached, &path, selected, file_index, cx.as_deref_mut());
            return;
        }

        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let Some(session) = self.repo_session.as_ref() else {
            return;
        };
        let repo = session.backend();

        match repo.commit_file_diff(&id, &path) {
            Ok(file_diff) => {
                let arc = std::sync::Arc::new(file_diff);
                self.diff_caches
                    .file_content
                    .insert((selected, file_index), arc.clone());
                self.set_commit_main_diff(&arc, &path, selected, file_index, cx.as_deref_mut());
            }
            Err(e) => {
                klog!("diff error: {}", e);
            }
        }
    }

    /// Build the full-width [`MainDiffView`] for a commit's file diff. Shared by
    /// the local (`git2`) path and the remote (SSH) path so both render
    /// identically.
    fn set_commit_main_diff(
        &mut self,
        file_diff: &FileDiff,
        path: &std::path::Path,
        selected: usize,
        file_index: usize,
        cx: Option<&mut Context<Self>>,
    ) {
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
        eprintln!(
            "[kagi] diff: {} hunks={} (+{} -{})",
            path.display(),
            file_diff.hunks.len(),
            added,
            removed,
        );

        let fdv = FileDiffView::from_file_diff(file_diff, file_index);
        let stats = SharedString::from(format!("+{} \u{2212}{}", added, removed));
        let title = fdv.file_name.clone();

        match cx {
            // UI path (cx present): render text-first, highlight off-thread.
            Some(cx) => {
                let rows = fdv.rows;
                let row_count = rows.len();
                let path_for_hl = path.to_path_buf();
                let rows_snapshot = rows.clone();
                let selected_for_hl = selected;
                let file_index_for_hl = file_index;

                self.main_diff = Some(MainDiffView {
                    title,
                    stats,
                    rows,
                    source: MainDiffSource::Commit {
                        row_index: selected,
                        file_index,
                    },
                });

                // Spawn the highlight off-thread; store the result for swap-in.
                cx.spawn(async move |this, acx| {
                    let (hl_lang, highlights) =
                        diff_view::highlight_diff_rows_send(&rows_snapshot, &path_for_hl);
                    let _ = this.update(acx, |app, cx| {
                        app.pending_diff_highlight =
                            Some((selected_for_hl, file_index_for_hl, highlights));
                        eprintln!(
                            "[kagi] main-diff: highlight ready {} rows={} lang={}",
                            path_for_hl.display(),
                            row_count,
                            hl_lang
                        );
                        cx.notify();
                    });
                })
                .detach();
            }
            // Headless path (no cx): synchronous highlight (test-only, no UI).
            None => {
                let mut rows = fdv.rows;
                let hl_lang = diff_view::highlight_diff_rows(&mut rows, path);
                eprintln!(
                    "[kagi] main-diff: open {} rows={} highlight={}",
                    path.display(),
                    rows.len(),
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
        }
    }

    /// Apply a pending background highlight result to `main_diff` if it still
    /// matches the current view (same row/file index). Called from render so
    /// the swap happens on the next frame after the background task completes.
    /// Stale results (view changed) are discarded.
    fn apply_pending_highlights(&mut self) {
        let Some((row, file, highlights)) = self.pending_diff_highlight.take() else {
            return;
        };
        let Some(view) = self.main_diff.as_mut() else {
            return;
        };
        // Only apply if the view hasn't changed since the highlight was requested.
        match view.source {
            MainDiffSource::Commit {
                row_index,
                file_index,
            } if row_index == row && file_index == file => {}
            _ => return,
        }
        for (row_i, row_highlights) in highlights {
            if let Some(DiffRow::Line { highlights: hl, .. }) = view.rows.get_mut(row_i) {
                *hl = row_highlights;
            }
        }
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

    /// Open the full-width diff for a clicked file of the selected remote commit,
    /// loading the unified diff over SSH off the UI thread (ADR-0089 Phase 2c).
    fn open_remote_main_diff(&mut self, file_index: usize, cx: &mut Context<Self>) {
        let (host, root) = match &self.remote_view {
            Some(v) => (v.host.clone(), v.root.clone()),
            None => return,
        };
        let selected = match self.selected {
            Some(s) => s,
            None => return,
        };
        let sha = match self.active_view.details.get(selected) {
            Some(d) => d.full_sha.as_ref().to_string(),
            None => return,
        };
        let path = match self
            .diff_caches
            .changed_files
            .get(&selected)
            .and_then(|v| v.as_ref())
            .and_then(|files| files.get(file_index))
        {
            Some(f) => f.path.clone(),
            None => return,
        };
        let path_str = path.to_string_lossy().into_owned();

        let task = cx.background_spawn(async move {
            kagi::remote::remote_commit_file_diff(&host, &root, &sha, &path_str)
                .map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, acx| {
            let result = task.await;
            let _ = this.update(acx, |app, cx| {
                match result {
                    Ok(file_diff) => {
                        app.set_commit_main_diff(&file_diff, &path, selected, file_index, Some(cx))
                    }
                    Err(e) => klog!("remote diff error: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn open_main_diff_compare(&mut self, file_index: usize) {
        let _repo_path = match self.repo_path.as_ref() {
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

        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let Some(session) = self.repo_session.as_ref() else {
            return;
        };
        let repo = session.backend();

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
                klog!("compare diff error: {}", e);
            }
        }
    }

    /// T-UI-003: Open the diff for a Commit Panel file in the full-width main pane.
    pub fn open_main_diff_wip(
        &mut self,
        file_ref: commit_panel::CommitPanelFileRef,
        cx: &mut Context<Self>,
    ) {
        let _repo_path = match self.repo_path.clone() {
            Some(p) => p,
            None => return,
        };
        let entity = match self.commit_panel.as_ref() {
            Some(e) => e.clone(),
            None => return,
        };

        let (is_staged, path) = {
            let panel = &entity.read(cx).state;
            match &file_ref {
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
            }
        };

        // ADR-0107: use the per-tab RepoSession instead of re-opening.
        let Some(session) = self.repo_session.as_ref() else {
            return;
        };
        let repo = session.backend();

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
                klog!("commit-panel diff error: {}", e);
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

    pub fn open_compare_with_head(&mut self, target: CommitId) {
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
                self.compare_view = Some(CompareView {
                    base: target,
                    target: CompareTarget::Head,
                    files,
                    title,
                });
            }
            Err(e) => {
                klog!("compare: error: {}", e);
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
                self.compare_view = Some(CompareView {
                    base: target,
                    target: CompareTarget::WorkingTree,
                    files,
                    title,
                });
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
            Some(target) => self.open_compare_with_head(target),
            None => klog!("compare: row={} out of range", row_index),
        }
    }

    pub fn open_compare_with_working_tree_row(&mut self, row_index: usize) {
        match self.commit_id_for_row(row_index) {
            Some(target) => self.open_compare_with_working_tree(target),
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

    fn branch_history_commits(&self, target: &CommitId) -> HashSet<CommitId> {
        let parents_by_id: HashMap<CommitId, Vec<CommitId>> = self
            .active_view
            .rows
            .iter()
            .map(|row| (row.id.clone(), row.parents.clone()))
            .collect();
        collect_history_commits(target, &parents_by_id)
    }

    pub fn toggle_branch_solo(&mut self, name: String, target: CommitId, cx: &mut Context<Self>) {
        let already_soloed = self
            .active_view
            .branch_solo
            .as_ref()
            .is_some_and(|solo| solo.name == name && solo.target == target);

        if already_soloed {
            self.active_view.branch_solo = None;
            self.status_footer = FooterStatus::Idle(SharedString::from("Solo off"));
            self.push_toast(ToastKind::Info, "Solo off", cx);
            return;
        }

        let visible_commits = self.branch_history_commits(&target);
        self.active_view.branch_solo = Some(BranchSolo {
            name: name.clone(),
            target,
            visible_commits,
        });
        self.status_footer = FooterStatus::Idle(SharedString::from(format!("Solo: {}", name)));
        self.push_toast(ToastKind::Info, format!("Solo: {}", name), cx);
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
                    .active_view
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
            BranchAction::ToggleSolo => {
                self.toggle_branch_solo(state.name, state.target, cx);
            }
            BranchAction::Checkout => {
                if matches!(state.kind, BranchKind::Local) {
                    self.open_plan_modal(state.name);
                } else {
                    self.open_tracking_checkout_modal(state.name);
                }
            }
            BranchAction::SwitchToLatest => {
                let (branch_name, remote_branch) = if matches!(state.kind, BranchKind::Local) {
                    let upstream = self
                        .active_view
                        .branch_upstream_info
                        .get(&state.name)
                        .map(|u| u.remote_branch.clone());
                    (state.name.clone(), upstream)
                } else {
                    (
                        default_tracking_branch_name(&state.name),
                        Some(state.name.clone()),
                    )
                };
                match remote_branch {
                    Some(remote_branch) => {
                        self.open_switch_to_latest_modal(branch_name, remote_branch);
                    }
                    None => {
                        self.status_footer =
                            FooterStatus::Idle(SharedString::from(Msg::BcmNoUpstream.t()));
                    }
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
                        .active_view
                        .branches
                        .iter()
                        .any(|(name, current)| name == &state.name && *current);
                    if is_current {
                        self.open_pull_modal(cx);
                    } else {
                        self.open_branch_plan_modal(state.name, BranchPlanKind::PullFfOnly);
                    }
                }
            }
            BranchAction::Push => {
                if matches!(state.kind, BranchKind::Local) {
                    let is_current = self
                        .active_view
                        .branches
                        .iter()
                        .any(|(name, current)| name == &state.name && *current);
                    if is_current {
                        self.open_push_modal(cx);
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
                    .active_view
                    .worktrees
                    .iter()
                    .find(|wt| wt.branch.as_deref() == Some(state.name.as_str()))
                    .map(|wt| wt.path.display().to_string());
                if let Some(path) = existing_path {
                    self.status_footer = FooterStatus::Idle(SharedString::from(format!(
                        "worktree already exists: {}",
                        path
                    )));
                    self.push_toast(ToastKind::Info, format!("Worktree: {}", path), cx);
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
                    if let Some(detail) = self.active_view.details.get(row_index) {
                        let full_sha = detail.full_sha.as_ref().to_string();
                        let short: String = full_sha.chars().take(8).collect();
                        context_menu::copy_full_sha(self, full_sha, cx);
                        // W18-COAUTHOR-COPY: surface a toast so the copy is
                        // visible regardless of where it was triggered
                        // (hash chip click or the "Copy SHA" action button).
                        self.push_toast(ToastKind::Info, format!("Copied {}", short), cx);
                    }
                }
            }
            CommitAction::CopyShortSha => {
                if let Some(row_index) = self.row_for_commit_id(&target) {
                    if let Some(detail) = self.active_view.details.get(row_index) {
                        let full_sha = detail.full_sha.as_ref().to_string();
                        context_menu::copy_short_sha(self, &full_sha, cx);
                    }
                }
            }
            CommitAction::CopyMessage => {
                if let Some(row_index) = self.row_for_commit_id(&target) {
                    if let Some(detail) = self.active_view.details.get(row_index) {
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
                    self.create_branch_modal()
                        .map(|m| m.at.short())
                        .unwrap_or_default()
                );
            }
            CommitAction::CreateWorktreeHere => {
                self.open_create_worktree_modal(target, cx);
                eprintln!(
                    "[kagi] context-menu: create-worktree {}",
                    self.create_worktree_modal()
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
                klog!("context-menu: stub Reset {}", target.short());
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
        if self.discard_modal().is_some() {
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
        if self.discard_modal().is_some() {
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
            klog!("fonts: add_fonts failed (UI may fall back): {e}");
        } else {
            klog!("fonts: loaded Inter + JetBrains Mono");
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
                window.focus(&fh);
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
    use super::{collect_history_commits, context_branch_name, draggable_branch_name};
    use crate::ui::commit_list::{BadgeKind, RefBadge};
    use kagi_git::CommitId;
    use std::collections::{HashMap, HashSet};

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

    #[test]
    fn history_collection_follows_all_merge_parents() {
        let tip = CommitId("m".to_string());
        let a = CommitId("a".to_string());
        let b = CommitId("b".to_string());
        let root = CommitId("root".to_string());
        let mut parents = HashMap::new();
        parents.insert(tip.clone(), vec![a.clone(), b.clone()]);
        parents.insert(a.clone(), vec![root.clone()]);
        parents.insert(b.clone(), vec![root.clone()]);
        parents.insert(root.clone(), vec![]);

        let actual = collect_history_commits(&tip, &parents);
        let expected = HashSet::from([tip, a, b, root]);
        assert_eq!(actual, expected);
    }
}
