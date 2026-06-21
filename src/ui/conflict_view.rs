//! W30/W33-CONFLICT-UI: Conflict Mode UI — persistent banner + Conflict
//! Dashboard (right panel) + per-file choose / preview center.
//!
//! This module is the **UI half** of the conflict feature.  All git logic lives
//! in the `kagi_git::conflicts` / `resolution` backend (W26): this file only
//! *renders* a [`ConflictMode`] snapshot and wires its buttons to the `KagiApp`
//! handlers (which in turn call the backend `plan_*` / `ResolutionBuffer` API).
//! No `git2` calls happen here.
//!
//! # W33 scope (this lane)
//!
//! - **Conflict Dashboard** (ADR-0063 / T-CONFLICT-DASH-021/022): the right panel
//!   in Conflict Mode, ordered state → next action → files:
//!   - state: op header + direction summary + Current/Incoming role badges +
//!     conflicted/resolved counts (with prev/next nav),
//!   - next action: Continue (filled, gated on `can_continue`) and Abort (danger,
//!     two-stage) side by side, the blocker/ready note, and a *secondary*
//!     Next-conflict / Skip row,
//!   - files: Conflicted / Resolved cards; each card carries icon buttons
//!     (open externally, copy path) plus a "…" overflow menu (the full action
//!     set) rendered via the shared `menu_overlay` (so it clamps on-screen).
//! - Activating a Conflicted row sets `editing_file` (W32's Conflict Editor lane
//!   reads it); this lane does not implement the editor.
//!
//! Terminology (ADR-0058): every side label comes from `side_labels` — the words
//! "ours"/"theirs" never appear in any user-facing string.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::{
    div, prelude::*, px, rgb, Context, Entity, MouseButton, SharedString, UniformListScrollHandle,
    Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::InputState;
use gpui_component::tooltip::Tooltip;
use gpui_component::{Disableable as _, Sizable as _};

use kagi_git::conflicts::{ConflictKind, ConflictOp, ConflictStatus, SideLabels};

use super::button_style::{apply_accent, KagiButton};
use super::context_menu::{ItemState, MenuGroup, MenuItem};
use super::i18n::Msg;
use super::menu_overlay;
use super::theme::{self, theme};
use super::ConflictEditorInputs;
use super::KagiApp;

/// T-CONFLICT-UI: the per-pane editor state for the open file.
/// Held on [`KagiApp`] (they need a `Window` to create) and passed into the
/// render functions via [`EditorChrome`].
#[derive(Clone)]
pub struct EditorInputs {
    /// The conflicting file these inputs are bound to.
    pub path: PathBuf,
    /// Result editor (read-only in Preview, editable in Edit).
    pub result: Entity<InputState>,
}

/// T-CONFLICT-UI/UX: the Conflict Editor chrome the app threads into the render
/// functions (the editors + split ratios + Result mode + resize geometry live
/// on [`KagiApp`], not on the cloned [`ConflictMode`]).
#[derive(Clone)]
pub struct EditorChrome {
    /// The Result pane `InputState`, when present for the edited content file.
    pub inputs: Option<EditorInputs>,
    /// Shared A/B row-list scroll handle; both panes track it for vertical sync.
    pub ab_scroll: UniformListScrollHandle,
    /// Whether the Result pane is in Edit mode (UX-015).
    pub result_editing: bool,
    /// Whether the destructive "Reset all" is armed (POLISH-042).
    pub reset_all_armed: bool,
    /// A|B pane width split ratio (fraction given to A).
    pub ab_split: f32,
    /// A·B / Result vertical split ratio (fraction given to the A·B row).
    pub result_split: f32,
    /// T-CONFLICT-UX-010/012: index (among conflict hunks) of the focused hunk,
    /// for the selected-hunk highlight in the per-hunk accept list.
    pub selected_hunk: usize,
    /// Measured (top, bottom) screen-px bounds of the editor split region.
    pub geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    /// Measured (left, right) screen-px bounds of the A·B row.
    pub ab_geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
}

/// Consolidated Conflict-Editor view state.
///
/// Previously thirteen+ flat `conflict_*` fields on the `KagiApp` god-struct;
/// grouped here as the prep step for a future `Entity<ConflictState>` migration
/// (ADR-0110 Phase 5 Step 5.1). All fields are app-global (not per-tab); cleared
/// on reload / abort. Pure constructible (no `cx`) — the `editor_inputs`
/// `InputState`s are created lazily in a `Window` context. The status-bar
/// `conflict_count` badge and `merge_commit_ready` are intentionally left on
/// `KagiApp` (separate concerns, not editor state).
pub struct ConflictState {
    /// `Some(_)` while a conflict/merge is in progress (set by
    /// `detect_conflict_mode`). The repository is untouched until Continue /
    /// Abort execute through the existing plan pipeline.
    pub mode: Option<ConflictMode>,
    /// Guard so render-time detection runs at most once per (repo_path) until a
    /// reload / tab switch invalidates it. Holds the repo path whose conflict
    /// state has been detected this cycle.
    pub detected_for: Option<PathBuf>,
    /// W32: `Some(path)` while the dedicated hunk-level Conflict Editor is open
    /// for that conflicting file. `None` shows the Dashboard.
    pub editing: Option<PathBuf>,
    /// W32: last-saved resolved text per file, so a Save can log a before→after
    /// file-content hash pair (T-035) without re-reading the working tree.
    pub editing_before_text: HashMap<PathBuf, String>,
    /// T-CONFLICT-UI-001/005: the three CodeEditor `InputState`s backing the
    /// A / B / Result panes (ADR-0069). Lazily created in a `Window` context.
    pub editor_inputs: Option<ConflictEditorInputs>,
    /// T-CONFLICT-UX-015: whether the Result pane is in Edit mode (editable)
    /// rather than Preview (read-only).
    pub result_editing: bool,
    /// T-CONFLICT-POLISH-042: armed state for the destructive "Reset all".
    pub reset_all_armed: bool,
    /// T-CONFLICT-UI-003: A|B pane width split ratio (fraction given to A).
    pub ab_split: f32,
    /// T-CONFLICT-UI-003: A·B / Result vertical split ratio (fraction to A·B).
    pub result_split: f32,
    /// T-CONFLICT-UI-003: measured (top, bottom) screen-px bounds of the
    /// editor's split region, for absolute-coordinate divider dragging.
    pub geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    /// T-CONFLICT-UI-003: measured (left, right) screen-px bounds of the A·B
    /// row, for the vertical A|B divider drag.
    pub ab_geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    /// T-CONFLICT-FLOW-030/031 (ADR-0068): showing the merge commit panel
    /// (every file saved + staged, MERGE_HEAD still present). Cleared on commit /
    /// abort / reload.
    pub merge_commit_pending: bool,
    /// T-CONFLICT-UX-010/012: index (among conflict hunks) of the focused hunk
    /// in the per-hunk Conflict Editor.
    pub selected_hunk: usize,
    /// ADR-0070: shared A/B uniform-list scroll handle for synchronized vertical
    /// scrolling in the Conflict Editor.
    pub ab_scroll_handle: UniformListScrollHandle,
    /// T-CONFLICT-DASH-022: the open per-file "…" context menu, as
    /// `(file_index, anchor)` where `anchor` is the click position in window px.
    /// `None` when no menu is open. Rendered as a top-level overlay (render.rs).
    pub file_menu: Option<(usize, gpui::Point<gpui::Pixels>)>,
}

impl ConflictState {
    pub fn new() -> Self {
        Self {
            mode: None,
            detected_for: None,
            editing: None,
            editing_before_text: HashMap::new(),
            editor_inputs: None,
            result_editing: false,
            reset_all_armed: false,
            ab_split: super::CONFLICT_AB_DEFAULT,
            result_split: super::CONFLICT_RESULT_DEFAULT,
            geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            ab_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            merge_commit_pending: false,
            selected_hunk: 0,
            ab_scroll_handle: UniformListScrollHandle::new(),
            file_menu: None,
        }
    }
}

impl Default for ConflictState {
    fn default() -> Self {
        Self::new()
    }
}

/// Pure UI-side data: the `ConflictSession` describes the in-progress operation
/// and its files; the `ResolutionBuffer` holds the in-memory Result drafts (the
/// repository is untouched until Continue/Abort/Skip execute through the plan
/// pipeline).  `current_branch` is captured once at detection time for the
/// `side_labels` left role.
#[derive(Clone)]
pub struct ConflictMode {
    /// The detected conflict session (operation + files), with per-file `status`
    /// recomputed from the buffer at detection time.
    pub session: kagi_git::conflicts::ConflictSession,
    /// The resolution buffer (in-memory Result drafts + materialized sides).
    pub buffer: kagi_git::resolution::ResolutionBuffer,
    /// Current branch short name, for the `side_labels` left role.
    pub current_branch: String,
    /// Index into `session.files` of the file whose detail/preview is open.
    pub selected_file: Option<usize>,
    /// Index into `session.files` of the file currently open in the Conflict
    /// Editor (W32 lane reads/owns this; the dashboard only *sets* it when a
    /// Conflicted row is activated).  `None` when no editor file is open.
    pub editing_file: Option<usize>,
    /// Whether the destructive Abort is armed (two-stage confirm, ADR-0067).
    pub abort_armed: bool,
}

impl ConflictMode {
    /// Number of files with a resolution draft in the buffer.
    pub fn resolved_count(&self) -> usize {
        self.session
            .files
            .iter()
            .filter(|f| f.status != ConflictStatus::Unresolved)
            .count()
    }

    /// Number of files still unresolved.
    pub fn conflicted_count(&self) -> usize {
        self.session
            .files
            .iter()
            .filter(|f| f.status == ConflictStatus::Unresolved)
            .count()
    }

    /// Whether Continue is allowed from the UI's point of view: every file has
    /// a clean resolution (no marker residue), every binary has a side chosen,
    /// and every keep-or-delete decision is made.  This mirrors the buffer-only
    /// subset of `kagi_git::continue_blockers`; the repo-bound checks (index
    /// unmerged / empty merge message) are enforced at execute time in mod.rs.
    pub fn can_continue(&self) -> bool {
        self.continue_blocker().is_none()
    }

    /// The first UI-visible Continue blocker, or `None` when Continue is allowed.
    /// Returns a localized `Msg` describing the specific blocking reason
    /// (ADR-0067 — surface the specific reason in the UI).
    pub fn continue_blocker(&self) -> Option<Msg> {
        // 1. Any file without a resolution draft.
        if self
            .session
            .files
            .iter()
            .any(|f| !self.buffer.has_resolution(&f.path))
        {
            // Distinguish binary / deletion for a more specific message.
            if self
                .session
                .files
                .iter()
                .any(|f| f.kind == ConflictKind::Binary && !self.buffer.has_resolution(&f.path))
            {
                return Some(Msg::ConflictBlockerBinary);
            }
            if self.session.files.iter().any(|f| {
                matches!(
                    f.kind,
                    ConflictKind::ModifyDelete | ConflictKind::RenameDelete
                ) && !self.buffer.has_resolution(&f.path)
            }) {
                return Some(Msg::ConflictBlockerDeletion);
            }
            return Some(Msg::ConflictBlockerUnresolved);
        }
        // 2. Marker residue in any resolved buffer text.
        if !self.buffer.files_with_marker_residue().is_empty() {
            return Some(Msg::ConflictBlockerMarker);
        }
        None
    }

    /// Paths eligible for "Mark all clean files resolved": files with no marker
    /// residue AND a resolvable resolution draft (ADR-0063).  We approximate
    /// "index-resolvable" with "has a clean buffer resolution" — the plan
    /// pipeline re-checks the live index before writing.

    /// The role labels for the current operation (ADR-0058, never ours/theirs).
    pub fn labels(&self) -> SideLabels {
        kagi_git::conflicts::side_labels(&self.session.op, &self.current_branch)
    }
}

// ────────────────────────────────────────────────────────────
// Small localized helpers
// ────────────────────────────────────────────────────────────

/// One-line "what is being merged into what" summary, using ADR-0058 direction
/// wording (Merging X into Y / Rebasing X onto Y / Cherry-picking abc onto Y /
/// Reverting abc on Y).  Never says ours/theirs.  Branch / commit names verbatim.
fn op_summary(mode: &ConflictMode) -> String {
    let labels = mode.labels();
    match &mode.session.op {
        ConflictOp::Rebase { step, total, .. } => format!(
            "{} {} {} {} — {} {}/{}",
            Msg::ConflictRebasing.t(),
            labels.incoming.name,
            Msg::ConflictOnto.t(),
            labels.current.name,
            Msg::ConflictCommit.t(),
            step,
            total
        ),
        ConflictOp::Merge { .. } => format!(
            "{} {} {} {}",
            Msg::ConflictMerging.t(),
            labels.incoming.name,
            Msg::ConflictOnto.t(),
            labels.current.name
        ),
        ConflictOp::CherryPick { .. } => format!(
            "{} {} {} {}",
            Msg::ConflictCherryPicking.t(),
            labels.incoming.name,
            Msg::ConflictOnto.t(),
            labels.current.name
        ),
        ConflictOp::Revert { .. } => format!(
            "{} {} {} {}",
            Msg::ConflictReverting.t(),
            labels.incoming.name,
            Msg::ConflictOnto.t(),
            labels.current.name
        ),
    }
}

/// Map a backend [`kagi_git::ContinueBlocker`] to its localized UI message
/// (ADR-0067 — surface the specific blocking reason).  Used by `conflict_continue`
/// when the plan pipeline refuses, so the toast names the precise reason.
pub fn blocker_msg(b: &kagi_git::ContinueBlocker) -> Msg {
    use kagi_git::ContinueBlocker as B;
    match b {
        B::UnresolvedFiles(_) => Msg::ConflictBlockerUnresolved,
        B::MarkerResidue(_) => Msg::ConflictBlockerMarker,
        B::BinaryUnresolved(_) => Msg::ConflictBlockerBinary,
        B::DeletionUndecided(_) => Msg::ConflictBlockerDeletion,
        B::IndexUnmerged(_) => Msg::ConflictBlockerIndex,
        B::EmptyMergeMessage => Msg::ConflictBlockerMessage,
        B::ChecklistBlocker(_) => Msg::ConflictBlockerChecklist,
    }
}

/// Translated tag text for a conflict kind.
fn kind_tag(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::Content => Msg::ConflictKindContent.t(),
        ConflictKind::RenameDelete => Msg::ConflictKindRenameDelete.t(),
        ConflictKind::ModifyDelete => Msg::ConflictKindModifyDelete.t(),
        ConflictKind::Binary => Msg::ConflictKindBinary.t(),
    }
}

// ────────────────────────────────────────────────────────────
// Banner (persistent, under the header)
// ────────────────────────────────────────────────────────────

/// Render the persistent conflict banner shown directly under the header.
pub fn render_banner(mode: &ConflictMode, _cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let total = mode.session.total_count();
    let resolved = mode.resolved_count();
    let progress = format!("{} {}/{}", Msg::ConflictResolved.t(), resolved, total);

    div()
        .id("conflict-banner")
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .w_full()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(6.))
        .bg(rgb(theme().surface))
        .border_b_1()
        .border_color(rgb(theme().color_warning))
        .child(
            // operation summary + progress laid out horizontally (was stacked)
            // to reduce the banner's height.
            div()
                .flex()
                .flex_row()
                .items_center()
                .flex_grow()
                .gap_3()
                .child(
                    div()
                        .text_size(theme::scaled_px(13.))
                        .text_color(rgb(theme().text_main))
                        .overflow_hidden()
                        .child(SharedString::from(op_summary(mode))),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(theme::scaled_px(11.))
                        .text_color(if mode.can_continue() {
                            rgb(theme().color_success)
                        } else {
                            rgb(theme().text_sub)
                        })
                        .child(SharedString::from(progress)),
                ),
        )
        .into_any_element()
}

// ────────────────────────────────────────────────────────────
// Body: center (file preview / choose) | right (Conflict Dashboard)
// ────────────────────────────────────────────────────────────

/// Render the Conflict Mode main pane.  Center = per-file choose + Result
/// preview (the W32 Conflict Editor lane replaces this center by reading
/// `editing_file`); right = the Conflict Dashboard (ADR-0063).
pub fn render_body(
    mode: &ConflictMode,
    chrome: &EditorChrome,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // GitKraken-style layout: the CENTER is the main 3-pane Conflict Editor
    // (A | B on top, Result below), and the Conflict Dashboard is always on the
    // RIGHT (info + navigation + continue/abort/skip + escape hatch — no
    // resolution actions live there, T-CONFLICT-DASH-020).  For a content
    // conflict we render the hunk-level 3-pane editor; binary / single-sided
    // files (no hunk model) fall back to the simple choose surface.
    let center = match mode.selected_file.and_then(|i| mode.session.files.get(i)) {
        Some(file) if file.kind == ConflictKind::Content => {
            let path = file.path.clone();
            super::conflict_editor::render_editor(mode, chrome, &path, cx)
        }
        _ => render_center(mode, cx),
    };
    // flex_1 + min_h(0) (NOT size_full): in the root flex_col the conflict body
    // must share height with the bottom panel + status bar, not take 100% and
    // push the terminal off-screen (user-reported terminal-position bug).
    div()
        .flex()
        .flex_row()
        .flex_1()
        .min_h(px(0.))
        .bg(rgb(theme().bg_base))
        .child(center)
        .child(render_dashboard(mode, cx))
        .into_any_element()
}

// ────────────────────────────────────────────────────────────
// Right panel: Conflict Dashboard (ADR-0063)
// ────────────────────────────────────────────────────────────

fn render_dashboard(mode: &ConflictMode, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    // T-CONFLICT-DASH-021/022: information hierarchy is state → next action →
    // files. The right sidebar shows what's left and the next steps (Continue
    // and Abort side by side); each file card carries its own actions (open
    // externally / copy path as icon buttons, plus a "…" overflow menu), so
    // utilities never crowd the top-level layout.
    div()
        .id("conflict-dashboard")
        .flex()
        .flex_col()
        .w(theme::scaled_px(238.))
        .h_full()
        .border_l_1()
        .border_color(rgb(theme().surface))
        .bg(rgb(theme().sidebar))
        .overflow_y_scroll()
        // ── State ──
        .child(dash_header(mode))
        .child(dash_role_badges(mode))
        .child(dash_counts(mode, cx))
        // ── Next action (Continue + Abort side by side) ──
        .child(dash_primary(mode, cx))
        // ── Files (per-card actions: icons + "…" overflow menu) ──
        .child(dash_sections(mode, cx))
        .into_any_element()
}

/// Header: operation-specific "Merge conflicts detected" + direction summary.
fn dash_header(mode: &ConflictMode) -> gpui::AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(10.))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(
            div()
                .text_size(theme::scaled_px(14.))
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(Msg::ConflictDashHeader.t())),
        )
        .child(
            div()
                .text_size(theme::scaled_px(11.))
                .text_color(rgb(theme().text_sub))
                .child(SharedString::from(op_summary(mode))),
        )
        .into_any_element()
}

/// Current / Incoming role + real-name badges (tooltip notes the git stage).
fn dash_role_badges(mode: &ConflictMode) -> gpui::AnyElement {
    let labels = mode.labels();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(8.))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(role_badge(
            Msg::ConflictRoleCurrent.t(),
            &labels.current.role,
            &labels.current.name,
            theme().color_branch,
        ))
        .child(role_badge(
            Msg::ConflictRoleIncoming.t(),
            &labels.incoming.role,
            &labels.incoming.name,
            theme().color_remote,
        ))
        .into_any_element()
}

/// A single role badge: side tag + role word + real name. The git-stage hint is
/// attached as a tooltip (ADR-0058 — internal term only in tooltip).
fn role_badge(side: &str, role: &str, name: &str, accent: u32) -> gpui::AnyElement {
    div()
        .id(SharedString::from(format!("conflict-role-{}", side)))
        .flex()
        .flex_col()
        .gap_1()
        .px(theme::scaled_px(8.))
        .py(theme::scaled_px(5.))
        .rounded_md()
        .border_1()
        .border_color(rgb(accent))
        .tooltip(move |window, cx| Tooltip::new(Msg::ConflictGitTermHint.t()).build(window, cx))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_size(theme::scaled_px(10.))
                        .text_color(rgb(accent))
                        .child(SharedString::from(side.to_string())),
                )
                .child(
                    div()
                        .text_size(theme::scaled_px(10.))
                        .text_color(rgb(theme().text_sub))
                        .child(SharedString::from(role.to_string())),
                ),
        )
        .child(
            div()
                .text_size(theme::scaled_px(12.))
                .text_color(rgb(theme().text_main))
                .child(SharedString::from(name.to_string())),
        )
        .into_any_element()
}

/// Conflicted count / resolved count line, with prev/next unresolved nav.
fn dash_counts(mode: &ConflictMode, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let conflicted = mode.conflicted_count();
    let resolved = mode.resolved_count();

    let prev = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_nav_unresolved(-1);
        cx.notify();
    });
    let next = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_nav_unresolved(1);
        cx.notify();
    });

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_4()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(8.))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(
            div()
                .text_size(theme::scaled_px(11.))
                .text_color(rgb(if conflicted == 0 {
                    theme().text_sub
                } else {
                    theme().color_blocker
                }))
                .child(SharedString::from(format!(
                    "{} {}",
                    conflicted,
                    Msg::ConflictConflictedCount.t()
                ))),
        )
        .child(
            div()
                .flex_grow()
                .text_size(theme::scaled_px(11.))
                .text_color(rgb(theme().color_success))
                .child(SharedString::from(format!(
                    "{} {}",
                    resolved,
                    Msg::ConflictResolvedCount.t()
                ))),
        )
        .child(nav_button("‹", prev))
        .child(nav_button("›", next))
        .into_any_element()
}

/// Small prev/next unresolved-file nav button.
fn nav_button<H>(label: &str, handler: H) -> Button
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    Button::new(SharedString::from(format!("conflict-nav-{}", label)))
        .label(SharedString::from(label.to_string()))
        .ghost()
        .small()
        .on_click(handler)
}

/// The "next step" actions (T-CONFLICT-DASH-021): Continue and Abort side by
/// side. Continue is the filled success action, gated on `can_continue` (all
/// conflicts resolved); Abort is the danger action with a two-stage confirm. A
/// status note (ready / specific blocker) sits below, and — for sequencer ops or
/// while blocked — a secondary row offers Next-conflict / Skip.
fn dash_primary(mode: &ConflictMode, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let can_continue = mode.can_continue();
    let is_sequencer = mode.session.op.is_sequencer();

    let continue_handler = cx.listener(|this, _e: &gpui::ClickEvent, window, cx| {
        this.conflict_continue(window, cx);
        cx.notify();
    });
    let abort_handler = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_abort_request(cx);
        cx.notify();
    });
    let abort_label = if mode.abort_armed {
        Msg::ConflictConfirmAbort.t()
    } else {
        Msg::ConflictAbort.t()
    };

    // Continue (filled, gated) + Abort (danger, two-stage) on one row. The
    // buttons size to content (gpui-component Buttons are flex_shrink_0), so wrap
    // rather than overflow the fixed-width dashboard when the armed-abort label
    // grows (T-CONFLICT-DASH-023).
    let primary_row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_2()
        .child(action_button(
            Msg::ConflictContinue.t(),
            theme().color_success,
            can_continue,
            if can_continue {
                Some(continue_handler)
            } else {
                None
            },
            cx,
        ))
        .child(action_button(
            abort_label,
            theme().color_blocker,
            true,
            Some(abort_handler),
            cx,
        ));

    let mut col = div()
        .flex()
        .flex_col()
        .gap_2()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(8.))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(primary_row);

    // Status note: the specific blocker reason, or the ready confirmation.
    let (note, note_color) = match mode.continue_blocker() {
        Some(msg) => (msg.t(), theme().color_blocker),
        None => (Msg::ConflictContinueReady.t(), theme().color_success),
    };
    col = col.child(dash_note(note, note_color));
    if mode.abort_armed {
        col = col.child(dash_note(
            Msg::ConflictConfirmAbortHint.t(),
            theme().color_warning,
        ));
    }

    // Secondary row: Next-conflict (while blocked) and Skip (sequencer ops).
    let mut row = div().flex().flex_row().flex_wrap().gap_2();
    let mut has_secondary = false;
    if !can_continue {
        let next = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
            this.conflict_nav_unresolved(1);
            cx.notify();
        });
        row = row.child(secondary_button(
            "conflict-next".to_string(),
            Msg::ConflictNextConflict.t(),
            true,
            Some(next),
        ));
        has_secondary = true;
    }
    if is_sequencer {
        let skip = cx.listener(|this, _e: &gpui::ClickEvent, _w, cx| {
            this.conflict_skip(cx);
            cx.notify();
        });
        row = row.child(secondary_button(
            "conflict-skip".to_string(),
            Msg::ConflictSkip.t(),
            true,
            Some(skip),
        ));
        has_secondary = true;
    }
    if has_secondary {
        col = col.child(row);
    }

    col.into_any_element()
}

/// A small status line used throughout the dashboard.
fn dash_note(text: &str, color: u32) -> gpui::AnyElement {
    div()
        .text_size(theme::scaled_px(10.))
        .text_color(rgb(color))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// A Keynote-style icon button for a file card: an icon-only, rounded, hover-lit
/// hit target. `on_down` fires on mouse-down (use a `cx.listener` that calls
/// `cx.stop_propagation()` so the card's row-select doesn't also run).
fn card_icon<H>(
    id: String,
    icon: gpui_component::IconName,
    tip: &'static str,
    on_down: H,
) -> gpui::Stateful<gpui::Div>
where
    H: Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + 'static,
{
    div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .justify_center()
        .w(theme::scaled_px(22.))
        .h(theme::scaled_px(22.))
        .rounded_md()
        .text_color(rgb(theme().text_sub))
        .hover(|s| {
            s.bg(rgb(theme().bg_row_alt))
                .text_color(rgb(theme().text_main))
                .cursor_pointer()
        })
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
        .on_mouse_down(MouseButton::Left, on_down)
        .child(
            gpui_component::Icon::new(icon)
                .with_size(gpui_component::Size::Size(theme::scaled_px(14.))),
        )
}

/// Actions for the per-file "…" overflow menu.
#[derive(Clone)]
enum ConflictFileAction {
    OpenExternally,
    CopyPath,
    CopyGitCommand,
    OpenTerminal,
}

/// The per-file "…" overflow menu (T-CONFLICT-DASH-022): the full set of
/// file/repo actions (the card icons are quick shortcuts for the top two).
/// Rendered with the shared `menu_overlay` machinery so it gets the same
/// viewport clamping (stays on-screen near the right edge) and styling as the
/// commit / branch / stash context menus, anchored at the click position.
pub fn render_file_menu(
    mode: &ConflictMode,
    idx: usize,
    pos: gpui::Point<gpui::Pixels>,
    window: &mut Window,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    // Header = the file the menu acts on.
    let header = mode
        .session
        .files
        .get(idx)
        .map(|f| SharedString::from(f.path.to_string_lossy().into_owned()))
        .unwrap_or_default();

    let item = |action: ConflictFileAction, label: &str| MenuItem {
        action,
        label: SharedString::from(label.to_string()),
        state: ItemState::Enabled,
        dangerous: false,
    };
    let groups = vec![
        MenuGroup {
            title: None,
            items: vec![
                item(
                    ConflictFileAction::OpenExternally,
                    Msg::ConflictExternalTool.t(),
                ),
                item(ConflictFileAction::CopyPath, Msg::ConflictCopyPath.t()),
            ],
        },
        MenuGroup {
            title: None,
            items: vec![
                item(
                    ConflictFileAction::CopyGitCommand,
                    Msg::ConflictCopyGitCommand.t(),
                ),
                item(
                    ConflictFileAction::OpenTerminal,
                    Msg::ConflictOpenTerminal.t(),
                ),
            ],
        },
    ];

    let on_dismiss = |this: &mut KagiApp, _w: &mut Window, _cx: &mut Context<KagiApp>| {
        this.conflict.file_menu = None;
    };
    let on_select = move |this: &mut KagiApp,
                          action: ConflictFileAction,
                          window: &mut Window,
                          cx: &mut Context<KagiApp>| {
        this.conflict.file_menu = None;
        match action {
            ConflictFileAction::OpenExternally => this.conflict_open_external_tool(idx, cx),
            ConflictFileAction::CopyPath => this.conflict_copy_path(idx, cx),
            ConflictFileAction::CopyGitCommand => this.conflict_copy_git_command(cx),
            ConflictFileAction::OpenTerminal => this.conflict_open_terminal(window, cx),
        }
    };

    menu_overlay::render_menu_overlay(
        "conflict-file-context-menu",
        "conflict-file-menu-item",
        220.0,
        "Danger",
        pos,
        header,
        groups,
        on_dismiss,
        on_select,
        window,
        cx,
    )
}

/// A left-aligned secondary (utility / navigation) button with clear button
/// affordance — outlined rather than ghost so it never reads as plain text.
fn secondary_button<H>(id: String, label: &str, enabled: bool, handler: Option<H>) -> Button
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    let mut btn = Button::new(SharedString::from(id))
        .label(SharedString::from(label.to_string()))
        .outline()
        .small()
        .disabled(!enabled);
    if enabled {
        if let Some(h) = handler {
            btn = btn.on_click(h);
        }
    }
    btn
}

/// Two sections: Conflicted Files and Resolved Files, each row with a kind badge.
fn dash_sections(mode: &ConflictMode, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let conflicted: Vec<usize> = mode
        .session
        .files
        .iter()
        .enumerate()
        .filter(|(_, f)| f.status == ConflictStatus::Unresolved)
        .map(|(i, _)| i)
        .collect();
    let resolved: Vec<usize> = mode
        .session
        .files
        .iter()
        .enumerate()
        .filter(|(_, f)| f.status != ConflictStatus::Unresolved)
        .map(|(i, _)| i)
        .collect();

    div()
        .flex()
        .flex_col()
        .child(section(
            mode,
            Msg::ConflictSectionConflicted.t(),
            &conflicted,
            Msg::ConflictNoConflictedFiles.t(),
            true,
            cx,
        ))
        .child(section(
            mode,
            Msg::ConflictSectionResolved.t(),
            &resolved,
            Msg::ConflictNoResolvedFiles.t(),
            false,
            cx,
        ))
        .into_any_element()
}

/// A labelled section listing the given file indices.  `is_conflicted` rows are
/// "activatable": clicking sets the editing-file state (W32 opens the editor).
fn section(
    mode: &ConflictMode,
    title: &str,
    indices: &[usize],
    empty_msg: &str,
    _is_conflicted: bool,
    cx: &mut Context<KagiApp>,
) -> gpui::AnyElement {
    let mut col = div().flex().flex_col().child(
        div()
            .px(theme::scaled_px(12.))
            .py(theme::scaled_px(6.))
            .text_size(theme::scaled_px(10.))
            .text_color(rgb(theme().text_label))
            .child(SharedString::from(format!("{} ({})", title, indices.len()))),
    );

    if indices.is_empty() {
        col = col.child(
            div()
                .px(theme::scaled_px(12.))
                .py(theme::scaled_px(4.))
                .text_size(theme::scaled_px(11.))
                .text_color(rgb(theme().text_muted))
                .child(SharedString::from(empty_msg.to_string())),
        );
        return col.into_any_element();
    }

    for &idx in indices {
        let file = &mode.session.files[idx];
        let selected = mode.selected_file == Some(idx);
        let path_str = file.path.to_string_lossy().into_owned();
        let kind = file.kind;
        let (status_color, status_text) = match file.status {
            ConflictStatus::Unresolved => (theme().color_blocker, Msg::ConflictUnresolved.t()),
            ConflictStatus::Resolved => (theme().color_success, Msg::ConflictResolvedShort.t()),
            ConflictStatus::NeedsReview => (theme().color_warning, Msg::ConflictNeedsReview.t()),
        };

        // Row click → select; for content conflicts conflict_select_file also
        // opens the hunk-level Conflict Editor (W32). Resolved rows just preview.
        let row_click = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
            this.conflict_select_file(idx);
            cx.notify();
        });

        // Per-card actions (T-CONFLICT-DASH-022). Each acts on THIS row's file
        // (not the selected one) and stops propagation so the row-select / editor
        // open doesn't also fire. "…" opens the overflow menu at the cursor.
        let more = cx.listener(move |this, e: &gpui::MouseDownEvent, _w, cx| {
            cx.stop_propagation();
            this.conflict.file_menu = Some((idx, e.position));
            cx.notify();
        });
        let ext = cx.listener(move |this, _e: &gpui::MouseDownEvent, _w, cx| {
            cx.stop_propagation();
            this.conflict_open_external_tool(idx, cx);
            cx.notify();
        });
        let copy = cx.listener(move |this, _e: &gpui::MouseDownEvent, _w, cx| {
            cx.stop_propagation();
            this.conflict_copy_path(idx, cx);
            cx.notify();
        });

        let row_el = div()
            .id(SharedString::from(format!("conflict-row-{}", idx)))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .mx(theme::scaled_px(6.))
            .px(theme::scaled_px(6.))
            .py(theme::scaled_px(6.))
            .rounded_md()
            .cursor_pointer()
            .when(selected, |s| s.bg(rgb(theme().selected)))
            .hover(|s| s.bg(rgb(theme().bg_row_alt)))
            .on_click(row_click)
            // Left: file name + status/kind (flex-shrinks; name truncates).
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_1()
                    .child(
                        div()
                            .text_size(theme::scaled_px(12.))
                            .text_color(rgb(theme().text_main))
                            .truncate()
                            .child(SharedString::from(path_str)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(theme::scaled_px(10.))
                                    .text_color(rgb(status_color))
                                    .child(SharedString::from(status_text)),
                            )
                            .child(kind_badge(kind)),
                    ),
            )
            // Right: Keynote-style icon group — "…" / open externally / copy path.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(theme::scaled_px(1.))
                    .flex_shrink_0()
                    .child(card_icon(
                        format!("conflict-more-{}", idx),
                        gpui_component::IconName::Ellipsis,
                        Msg::ConflictMore.t(),
                        more,
                    ))
                    .child(card_icon(
                        format!("conflict-ext-{}", idx),
                        gpui_component::IconName::ExternalLink,
                        Msg::ConflictExternalTool.t(),
                        ext,
                    ))
                    .child(card_icon(
                        format!("conflict-copy-{}", idx),
                        gpui_component::IconName::Copy,
                        Msg::ConflictCopyPath.t(),
                        copy,
                    )),
            );

        col = col.child(row_el);
    }

    col.into_any_element()
}

/// A conflict-type badge (ADR-0065): both modified / rename-delete / etc.
fn kind_badge(kind: ConflictKind) -> gpui::AnyElement {
    div()
        .px(theme::scaled_px(5.))
        .py(px(1.))
        .rounded_md()
        .border_1()
        .border_color(rgb(theme().text_muted))
        .text_size(theme::scaled_px(9.))
        .text_color(rgb(theme().text_muted))
        .child(SharedString::from(kind_tag(kind)))
        .into_any_element()
}

/// A dashboard action button.  `enabled == false` renders muted and attaches no
/// click handler (e.g. the Continue gate).
fn action_button<H>(
    label: &str,
    accent: u32,
    enabled: bool,
    handler: Option<H>,
    cx: &gpui::App,
) -> Button
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    let label = label.to_string();
    let mut btn = Button::new(SharedString::from(format!("conflict-act-{}", label)))
        .label(SharedString::from(label))
        .small()
        .disabled(!enabled);
    btn = apply_accent(btn, accent, cx);
    if enabled {
        if let Some(h) = handler {
            btn = btn.on_click(h);
        }
    }
    btn
}

// ────────────────────────────────────────────────────────────
// Center pane: per-file choose + Result preview (W30; W32 supersedes)
// ────────────────────────────────────────────────────────────

/// The center pane: the selected file's choose buttons + Result preview.  When
/// the W32 Conflict Editor lands it renders here for `editing_file`; until then
/// this MVP keeps the file-granularity choose + preview.
fn render_center(mode: &ConflictMode, cx: &mut Context<KagiApp>) -> gpui::AnyElement {
    let Some(idx) = mode.selected_file else {
        return div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .flex_grow()
            .h_full()
            .child(
                div()
                    .text_size(theme::scaled_px(13.))
                    .text_color(rgb(theme().text_muted))
                    .child(SharedString::from(Msg::ConflictSelectFile.t())),
            )
            .into_any_element();
    };
    let Some(file) = mode.session.files.get(idx) else {
        return div().flex_grow().h_full().into_any_element();
    };

    let labels = mode.labels();
    let is_binary = file.kind == ConflictKind::Binary;
    let path = file.path.clone();

    let keep_current_label = format!("{} ({})", Msg::ConflictKeepCurrent.t(), labels.current.name);
    let take_incoming_label = format!(
        "{} ({})",
        Msg::ConflictTakeIncoming.t(),
        labels.incoming.name
    );
    let keep_both_label = Msg::ConflictKeepBoth.t().to_string();

    let p1 = path.clone();
    let keep_current = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_apply_choice(&p1, kagi_git::resolution::ResolutionChoice::Current, cx);
        cx.notify();
    });
    let p2 = path.clone();
    let take_incoming = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_apply_choice(&p2, kagi_git::resolution::ResolutionChoice::Incoming, cx);
        cx.notify();
    });
    let p3 = path.clone();
    let keep_both = cx.listener(move |this, _e: &gpui::ClickEvent, _w, cx| {
        this.conflict_apply_choice(
            &p3,
            kagi_git::resolution::ResolutionChoice::BothCurrentFirst,
            cx,
        );
        cx.notify();
    });

    let mut choose_row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_2()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(8.))
        .border_b_1()
        .border_color(rgb(theme().surface))
        .child(choose_button(
            keep_current_label,
            theme().color_branch,
            keep_current,
            cx,
        ))
        .child(choose_button(
            take_incoming_label,
            theme().color_remote,
            take_incoming,
            cx,
        ));

    if !is_binary && file.kind == ConflictKind::Content {
        choose_row = choose_row.child(choose_button(
            keep_both_label,
            theme().text_sub,
            keep_both,
            cx,
        ));
    }

    let preview = render_preview(mode, &path, is_binary);

    div()
        .flex()
        .flex_col()
        .flex_grow()
        .h_full()
        .child(choose_row)
        .child(preview)
        .into_any_element()
}

fn choose_button<H>(label: String, accent: u32, handler: H, cx: &gpui::App) -> Button
where
    H: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    KagiButton::accent(
        SharedString::from(format!("conflict-choose-{}", label)),
        SharedString::from(label),
        accent,
        cx,
    )
    .small()
    .on_click(handler)
}

/// Result preview scroll box (MVP: plain text, no syntax / diff coloring).
fn render_preview(
    mode: &ConflictMode,
    path: &std::path::Path,
    is_binary: bool,
) -> gpui::AnyElement {
    let body: gpui::AnyElement = if is_binary {
        div()
            .text_size(theme::scaled_px(12.))
            .text_color(rgb(theme().text_muted))
            .child(SharedString::from(Msg::ConflictBinaryNoPreview.t()))
            .into_any_element()
    } else if let Some(text) = mode.buffer.resolved_text(path) {
        let mut col = div().flex().flex_col();
        for line in text.split('\n') {
            col = col.child(
                div()
                    .text_size(theme::scaled_px(12.))
                    .text_color(rgb(theme().text_main))
                    .child(SharedString::from(line.to_string())),
            );
        }
        col.into_any_element()
    } else {
        div()
            .text_size(theme::scaled_px(12.))
            .text_color(rgb(theme().text_sub))
            .child(SharedString::from(Msg::ConflictPreviewHint.t()))
            .into_any_element()
    };

    div()
        .id("conflict-preview")
        .flex()
        .flex_col()
        .flex_grow()
        .w_full()
        .overflow_y_scroll()
        .px(theme::scaled_px(12.))
        .py(theme::scaled_px(8.))
        .child(
            div()
                .text_size(theme::scaled_px(11.))
                .text_color(rgb(theme().text_label))
                .pb(px(4.))
                .child(SharedString::from(Msg::ConflictResultPreview.t())),
        )
        .child(body)
        .into_any_element()
}

// ────────────────────────────────────────────────────────────
// Unit tests — the pure Conflict Mode gate / status / heading logic.
// (The render functions need a gpui window and are exercised manually.)
// ────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .expect("git runs")
            .success();
        assert!(ok, "git {:?} failed", args);
    }

    /// `git merge` is allowed to exit non-zero (the conflict).
    fn git_allow_fail(dir: &std::path::Path, args: &[&str]) {
        let _ = Command::new("git").args(args).current_dir(dir).status();
    }

    /// Build a real merge-conflict repo and return its TempDir.
    fn merge_conflict_repo() -> TempDir {
        let td = TempDir::new().unwrap();
        let p = td.path();
        git(p, &["init", "-q", "-b", "main"]);
        git(p, &["config", "user.email", "t@e.com"]);
        git(p, &["config", "user.name", "t"]);
        std::fs::write(p.join("f.txt"), "line1\nline2\nline3\n").unwrap();
        git(p, &["add", "f.txt"]);
        git(p, &["commit", "-qm", "base"]);
        git(p, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(p.join("f.txt"), "line1\nFEATURE\nline3\n").unwrap();
        git(p, &["commit", "-qam", "feature"]);
        git(p, &["checkout", "-q", "main"]);
        std::fs::write(p.join("f.txt"), "line1\nMAIN\nline3\n").unwrap();
        git(p, &["commit", "-qam", "main"]);
        git_allow_fail(p, &["merge", "feature"]);
        td
    }

    /// Build a ConflictMode from a repo path, mirroring `detect_conflict_mode`.
    fn detect(repo_path: &std::path::Path, branch: &str) -> ConflictMode {
        let backend = kagi_git::Backend::open(repo_path).unwrap();
        let mut session = backend.detect_conflict_session().expect("conflict session");
        let buffer = backend.resolution_buffer_from_repo().unwrap();
        let residue = buffer.files_with_marker_residue();
        for f in &mut session.files {
            f.status = if buffer.has_resolution(&f.path) {
                if residue.contains(&f.path) {
                    ConflictStatus::NeedsReview
                } else {
                    ConflictStatus::Resolved
                }
            } else {
                ConflictStatus::Unresolved
            };
        }
        ConflictMode {
            session,
            buffer,
            current_branch: branch.to_string(),
            selected_file: Some(0),
            editing_file: None,
            abort_armed: false,
        }
    }

    #[test]
    fn continue_gate_blocks_until_resolved() {
        let td = merge_conflict_repo();
        let mut mode = detect(td.path(), "main");

        // Detected: one unresolved content conflict → continue is blocked.
        assert_eq!(mode.session.total_count(), 1);
        assert_eq!(mode.resolved_count(), 0);
        assert_eq!(mode.conflicted_count(), 1);
        assert!(!mode.can_continue(), "gate must block while unresolved");
        assert_eq!(
            mode.continue_blocker(),
            Some(Msg::ConflictBlockerUnresolved)
        );

        // Apply a side choice → resolved, no marker residue → gate opens.
        let path = mode.session.files[0].path.clone();
        mode.buffer
            .apply_choice(&path, kagi_git::ResolutionChoice::Current)
            .unwrap();
        // Recompute status as the UI handler does.
        let residue = mode.buffer.files_with_marker_residue();
        mode.session.files[0].status = if residue.contains(&path) {
            ConflictStatus::NeedsReview
        } else {
            ConflictStatus::Resolved
        };

        assert_eq!(mode.resolved_count(), 1);
        assert!(
            mode.can_continue(),
            "gate must open once all files resolved"
        );
        assert_eq!(mode.continue_blocker(), None);
    }

    #[test]
    fn marker_residue_keeps_gate_closed() {
        let td = merge_conflict_repo();
        let mut mode = detect(td.path(), "main");
        let path = mode.session.files[0].path.clone();

        // A manual edit that still contains conflict markers is residue.
        mode.buffer
            .set_manual_text(
                &path,
                "<<<<<<< HEAD\nMAIN\n=======\nFEATURE\n>>>>>>> feature\n",
            )
            .unwrap();
        assert!(mode.buffer.has_resolution(&path));
        assert!(
            !mode.can_continue(),
            "marker residue must keep the continue gate closed"
        );
        assert_eq!(mode.continue_blocker(), Some(Msg::ConflictBlockerMarker));
    }

    #[test]
    fn heading_uses_roles_not_ours_theirs() {
        let td = merge_conflict_repo();
        let mode = detect(td.path(), "main");
        let heading = op_summary(&mode);
        let lower = heading.to_lowercase();
        assert!(
            !lower.contains("ours"),
            "heading leaked 'ours': {}",
            heading
        );
        assert!(
            !lower.contains("theirs"),
            "heading leaked 'theirs': {}",
            heading
        );
        // Merge summary names the current branch verbatim.
        assert!(
            heading.contains("main"),
            "summary should name current branch: {}",
            heading
        );
    }
}
