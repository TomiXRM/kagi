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
    /// Shared staging lists in the compact allocation.
    Changes,
}

impl BottomTab {
    pub(crate) fn label(self) -> &'static str {
        match self {
            BottomTab::OperationLog => super::i18n::Msg::BottomTabOperationLog.t(),
            BottomTab::Terminal => super::i18n::Msg::ToolbarTerminal.t(),
            BottomTab::Activity => super::i18n::Msg::BottomTabActivity.t(),
            BottomTab::Changes => super::i18n::Msg::BottomTabChanges.t(),
        }
    }
}

// ──────────────────────────────────────────────────────────────
// T023: Pane resize — divider drag state
// ──────────────────────────────────────────────────────────────

// ADR-0121 C3: the divider drag types moved to `kagi_ui_core::divider` so
// extracted pane crates (File History, …) can start divider drags. Re-exported
// here so existing `types::Divider…` / glob-import paths keep working.
pub use kagi_ui_core::divider::{DividerDrag, DividerGhost, DividerKind};

// ── T-DNDMERGE-001 / ADR-0079: branch drag-and-drop merge ─────
//
// Dragging a LOCAL branch label in the sidebar and dropping it on the current
// (checked-out) branch row starts a "merge dragged into current" preview.  The
// drop is a TRIGGER ONLY: it dispatches to `KagiApp::start_merge_from_drag`,
// which validates and delegates to the existing `open_merge_modal` pipeline.
// No git is executed on drop (ADR-0079).

/// Drag payload carrying the dragged local or remote-tracking branch name.  Layer 1 (the view)
/// emits this on `on_drag`; the current-branch drop zone consumes it via
/// `on_drop::<BranchDrag>` and dispatches it to the action layer.
#[derive(Clone, Debug)]
pub struct BranchDrag {
    /// The dragged local or remote-tracking branch name (= merge *source*).
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
        // same `graph_badge_style(color_branch)` tint, `rounded_sm`, `px_1`, `text_sm`.
        // gpui renders this ghost entity at the cursor automatically, so we only
        // need it to LOOK like the lifted badge. Used by BOTH the graph-badge
        // drag and the sidebar drag (kept consistent).
        let (badge_bg, badge_border, badge_text) =
            theme::graph_badge_style(theme::theme().color_branch);
        div()
            // Active drag views are separate GPUI roots, not descendants of Kit Root.
            .font(kagi_ui_core::fonts::ui_font())
            .px_1()
            .rounded_sm()
            .bg(gpui::rgba(badge_bg))
            .border_1()
            .border_color(gpui::rgba(badge_border))
            .text_sm()
            .text_color(rgb(badge_text))
            .child(self.name.clone())
            .map(|ghost| {
                #[cfg(feature = "gui-e2e")]
                let ghost = ghost.child(kagi_ui_core::fonts::e2e::probe("branch-drag-ghost"));
                ghost
            })
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
    /// W2-STATUS: a git operation is in progress (blue, with the spinning
    /// sync icon - `render_overlay::sync_spinner`).
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
    /// Operation failed or was refused (red; persists until explicit dismissal).
    Error,
    /// Sync-flavoured info (blue, 4s): renders the same big spinning sync icon
    /// as the busy snackbar, so "already up to date" reads consistently with
    /// an in-flight pull/push. Used for the no-op pull/push snackbars.
    Sync,
}

/// A shared toast retains a distinct close-button handle for every host window.
pub type ToastCloseFocus =
    std::rc::Rc<std::cell::RefCell<Vec<(gpui::WindowId, gpui::FocusHandle)>>>;

/// Actions remain inert until the user activates the notification's Button.
#[derive(Clone, Debug)]
pub enum ToastAction {
    OpenRepository(std::path::PathBuf),
    OpenOperationLog(super::oplog_panel::EntryKey),
    RefreshRepository(crate::app::Attachment),
}

/// One subject-owned notification with a shared lifecycle. Window hosts project
/// cards or the conflict footer without duplicating timers or action identity.
///
/// Pruned by a ticker that runs only while routine feedback is pending.
/// We deliberately do NOT use gpui-component's Root notification layer:
/// pushing into it requires `Root::update` and our render runs *inside*
/// Root's render pass, so that would re-entrantly borrow the Root entity.
#[derive(Clone, Debug)]
pub struct Toast {
    /// Unique id (for the dismiss button element id).
    pub id: u64,
    pub kind: ToastKind,
    pub subject: super::toast_stack::ToastSubject,
    pub message: SharedString,
    /// Stable repository / requested operation / outcome identity.
    pub result_key: Option<String>,
    pub action: Option<ToastAction>,
    pub refresh_owner: Option<crate::app::Attachment>,
    /// Creation time; drives the enter animation and auto-dismiss timing.
    pub born: std::time::Instant,
    /// Hover pause began here; elapsed hover time is excluded from the timer.
    pub paused_at: Option<std::time::Instant>,
    /// Per-window rendered Button handles, retained until removal repairs focus.
    pub close_focus: ToastCloseFocus,
    pub action_focus: Option<ToastCloseFocus>,
    pub refresh_focus: Option<ToastCloseFocus>,
    /// `Some(t)` once the toast has begun sliding out (auto-expiry or the ×
    /// button): `t` is when the exit animation started. The toast keeps
    /// rendering during the exit so the slide-out is visible, then a one-shot
    /// timer removes it. `None` while it is entering / visible.
    pub dismissing: Option<std::time::Instant>,
}

impl Toast {
    pub fn is_persistent(&self) -> bool {
        self.kind == ToastKind::Error || self.action.is_some() || self.refresh_owner.is_some()
    }

    /// Success/info/sync previews expire after four seconds of unhovered time.
    /// Errors require explicit dismissal, giving readers an unlimited interval.
    fn lifetime(&self) -> Duration {
        crate::ui::toast_stack::TOAST_ROUTINE_LIFETIME
    }

    /// The toast has been visible long enough to start sliding out (and is not
    /// already doing so).
    pub(crate) fn should_start_exit(&self) -> bool {
        !self.is_persistent()
            && self.paused_at.is_none()
            && self.dismissing.is_none()
            && self.born.elapsed() >= self.lifetime()
    }
}
