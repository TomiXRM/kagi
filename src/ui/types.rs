//! Small auxiliary / presentational helper types for the `ui` module.
//!
//! Extracted verbatim from `ui/mod.rs` (issue #13 Phase 1, P1). These are the
//! small drag/toast/status helper types that are NOT `KagiApp` or
//! `TabViewState` and are not coupled to state-construction. Behaviour is
//! unchanged — this is a pure physical split.

use std::time::Duration;

use gpui::{div, prelude::*, rgb, Context, SharedString, Window};

use super::theme;

// ──────────────────────────────────────────────────────────────
// T-BP-002: Bottom Panel — tab enum
// ──────────────────────────────────────────────────────────────

/// Active tab in the bottom panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomTab {
    OperationLog,
    Terminal,
    /// Commit-activity line chart + contributor ranking.
    Activity,
}

impl BottomTab {
    pub(crate) fn label(self) -> &'static str {
        match self {
            BottomTab::OperationLog => "Operation Log",
            BottomTab::Terminal => "Terminal",
            BottomTab::Activity => "Activity",
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
    /// ADR-0089: horizontal divider between the File History commit list (top)
    /// and the diff viewer (bottom).  Adjusts the list/diff vertical split.
    FileHistoryRows,
    /// T-WS-EDITOR-004: vertical divider between the Editor Workspace's left
    /// file tree and its center code viewer. Adjusts `EditorWorkspaceView::tree_w`.
    EditorTree,
    /// T-WS-EDITOR-004: vertical divider between the Editor Workspace's center
    /// code viewer and its right hunks pane. Adjusts `EditorWorkspaceView::hunks_w`.
    EditorHunks,
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
        let (badge_bg, badge_border, badge_text) = theme::badge_style(theme::theme().color_branch);
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
    pub(crate) fn should_start_exit(&self) -> bool {
        self.dismissing.is_none() && self.born.elapsed() >= self.lifetime()
    }
}
