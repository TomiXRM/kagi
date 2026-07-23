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
    WeakEntity, Window,
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

/// ADR-0118 (Phase 5.2) / T-ENTITY-CONFLICT-001: the Conflict-resolution panel
/// promoted to its own `Entity<T>` (Stage 1, mirroring the ADR-0117 FileHistory
/// fat-entity template).
///
/// Holds the former `ConflictState` view data (detected mode, the open editor
/// file, the A/B/Result inputs, and splits/geometry) plus the plumbing the entity
/// needs to drive itself and call back to the parent. The entity has its own
/// `cx.notify()` scope, so buffer-only / view-only interactions (hunk/side/line
/// choices, scroll, split drag, file-menu open/close, reset-all arm,
/// result-editing toggle, file selection) re-render only this subtree.
///
/// # Re-entrancy invariant (CRITICAL)
/// A `ConflictView` listener leases this entity. The four Backend actions
/// (`conflict_continue`/`abort`/`skip`/`editor_save`) and the snapshot-reading
/// context actions (open-external-tool / copy-path / copy-git-command /
/// open-terminal) call `KagiApp` methods that read/write `app.conflict` (directly
/// or via `reload()`→`detect_conflict_mode`→`apply_conflict_detect`). Calling any
/// of those synchronously from a leased listener re-leases this entity and
/// panics ("already borrowed"). So every such listener DEFERS to the parent via
/// `cx.spawn_in(window, …)` + `weak_app.update_in(acx, …)`, by which time the
/// listener has returned and the lease is released.
///
/// Parent-owned (NOT moved here): `conflict_merge_pending` (Stage 0d; read by the
/// render gate / watcher / commit flow) and `detected_for` (per-repo run-once
/// guard). The status-bar `conflict_count` badge and `merge_commit_ready` also
/// stay on `KagiApp` (separate concerns).
pub struct ConflictView {
    /// `Some(_)` while a conflict/merge is in progress (set by
    /// `detect_conflict_mode` via `apply_conflict_detect`). The repository is
    /// untouched until Continue / Abort execute through the existing plan
    /// pipeline.
    pub mode: Option<ConflictMode>,
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
    /// editor's split region, for absolute-coordinate divider dragging. Shared
    /// with `KagiApp` (the *same* `Rc<Cell>`): the render writes the measured
    /// bounds; the root `on_drag_move` handler (on `KagiApp`) reads them and
    /// updates the split via `entity.update`. Mirrors FileHistory `geom` sharing.
    pub geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    /// T-CONFLICT-UI-003: measured (left, right) screen-px bounds of the A·B
    /// row, for the vertical A|B divider drag. Shared with `KagiApp` as above.
    pub ab_geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    /// T-CONFLICT-UX-010/012: index (among conflict hunks) of the focused hunk
    /// in the per-hunk Conflict Editor.
    pub selected_hunk: usize,
    /// ADR-0070: shared A/B uniform-list scroll handle for synchronized vertical
    /// scrolling in the Conflict Editor.
    pub ab_scroll_handle: UniformListScrollHandle,
    /// T-CONFLICT-DASH-022: the open per-file "…" context menu, as
    /// `(file_index, anchor)` where `anchor` is the click position in window px.
    /// `None` when no menu is open. Rendered as a top-level overlay.
    pub file_menu: Option<(usize, gpui::Point<gpui::Pixels>)>,
    /// Weak back-reference to the parent. Used ONLY from event/listener closures
    /// (deferred Backend / context actions) — NEVER read in a `Render` path
    /// (would re-enter the parent and panic).
    pub(crate) app: WeakEntity<KagiApp>,
    /// Repo root for this conflict session. Captured when the entity is created
    /// in `apply_conflict_detect`; constant for the entity's life (the entity is
    /// dropped on conflict-clear / repo or tab switch). Used for the read-only
    /// marker materialization in `conflict_open_editor`.
    pub(crate) repo_path: PathBuf,
}

impl ConflictView {
    /// Construct the entity for a freshly-detected conflict. Created in
    /// `KagiApp::apply_conflict_detect` via `cx.new`; the caller assigns the
    /// `mode` / `editing` immediately after (or uses [`ConflictView::set_detected`]).
    pub fn new(app: WeakEntity<KagiApp>, repo_path: PathBuf) -> Self {
        Self {
            mode: None,
            editing: None,
            editing_before_text: HashMap::new(),
            editor_inputs: None,
            result_editing: false,
            reset_all_armed: false,
            ab_split: super::CONFLICT_AB_DEFAULT,
            result_split: super::CONFLICT_RESULT_DEFAULT,
            geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            ab_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            selected_hunk: 0,
            ab_scroll_handle: UniformListScrollHandle::new(),
            file_menu: None,
            app,
            repo_path,
        }
    }

    /// Update the A|B split ratio from the root divider-drag handler (lives on
    /// `KagiApp`, which reads the shared `ab_geom` cell, then pushes the ratio
    /// into the entity via `entity.update`). Child-scoped repaint. Mirrors the
    /// FileHistory `set_split` precedent.
    pub fn set_ab_split(&mut self, ratio: f32, cx: &mut Context<Self>) {
        if (ratio - self.ab_split).abs() > 0.001 {
            self.ab_split = ratio;
            cx.notify();
        }
    }

    /// Update the A·B / Result split ratio from the root divider-drag handler.
    pub fn set_result_split(&mut self, ratio: f32, cx: &mut Context<Self>) {
        if (ratio - self.result_split).abs() > 0.001 {
            self.result_split = ratio;
            cx.notify();
        }
    }

    /// Apply a per-file side choice to the in-memory resolution buffer, then
    /// recompute that file's status.  The repository is untouched (in-memory
    /// first); the buffer is autosaved so the partial resolution survives.
    ///
    /// On success this refreshes the file's status and autosaves; logging /
    /// toasting is the caller's responsibility (the `KagiApp` wrapper).
    ///
    /// Returns `None` when there is no active conflict mode — a no-op the caller
    /// must NOT log (mirrors the original early-return-without-logging path);
    /// `Some(Ok(()))` on apply, `Some(Err(_))` on buffer failure.
    pub fn apply_choice(
        &mut self,
        path: &std::path::Path,
        choice: kagi_git::ResolutionChoice,
    ) -> Option<Result<(), kagi_git::GitError>> {
        let c = self.mode.as_mut()?;
        if let Err(e) = c.buffer.apply_choice(path, choice) {
            return Some(Err(e));
        }
        // Refresh status for this file from the buffer.
        let residue = c.buffer.files_with_marker_residue();
        if let Some(f) = c.session.files.iter_mut().find(|f| f.path == path) {
            f.status = if residue.contains(&f.path) {
                kagi_git::ConflictStatus::NeedsReview
            } else {
                kagi_git::ConflictStatus::Resolved
            };
        }
        // Autosave (ADR-0057): never lose a partial resolution.
        let _ = c.buffer.autosave();
        Some(Ok(()))
    }

    pub fn set_file_side(
        &mut self,
        path: &std::path::Path,
        side: kagi_git::resolution::SelectionSide,
        taken: bool,
    ) {
        let Some(c) = self.mode.as_mut() else {
            return;
        };
        if c.buffer.set_file_side_selection(path, side, taken) {
            self.after_selection_change(path, None);
        }
    }

    pub fn set_hunk_side(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        side: kagi_git::resolution::SelectionSide,
        taken: bool,
    ) {
        let Some(c) = self.mode.as_mut() else {
            return;
        };
        if c.buffer
            .set_hunk_side_selection(path, hunk_index, side, taken)
        {
            self.after_selection_change(path, Some(hunk_index));
        }
    }

    pub fn set_hunk_line(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        side: kagi_git::resolution::SelectionSide,
        line_index: usize,
        taken: bool,
    ) {
        let Some(c) = self.mode.as_mut() else {
            return;
        };
        if c.buffer
            .set_hunk_line_selection(path, hunk_index, side, line_index, taken)
        {
            self.after_selection_change(path, Some(hunk_index));
        }
    }

    pub fn set_hunk_order(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        order: kagi_git::resolution::LineOrder,
    ) {
        let Some(c) = self.mode.as_mut() else {
            return;
        };
        if c.buffer.set_hunk_line_order(path, hunk_index, order) {
            self.after_selection_change(path, Some(hunk_index));
        }
    }

    fn after_selection_change(&mut self, path: &std::path::Path, selected_hunk: Option<usize>) {
        self.reset_all_armed = false;
        if let Some(hunk) = selected_hunk {
            self.selected_hunk = hunk;
        }
        if let Some(i) = self.editor_inputs.as_mut() {
            i.content_sig = 0;
        }
        let Some(c) = self.mode.as_mut() else {
            return;
        };
        let residue = c.buffer.files_with_marker_residue();
        if let Some(f) = c.session.files.iter_mut().find(|f| f.path == path) {
            f.status = if !c.buffer.has_resolution(path) {
                kagi_git::ConflictStatus::Unresolved
            } else if residue.contains(&f.path) {
                kagi_git::ConflictStatus::NeedsReview
            } else {
                kagi_git::ConflictStatus::Resolved
            };
        }
        let _ = c.buffer.autosave();
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

    // Paths eligible for "Mark all clean files resolved": files with no marker
    // residue AND a resolvable resolution draft (ADR-0063).  We approximate
    // "index-resolvable" with "has a clean buffer resolution" — the plan
    // pipeline re-checks the live index before writing.

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

// ────────────────────────────────────────────────────────────
// ADR-0118 / T-ENTITY-CONFLICT-001: ConflictView entity render + listeners.
//
// The entity renders the conflict BODY (center 3-pane editor + right Dashboard).
// The persistent banner is rendered separately by the parent (it sits under the
// header, a different flex_col position than the body, and is shown even during
// `conflict_merge_pending` while the body is not) via the free `render_banner`
// below, fed a cloned `ConflictMode` so the entity is never double-rendered.
// ────────────────────────────────────────────────────────────

impl Render for ConflictView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // entity-exists ⟺ mode-is-some in practice (the parent drops the entity on
        // clear), but guard defensively so a transient empty entity paints nothing.
        match self.mode.clone() {
            Some(mode) => {
                let chrome = self.editor_chrome();
                render_body(&mode, &chrome, cx)
            }
            None => div().into_any_element(),
        }
    }
}

impl ConflictView {
    /// Build the [`EditorChrome`] the render functions thread through, from this
    /// entity's own fields (the editors + split ratios + Result mode + geometry
    /// all live on the entity now).
    fn editor_chrome(&self) -> EditorChrome {
        EditorChrome {
            inputs: self.editor_inputs.as_ref().map(|i| EditorInputs {
                path: i.path.clone(),
                result: i.result.clone(),
            }),
            ab_scroll: self.ab_scroll_handle.clone(),
            result_editing: self.result_editing,
            reset_all_armed: self.reset_all_armed,
            ab_split: self.ab_split,
            result_split: self.result_split,
            selected_hunk: self.selected_hunk,
            geom: self.geom.clone(),
            ab_geom: self.ab_geom.clone(),
        }
    }
}

// ────────────────────────────────────────────────────────────
// ADR-0118: entity-internal Conflict actions (buffer-only / view-only).
//
// Moved off `KagiApp` into the entity. Each mutates `self` (the ConflictView) and
// the listener that calls it does its own `cx.notify()` (child-scoped repaint).
// NONE of these touch `reload()` / `detect_conflict_mode` / the parent
// `app.conflict`, so they are safe to call synchronously from a leased listener.
// Toasts (rare error paths) marshal to the parent via the weak handle.
// ────────────────────────────────────────────────────────────

impl ConflictView {
    /// Open the dedicated Conflict Editor for the conflicting file at `path`.
    /// Builds (idempotently) the per-file `HunkModel` from the repo's zdiff3
    /// materialization, then sets `editing`. Read-only repo open via `repo_path`.
    /// (Moved from `KagiApp::conflict_open_editor`, sourcing `repo_path` from self.)
    pub fn conflict_open_editor(&mut self, path: &std::path::Path) {
        // Materialize the markers (needs the repo) and build the hunk model.
        if let Ok(repo) = kagi_git::Backend::open(&self.repo_path) {
            if let Some(c) = self.mode.as_mut() {
                if let Some(markers) = repo.materialized_markers(&c.buffer, path) {
                    c.buffer.ensure_hunks(path, &markers);
                }
            }
        }
        // Keep the Dashboard selection in sync so back/forth is coherent.
        if let Some(c) = self.mode.as_mut() {
            if let Some(idx) = c.session.files.iter().position(|f| f.path == path) {
                c.selected_file = Some(idx);
                c.editing_file = Some(idx);
            }
        }
        self.editing = Some(path.to_path_buf());
    }

    /// T-CONFLICT-UX-010/012: set the focused hunk (selected-hunk highlight).
    pub fn conflict_editor_select_hunk(&mut self, hunk_index: usize) {
        self.selected_hunk = hunk_index;
    }

    pub fn conflict_editor_set_file_side(
        &mut self,
        path: &std::path::Path,
        side: kagi_git::resolution::SelectionSide,
        taken: bool,
    ) {
        self.set_file_side(path, side, taken);
    }

    pub fn conflict_editor_set_hunk_side(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        side: kagi_git::resolution::SelectionSide,
        taken: bool,
    ) {
        self.set_hunk_side(path, hunk_index, side, taken);
    }

    pub fn conflict_editor_set_hunk_line(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        side: kagi_git::resolution::SelectionSide,
        line_index: usize,
        taken: bool,
    ) {
        self.set_hunk_line(path, hunk_index, side, line_index, taken);
    }

    pub fn conflict_editor_set_hunk_order(
        &mut self,
        path: &std::path::Path,
        hunk_index: usize,
        order: kagi_git::resolution::LineOrder,
    ) {
        self.set_hunk_order(path, hunk_index, order);
    }

    /// T-CONFLICT-POLISH-042: "Reset all" two-stage confirm. First click arms,
    /// second performs the reset. (Moved from `KagiApp`.)
    pub fn conflict_editor_reset_all_request(&mut self, path: &std::path::Path) {
        if self.reset_all_armed {
            self.reset_all_armed = false;
            self.conflict_editor_reset_all(path);
        } else {
            self.reset_all_armed = true;
        }
    }

    /// T-CONFLICT-UX-015: toggle the Result pane between Preview / Edit mode.
    pub fn conflict_editor_toggle_result_mode(&mut self) {
        self.result_editing = !self.result_editing;
        // Force the inputs to re-sync (mode is part of the content signature).
        if let Some(i) = self.editor_inputs.as_mut() {
            i.content_sig = 0;
        }
    }

    /// Reset every hunk of `path` to unresolved (toolbar "Reset all").
    pub fn conflict_editor_reset_all(&mut self, path: &std::path::Path) {
        // Force the editor inputs to re-sync after the reset.
        if let Some(i) = self.editor_inputs.as_mut() {
            i.content_sig = 0;
        }
        let Some(c) = self.mode.as_mut() else {
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
                kagi_git::ConflictStatus::NeedsReview
            } else if c.buffer.has_resolution(path) {
                kagi_git::ConflictStatus::Resolved
            } else {
                kagi_git::ConflictStatus::Unresolved
            };
        }
        let _ = c.buffer.autosave();
    }

    /// Move to the next / previous unresolved hunk by selecting an adjacent
    /// still-conflicted file and re-opening the editor on it.
    pub fn conflict_editor_nav_hunk(&mut self, dir: i32) {
        self.conflict_nav_unresolved(dir);
        if let Some(c) = self.mode.as_ref() {
            if let Some(idx) = c.selected_file {
                if let Some(f) = c.session.files.get(idx) {
                    let p = f.path.clone();
                    self.conflict_open_editor(&p);
                }
            }
        }
    }

    /// Select a conflicting file (open its detail + Result preview). Activating a
    /// content conflict also opens the dedicated hunk-level Conflict Editor.
    pub fn conflict_select_file(&mut self, idx: usize) {
        let mut open_path: Option<PathBuf> = None;
        if let Some(c) = self.mode.as_mut() {
            if let Some(f) = c.session.files.get(idx) {
                c.selected_file = Some(idx);
                if f.kind == kagi_git::ConflictKind::Content {
                    open_path = Some(f.path.clone());
                }
            }
        }
        if let Some(p) = open_path {
            self.conflict_open_editor(&p);
        }
    }

    /// Move the selection to the previous / next unresolved file, wrapping.
    pub fn conflict_nav_unresolved(&mut self, dir: i32) {
        let Some(c) = self.mode.as_mut() else {
            return;
        };
        let n = c.session.files.len();
        if n == 0 {
            return;
        }
        let start = c.selected_file.unwrap_or(0);
        for step in 1..=n {
            let i = if dir >= 0 {
                (start + step) % n
            } else {
                (start + n - (step % n)) % n
            };
            if c.session.files[i].status == kagi_git::ConflictStatus::Unresolved {
                c.selected_file = Some(i);
                return;
            }
        }
        let i = if dir >= 0 {
            (start + 1) % n
        } else {
            (start + n - 1) % n
        };
        c.selected_file = Some(i);
    }

    /// Apply a per-file side choice to the in-memory resolution buffer, emit the
    /// `[kagi]` contract line, and (on a buffer error) marshal an error toast to
    /// the parent. The buffer work itself touches no parent state, so it stays
    /// synchronous; only the rare error toast defers.
    pub fn conflict_apply_choice(
        &mut self,
        path: &std::path::Path,
        choice: kagi_git::ResolutionChoice,
        cx: &mut Context<Self>,
    ) {
        match self.apply_choice(path, choice) {
            // No active conflict mode — no-op, no `[kagi]` line (matches original).
            None => {}
            Some(Ok(())) => {
                eprintln!(
                    "[kagi] conflict-mode: choice {} for {}",
                    match choice {
                        kagi_git::ResolutionChoice::Current => "current",
                        kagi_git::ResolutionChoice::Incoming => "incoming",
                        kagi_git::ResolutionChoice::BothCurrentFirst => "both(current-first)",
                        kagi_git::ResolutionChoice::BothIncomingFirst => "both(incoming-first)",
                    },
                    path.display()
                );
            }
            Some(Err(e)) => {
                eprintln!(
                    "[kagi] conflict-mode: choice failed for {}: {}",
                    path.display(),
                    e
                );
                self.marshal_error_toast(format!("{}", e), cx);
            }
        }
    }

    /// Entry point for "Open external tool" toolbar button (launch is W33). Emits
    /// the contract `eprintln!` line and marshals the info toast to the parent.
    pub fn conflict_editor_open_external(
        &mut self,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        eprintln!(
            "[kagi] conflict-editor: external tool requested for {} (launch is W33)",
            path.display()
        );
        let msg = format!(
            "External merge tool launch is not wired yet ({}).",
            path.display()
        );
        self.marshal_info_toast(msg, cx);
    }

    /// Two-stage Abort arming (ADR-0067). Returns `true` when already armed (the
    /// caller should EXECUTE the abort — which reloads, so it must defer to the
    /// parent); `false` when this click only ARMED it (no reload, repaint only).
    pub fn abort_request_arm(&mut self) -> bool {
        let armed = self.mode.as_ref().map(|c| c.abort_armed).unwrap_or(false);
        if !armed {
            if let Some(c) = self.mode.as_mut() {
                c.abort_armed = true;
            }
            klog!("conflict-mode: abort armed (second confirm required)");
            false
        } else {
            true
        }
    }

    /// Marshal an error toast to the parent (deferred — `push_toast` lives on
    /// `KagiApp`; this may run from a leased listener so it must not touch the
    /// parent synchronously).
    fn marshal_error_toast(&self, msg: String, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        cx.spawn(async move |_view, acx| {
            let _ = weak_app.update(acx, |app, cx| {
                app.push_toast(super::ToastKind::Error, SharedString::from(msg), cx);
            });
        })
        .detach();
    }

    /// Marshal an info toast to the parent (deferred — see `marshal_error_toast`).
    fn marshal_info_toast(&self, msg: String, cx: &mut Context<Self>) {
        let weak_app = self.app.clone();
        cx.spawn(async move |_view, acx| {
            let _ = weak_app.update(acx, |app, cx| {
                app.push_toast(super::ToastKind::Info, SharedString::from(msg), cx);
            });
        })
        .detach();
    }

    /// T-CONFLICT-UI-001 (moved from `KagiApp::sync_conflict_editor_inputs`):
    /// lazily create / refresh the Result CodeEditor `InputState` backing the
    /// Conflict Editor (ADR-0071). Runs from the parent render-sync pass via
    /// `entity.update` on a `Window` context (needed to create `InputState`); the
    /// entity now owns `editor_inputs` / `editing` / `mode`. In Preview mode the
    /// Result mirrors the assembled text (re-pushed only when the file/content/
    /// mode signature changes, so an in-progress edit is never clobbered); in Edit
    /// mode it instead *pulls* the editor's text into the buffer.
    pub fn sync_editor_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Only relevant while editing a content file with a hunk model.
        let Some(path) = self.editing.clone() else {
            self.editor_inputs = None;
            return;
        };
        let Some(c) = self.mode.as_ref() else {
            self.editor_inputs = None;
            return;
        };
        let Some(model) = c.buffer.hunk_model(&path) else {
            self.editor_inputs = None;
            return;
        };

        // Assemble the Result text block (chars/line-safe join).
        let result_text = model.assembled_text();
        let edit_mode = self.result_editing;

        // Edit mode: pull the user's edits out of the Result editor into the
        // buffer (set_manual_text), then return (do not overwrite their text).
        if edit_mode {
            if let Some(inputs) = self.editor_inputs.as_ref() {
                if inputs.path == path {
                    let edited = inputs.result.read(cx).value().to_string();
                    if edited != result_text {
                        if let Some(c) = self.mode.as_mut() {
                            let _ = c.buffer.set_manual_text(&path, &edited);
                            let _ = c.buffer.autosave();
                            // Refresh the file status from the buffer.
                            let residue = c.buffer.files_with_marker_residue();
                            if let Some(f) = c.session.files.iter_mut().find(|f| f.path == path) {
                                f.status = if residue.contains(&f.path) {
                                    kagi_git::ConflictStatus::NeedsReview
                                } else if c.buffer.has_resolution(&path) {
                                    kagi_git::ConflictStatus::Resolved
                                } else {
                                    kagi_git::ConflictStatus::Unresolved
                                };
                            }
                        }
                    }
                    // A/B row lists never change while editing the Result; keep as-is.
                    return;
                }
            }
        }

        let sig = super::conflict_content_sig(&path, &result_text, edit_mode);

        // Reuse existing inputs if the path + content + mode are unchanged.
        if let Some(inputs) = self.editor_inputs.as_ref() {
            if inputs.path == path && inputs.content_sig == sig {
                return;
            }
        }

        // Build or refresh.  Create the entities once per path; otherwise reuse.
        let need_create = self
            .editor_inputs
            .as_ref()
            .map(|i| i.path != path)
            .unwrap_or(true);

        if need_create {
            let result = cx.new(|cx| InputState::new(window, cx).code_editor("text"));
            self.editor_inputs = Some(super::ConflictEditorInputs {
                path: path.clone(),
                result,
                content_sig: 0,
            });
        }

        if let Some(inputs) = self.editor_inputs.as_ref() {
            inputs
                .result
                .update(cx, |s, cx| s.set_value(result_text.clone(), window, cx));
        }
        if let Some(inputs) = self.editor_inputs.as_mut() {
            inputs.content_sig = sig;
        }
    }
}

/// Render the persistent conflict banner shown directly under the header.
///
/// Free function (no listeners): the parent calls it with a cloned [`ConflictMode`]
/// (read out of the entity before the body is rendered) so the entity is never
/// rendered twice in one frame.
pub fn render_banner(mode: &ConflictMode) -> gpui::AnyElement {
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
                .flex_grow(1.)
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
    cx: &mut Context<ConflictView>,
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

fn render_dashboard(mode: &ConflictMode, cx: &mut Context<ConflictView>) -> gpui::AnyElement {
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
fn dash_counts(mode: &ConflictMode, cx: &mut Context<ConflictView>) -> gpui::AnyElement {
    let conflicted = mode.conflicted_count();
    let resolved = mode.resolved_count();

    let prev = cx.listener(|view: &mut ConflictView, _e: &gpui::ClickEvent, _w, cx| {
        view.conflict_nav_unresolved(-1);
        cx.notify();
    });
    let next = cx.listener(|view: &mut ConflictView, _e: &gpui::ClickEvent, _w, cx| {
        view.conflict_nav_unresolved(1);
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
                .flex_grow(1.)
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
fn dash_primary(mode: &ConflictMode, cx: &mut Context<ConflictView>) -> gpui::AnyElement {
    let can_continue = mode.can_continue();
    let is_sequencer = mode.session.op.is_sequencer();

    // Continue reloads (merge → commit panel / sequencer → confirm modal), so it
    // MUST defer to the parent — calling `conflict_continue` synchronously here
    // would re-lease this leased entity and panic.
    let continue_handler = cx.listener(
        |view: &mut ConflictView, _e: &gpui::ClickEvent, window, cx| {
            let weak_app = view.app.clone();
            cx.spawn_in(window, async move |_view, acx| {
                let _ =
                    weak_app.update_in(acx, |app, window, cx| app.conflict_continue(window, cx));
            })
            .detach();
        },
    );
    // Abort is two-stage: the first click ARMS (entity-internal, no reload), the
    // second EXECUTES (reload → defer to parent).
    let abort_handler = cx.listener(
        |view: &mut ConflictView, _e: &gpui::ClickEvent, window, cx| {
            if view.abort_request_arm() {
                // Already armed → execute via the parent (reloads).
                let weak_app = view.app.clone();
                cx.spawn_in(window, async move |_view, acx| {
                    let _ = weak_app.update_in(acx, |app, _window, cx| app.conflict_abort(cx));
                })
                .detach();
            } else {
                cx.notify();
            }
        },
    );
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
        let next = cx.listener(|view: &mut ConflictView, _e: &gpui::ClickEvent, _w, cx| {
            view.conflict_nav_unresolved(1);
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
        // Skip reloads → defer to the parent.
        let skip = cx.listener(
            |view: &mut ConflictView, _e: &gpui::ClickEvent, window, cx| {
                let weak_app = view.app.clone();
                cx.spawn_in(window, async move |_view, acx| {
                    let _ = weak_app.update_in(acx, |app, _window, cx| app.conflict_skip(cx));
                })
                .detach();
            },
        );
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
///
/// ADR-0118: rendered as a TOP-LEVEL overlay on the `KagiApp` context (a sibling
/// of the body in `render.rs`, never inside the `ConflictView` entity render), so
/// its `on_select` may call the parent's conflict actions directly without
/// leasing the entity. The owning `file_menu` state lives on the entity, so the
/// dismiss/select close over the entity handle to clear it via `entity.update`.
pub fn render_file_menu(
    entity: &Entity<ConflictView>,
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

    let dismiss_entity = entity.clone();
    let on_dismiss = move |_this: &mut KagiApp, _w: &mut Window, cx: &mut Context<KagiApp>| {
        dismiss_entity.update(cx, |v, _| v.file_menu = None);
    };
    let select_entity = entity.clone();
    let on_select = move |this: &mut KagiApp,
                          action: ConflictFileAction,
                          window: &mut Window,
                          cx: &mut Context<KagiApp>| {
        // This runs on the `KagiApp` context (top-level overlay), so the entity
        // is NOT leased — clearing `file_menu` via `update` and calling the parent
        // conflict actions directly is safe here.
        select_entity.update(cx, |v, _| v.file_menu = None);
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
fn dash_sections(mode: &ConflictMode, cx: &mut Context<ConflictView>) -> gpui::AnyElement {
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
    cx: &mut Context<ConflictView>,
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
        // Entity-internal (buffer + repo-relative marker build); mutates self.
        let row_click = cx.listener(
            move |view: &mut ConflictView, _e: &gpui::ClickEvent, _w, cx| {
                view.conflict_select_file(idx);
                cx.notify();
            },
        );

        // Per-card actions (T-CONFLICT-DASH-022). Each acts on THIS row's file
        // (not the selected one) and stops propagation so the row-select / editor
        // open doesn't also fire. "…" opens the overflow menu at the cursor
        // (entity-internal state); the ext/copy shortcuts read the conflict
        // snapshot on the parent → defer (would re-lease this leased entity).
        let more = cx.listener(
            move |view: &mut ConflictView, e: &gpui::MouseDownEvent, _w, cx| {
                cx.stop_propagation();
                view.file_menu = Some((idx, e.position));
                cx.notify();
            },
        );
        let ext = cx.listener(
            move |view: &mut ConflictView, _e: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                let weak_app = view.app.clone();
                cx.spawn_in(window, async move |_view, acx| {
                    let _ = weak_app.update_in(acx, |app, _window, cx| {
                        app.conflict_open_external_tool(idx, cx)
                    });
                })
                .detach();
            },
        );
        let copy = cx.listener(
            move |view: &mut ConflictView, _e: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                let weak_app = view.app.clone();
                cx.spawn_in(window, async move |_view, acx| {
                    let _ =
                        weak_app.update_in(acx, |app, _window, cx| app.conflict_copy_path(idx, cx));
                })
                .detach();
            },
        );

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
fn render_center(mode: &ConflictMode, cx: &mut Context<ConflictView>) -> gpui::AnyElement {
    let Some(idx) = mode.selected_file else {
        return div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .flex_grow(1.)
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
        return div().flex_grow(1.).h_full().into_any_element();
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
    let keep_current = cx.listener(
        move |view: &mut ConflictView, _e: &gpui::ClickEvent, _w, cx| {
            view.conflict_apply_choice(&p1, kagi_git::resolution::ResolutionChoice::Current, cx);
            cx.notify();
        },
    );
    let p2 = path.clone();
    let take_incoming = cx.listener(
        move |view: &mut ConflictView, _e: &gpui::ClickEvent, _w, cx| {
            view.conflict_apply_choice(&p2, kagi_git::resolution::ResolutionChoice::Incoming, cx);
            cx.notify();
        },
    );
    let p3 = path.clone();
    let keep_both = cx.listener(
        move |view: &mut ConflictView, _e: &gpui::ClickEvent, _w, cx| {
            view.conflict_apply_choice(
                &p3,
                kagi_git::resolution::ResolutionChoice::BothCurrentFirst,
                cx,
            );
            cx.notify();
        },
    );

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
        .flex_grow(1.)
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
        .flex_grow(1.)
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
