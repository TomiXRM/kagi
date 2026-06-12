//! UI module — T008: GPUI commit list / T009: commit graph lane / T010: commit selection + detail panel / T011: changed files list / T012: file diff viewer / T013: checkout plan modal + sidebar / T023: pane resize
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

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::{
    App, Context, Entity, FocusHandle, KeyDownEvent, SharedString, Window,
    div, prelude::*, px, rgb, uniform_list,
};
use gpui_component::input::{Input, InputState};

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

use kagi::git::{
    ChangeKind, CommitId, FileDiff, DiffLineKind, FileStatus, Head, RepoSnapshot, Stash,
    ops::{
        OperationPlan, StateSummary,
        execute_checkout, execute_create_branch, plan_checkout, plan_create_branch, preflight_check,
        plan_stash_push, execute_stash_push,
        plan_stash_apply, execute_stash_apply,
        preflight_check_stash,
        plan_cherry_pick, execute_cherry_pick,
    },
    oplog::{OpLogEntry, OpOutcome, append_oplog},
    stage_file, unstage_file, plan_commit, execute_commit,
};
use commit_panel::{CommitPanelState, CommitPanelFileRef, CommitPlanModal, status_badge};
use commit_list::{BadgeKind, CommitRow, build_commit_rows};
use detail_panel::{CommitDetail, build_commit_details};
use graph_view::{graph_canvas, graph_width};

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
    // ── T025: Commit Panel ───────────────────────────────────────
    /// Whether the commit panel is currently open (WIP row selected).
    pub commit_panel_open: bool,
    /// Commit panel state (staging lists, diff, message, modal).
    pub commit_panel: Option<CommitPanelState>,
    // ── T026: gpui-component Input for commit message (IME対応) ───
    /// InputState entity for the commit message field (gpui-component IME対応).
    /// Created lazily when the commit panel is first opened (requires &mut Window).
    pub commit_input: Option<Entity<InputState>>,
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

        KagiApp {
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
            commit_panel_open: false,
            commit_panel: None,
            commit_input: None,
        }
    }

    /// Construct a placeholder for the no-argument / error case.
    pub fn with_error(message: impl Into<String>) -> Self {
        KagiApp {
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
            commit_panel_open: false,
            commit_panel: None,
            commit_input: None,
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
        // status_footer is intentionally preserved across reloads so the last
        // operation result remains visible after the commit list refreshes.
        // sidebar_width / panel_width are also preserved so the user's resize
        // is not lost on checkout/reload (T023).
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
        match plan_stash_push(&mut repo, msg_opt) {
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
        if let Err(e) = execute_stash_push(&mut repo, msg_opt) {
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

        let repo_str = repo_path.display().to_string();
        let entry = OpLogEntry::new(op, &repo_str, before, outcome);

        if let Err(e) = append_oplog(&entry) {
            eprintln!("[kagi] oplog: write failed (non-fatal): {}", e);
        }

        if footer_ok {
            eprintln!("[kagi] footer: {}", footer_msg);
            self.status_footer = FooterStatus::Success(footer_msg);
        } else {
            eprintln!("[kagi] footer: {}", footer_msg);
            self.status_footer = FooterStatus::Failed(footer_msg);
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
}

impl Render for KagiApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.header.clone();
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
        let create_branch_modal = self.create_branch_modal.clone();
        let modal_focus = self.modal_focus.clone();
        let stash_push_modal = self.stash_push_modal.clone();
        let stash_push_focus = self.stash_push_focus.clone();
        let stash_apply_modal = self.stash_apply_modal.clone();
        let cherry_pick_modal = self.cherry_pick_modal.clone();
        let status_footer = self.status_footer.clone();

        // T023: pane widths for divider rendering.
        let sidebar_width = self.sidebar_width;
        let panel_width = self.panel_width;

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
            }
        });

        // T025/T026: extract commit panel state for render.
        let commit_panel_open = self.commit_panel_open;
        let commit_panel = self.commit_panel.clone();
        let commit_input = self.commit_input.clone();

        // ── Normal state: header + body (sidebar + list + optional panel) ─────
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG_BASE))
            // T023: capture drag-move for both dividers on the root element.
            .on_drag_move::<DividerDrag>(divider_drag_move)
            // ── Header bar ──────────────────────────────────
            .child({
                let stash_click = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
                    this.open_stash_push_modal(cx);
                    cx.notify();
                });
                let mut header_bar = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .w_full()
                    .px_3()
                    .py_1()
                    .bg(rgb(BG_SURFACE))
                    .text_color(rgb(TEXT_SUB))
                    .child(div().flex_1().overflow_hidden().child(header));
                // Show Stash button only when working tree is dirty.
                if is_dirty {
                    header_bar = header_bar.child(
                        div()
                            .id("stash-push-btn")
                            .ml_2()
                            .px_2()
                            .py_px()
                            .rounded_sm()
                            .bg(rgb(COLOR_WARNING))
                            .text_sm()
                            .text_color(rgb(BG_BASE))
                            .on_click(stash_click)
                            .hover(|style| style.opacity(0.85))
                            .child(SharedString::from("Stash")),
                    );
                }
                header_bar
            })
            // ── Body row: sidebar | divider1 | list (flex_1) | divider2 | optional panel ─
            .child({
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

                let commit_list_col = div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
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
                                // Badges column: WIP badge
                                .child(
                                    div()
                                        .w(px(150.))
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
                                render_rows(&this.rows, range, selected, cx)
                            }),
                        )
                        .flex_1()
                        .min_h(px(0.)),
                    );

                let mut body_row = div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .h_full()
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
            })
            // ── Plan modal overlay (above everything) ──────
            .when_some(plan_modal, |el, modal| {
                el.child(render_plan_modal(modal, cx))
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
            // ── Status footer (T017) — last operation result ─
            .child(render_status_footer(status_footer))
            .into_any()
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

            // ── Graph lane area (T009) ────────────────────────
            // Width is clamped to MAX_LANES lanes; unborn/empty repos
            // get lane_count=0 → graph_w=0 → no canvas rendered.
            let g_w = graph_width(row.lane_count);

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
                // ── Badges column: fixed 150px, right-aligned, graph side (T021) ──
                .child(render_badges_column(&row.badges))
                // ── Graph lane area (T009) ────────────────────────
                .when(g_w > 0.0, |el| {
                    el.child(
                        div()
                            .w(px(g_w))
                            .h_full()
                            .flex_shrink_0()
                            .child(
                                graph_canvas(row.lane, row.edges.clone())
                                    .size_full(),
                            ),
                    )
                })
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
                        .overflow_hidden()
                        .child(row.summary.clone()),
                )
                .child(
                    div()
                        .w(px(130.))
                        .flex_shrink_0()
                        .text_color(rgb(TEXT_SUB))
                        .overflow_hidden()
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

/// Render the badges column: fixed 150px, right-aligned (`justify_end`),
/// `overflow_hidden`.  An empty badges list still occupies the full 150px so
/// that all rows share the same graph start position (GitKraken layout, T021).
fn render_badges_column(badges: &[commit_list::RefBadge]) -> impl IntoElement {
    const MAX_BADGE_CHARS: usize = 24;

    // Highest-priority badge (HEAD) goes RIGHTMOST: the column is
    // right-justified and clips on the left, so the rightmost badge — the
    // one nearest the graph — is the one that survives clipping.
    let mut sorted: Vec<&commit_list::RefBadge> = badges.iter().collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(badge_priority(&b.kind)));

    let mut inner = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_end()
        .gap_1();

    for badge in &sorted {
        let color = match badge.kind {
            BadgeKind::HeadBranch => COLOR_HEAD,
            BadgeKind::Branch => COLOR_BRANCH,
            BadgeKind::Remote => COLOR_REMOTE,
            BadgeKind::Tag => COLOR_TAG,
        };
        // Truncate long labels so they don't overflow the column.
        let label: SharedString = if badge.label.chars().count() > MAX_BADGE_CHARS {
            let s: String = badge.label.chars().take(MAX_BADGE_CHARS - 1).collect();
            SharedString::from(format!("{}\u{2026}", s))
        } else {
            badge.label.clone()
        };
        let chip = div()
            .px_1()
            .rounded_sm()
            .bg(rgb(color))
            .text_color(rgb(BG_BASE))
            .text_sm()
            .flex_shrink_0()
            .child(label);
        inner = inner.child(chip);
    }

    // Fixed 150px container, overflow clipped so long badge lists don't push graph.
    div()
        .w(px(150.))
        .flex_shrink_0()
        .overflow_hidden()
        .flex()
        .flex_row()
        .items_center()
        .justify_end()
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
    let mut col = div()
        .w(px(width))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(BG_SIDEBAR))
        .py_2()
        // ── LOCAL BRANCHES label ──────────────────────────
        .child(
            div()
                .px_3()
                .py_1()
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
                .px_3()
                .py_1()
                .text_sm()
                .text_color(rgb(text_color))
                .overflow_hidden()
                .child(label)
                .into_any()
        } else {
            let click_handler = cx.listener(move |this, _event: &gpui::ClickEvent, _window, cx| {
                this.open_plan_modal(branch_for_click.clone());
                cx.notify();
            });
            div()
                .id(SharedString::from(format!("sidebar-branch-{}", branch_name)))
                .flex()
                .flex_row()
                .items_center()
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

    col
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
fn render_plan_modal(modal: CheckoutPlanModal, cx: &mut Context<KagiApp>) -> impl IntoElement {
    let plan = modal.plan.clone();
    let has_blockers = !plan.blockers.is_empty();

    // ── Cancel handler ──────────────────────────────────────
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.cancel_modal();
        cx.notify();
    });

    // ── Confirm handler (only created when no blockers) ─────
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.confirm_checkout();
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
    if let Some(err) = &modal.error {
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
                .child(SharedString::from("Checkout")),
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
    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.cancel_create_branch_modal();
        cx.notify();
    });

    // ── Confirm handler (only created when no blockers) ─────
    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.confirm_create_branch();
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

    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.cancel_stash_push_modal();
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.confirm_stash_push();
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

    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.cancel_stash_apply_modal();
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.confirm_stash_apply();
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

    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.cancel_cherry_pick_modal();
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.confirm_cherry_pick();
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
    let mut unstaged_section = div()
        .flex()
        .flex_col()
        .flex_shrink_0();

    // Header row
    unstaged_section = unstaged_section.child(
        div()
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
            .child(toggle_btn),
    );

    if tree_view {
        // Tree view: use build_file_tree
        let tree_rows = file_tree::build_file_tree(&panel.unstaged);
        for row in &tree_rows {
            match row {
                file_tree::TreeRow::Dir { depth, name } => {
                    let indent = (*depth as f32) * 12.0;
                    unstaged_section = unstaged_section.child(
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
                    unstaged_section = unstaged_section.child(file_row);
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
            unstaged_section = unstaged_section.child(file_row);
        }
    }

    // ── Staged section ───────────────────────────────────────
    let mut staged_section = div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .mt_1();

    staged_section = staged_section.child(
        div()
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
            ),
    );

    if tree_view {
        let tree_rows = file_tree::build_file_tree(&panel.staged);
        for row in &tree_rows {
            match row {
                file_tree::TreeRow::Dir { depth, name } => {
                    let indent = (*depth as f32) * 12.0;
                    staged_section = staged_section.child(
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
                    staged_section = staged_section.child(
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
            staged_section = staged_section.child(
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
        // Unstaged section (scrollable within fixed height)
        .child(
            div()
                .id("cp-unstaged-scroll")
                .flex_shrink_0()
                .max_h(px(150.))
                .overflow_y_scroll()
                .child(unstaged_section),
        )
        // Staged section (scrollable within fixed height)
        .child(
            div()
                .id("cp-staged-scroll")
                .flex_shrink_0()
                .max_h(px(150.))
                .overflow_y_scroll()
                .child(staged_section),
        )
        // Diff area (flex_1 — takes remaining space)
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

    let cancel_handler = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.cancel_commit_plan_modal();
        cx.notify();
    });

    let confirm_handler = cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        this.confirm_commit(cx);
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
pub fn run_app(app_state: KagiApp) {
    use gpui::{Application, Bounds, WindowBounds, WindowOptions, size};

    Application::new().run(move |cx: &mut App| {
        // T025: initialize gpui-component (registers key bindings, themes, etc.)
        gpui_component::init(cx);

        let bounds = Bounds::centered(None, size(px(1024.), px(768.)), cx);
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
                let kagi: Entity<KagiApp> = cx.new(|_| app_state);
                // Regression coverage for the Root::read crash: with
                // KAGI_COMMIT_PANEL=1, open the panel through the real
                // window-context path so the InputState + Input element
                // actually render during headless verification (the
                // pre-window env path in main.rs cannot create them).
                if std::env::var("KAGI_COMMIT_PANEL").as_deref() == Ok("1") {
                    kagi.update(cx, |app, cx| app.open_commit_panel(window, cx));
                }
                cx.new(|cx| gpui_component::Root::new(kagi, window, cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
