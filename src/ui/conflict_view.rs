//! Session-owned conflict workspace: file tree, one hunk, and Result inspection.
//! Git mutation stays behind the existing frozen intent/application boundary.
//! ResolutionBuffer owns every choice and manual draft; pages are view-only.

use kagi_ui_core::tooltip::HoverTooltip as _;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use gpui::{
    div, prelude::*, px, rgb, Context, Entity, SharedString, UniformListScrollHandle, WeakEntity,
    Window,
};
use gpui_component::button::Button;
use gpui_component::input::InputState;
use gpui_component::{Disableable as _, Sizable as _};

use kagi_git::conflicts::{ConflictKind, ConflictStatus, SideLabels};

use super::button_style::{ButtonRole, KagiButton};
use super::context_menu::{ItemState, MenuGroup, MenuItem};
use super::i18n::Msg;
use super::menu_overlay;
use super::theme::{self, theme};
use super::ConflictEditorInputs;
use super::KagiApp;

use super::{conflict_binary_view, e2e, ToastKind};

static NEXT_INTENT_TOKEN: AtomicU64 = AtomicU64::new(1);

mod actions;
mod file_menu;
mod saved;
mod sidebar;
mod special;
mod toolbar;
pub use file_menu::render_file_menu;
pub use saved::SavedPreview;
#[cfg(test)]
mod tests;
#[cfg(feature = "gui-e2e")]
pub use toolbar::continue_primary_for_e2e;
mod draft_owner;
pub(super) mod flow;
pub use flow::ConflictPage;

#[cfg(feature = "gui-e2e")]
thread_local! {
    static NEXT_CONFLICT_FOCUS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Records real keyboard focus without assuming this is the window's last Tab stop.
#[cfg(feature = "gui-e2e")]
pub fn take_next_conflict_focus_probe() -> bool {
    NEXT_CONFLICT_FOCUS.with(|focused| focused.replace(false))
}

/// T-CONFLICT-UI: the per-pane editor state for the open file.
/// Held on [`KagiApp`] (they need a `Window` to create) and passed into the
/// render functions via [`EditorChrome`].
#[derive(Clone)]
pub struct EditorInputs {
    /// The conflicting file these inputs are bound to.
    pub path: PathBuf,
    /// Directly editable Result; the Input is disabled while its writer is busy.
    pub result: Entity<InputState>,
}

/// T-CONFLICT-UI/UX: the Conflict Editor chrome the app threads into the render
/// functions (the editors + split ratios + Result mode + resize geometry live
/// on [`KagiApp`], not on the cloned [`ConflictMode`]).
#[derive(Clone)]
pub struct EditorChrome {
    pub current_commit: String,
    /// Presentation-only projection of the authoritative Sessions writer lease.
    pub writer_busy: bool,
    pub save_refused: bool,
    pub source_choices_enabled: bool,
    pub manual_override: bool,
    pub page: ConflictPage,
    pub navigation_focus: Option<gpui::FocusHandle>,
    pub show_tools: bool,
    pub reset_all_armed: bool,
    pub reset_all_focus: Option<gpui::FocusHandle>,
    /// The Result pane `InputState`, when present for the edited content file.
    pub inputs: Option<EditorInputs>,
    /// Shared A/B row-list scroll handle; both panes track it for vertical sync.
    pub ab_scroll: UniformListScrollHandle,
    pub(crate) resolver_keyboard:
        std::rc::Rc<std::cell::RefCell<super::conflict_editor_keys::ResolverKeyboard>>,
    /// Scroll handle for the Result *preview* row list (ADR-0135 badges).
    pub result_scroll: UniformListScrollHandle,
    /// Syntax-highlight cache for the Result preview, keyed by the assembled
    /// text (which changes on every selection change, unlike the A/B rows).
    pub result_hl:
        std::rc::Rc<std::cell::RefCell<Option<crate::ui::conflict_editor::ResultHlCache>>>,
    /// Whether the Result pane is in Edit mode (UX-015).
    pub result_editing: bool,
    /// A|B pane width split ratio (fraction given to A).
    pub ab_split: f32,
    /// T-CONFLICT-UX-010/012: index (among conflict hunks) of the focused hunk,
    /// for the selected-hunk highlight in the per-hunk accept list.
    pub selected_hunk: usize,
    /// Measured (left, right) screen-px bounds of the A·B row.
    pub ab_geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    pub raw_preview_scroll: gpui::ScrollHandle,
    pub(crate) raw_preview: conflict_binary_view::PreviewState,
    /// Per-side syntax-highlight cache for the A/B row lists (ADR-0135).
    /// Interior-mutable so the render path can fill it lazily; keyed by
    /// (path, theme slug) — row TEXT never changes within a session, only the
    /// selection flags do, so selection clicks never re-parse.
    pub hl_cache: std::rc::Rc<std::cell::RefCell<Option<crate::ui::conflict_editor::SideHlCache>>>,
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
/// A `ConflictView` listener leases this entity. Its Backend actions
/// (`conflict_continue`/`abort`/`skip`/`editor_save`) and snapshot-reading
/// context actions (open-external-tool / copy-path / copy-git-command /
/// open-terminal) call `KagiApp` methods that read/write the owner's conflict
/// slot, so calling any synchronously from a leased listener re-leases this
/// entity and panics. Each therefore defers to the parent via `cx.spawn_in` +
/// `weak_app.update_in`, carrying the frozen owner (ADR-0197 決定 5) so a
/// deferred action lands on its own session, never the active one.
///
/// Session-owned but outside this entity: `conflict_merge_pending` and the
/// per-owner conflict-detection guard.
pub struct ConflictView {
    window_activation_subscription: Option<gpui::Subscription>,
    _draft_quit_subscription: gpui::Subscription,
    pub(crate) operation_chrome_subscription: Option<gpui::Subscription>,
    #[cfg(feature = "gui-e2e")]
    pub(crate) window_active_override: Option<bool>,
    /// `Some(_)` while a conflict/merge is in progress (set by
    /// `detect_conflict_mode` via `apply_conflict_detect`). The repository is
    /// untouched until Continue / Abort execute through the existing plan
    /// pipeline.
    pub mode: Option<ConflictMode>,
    pub current_commit: String,
    /// Retained editor owner; saved inspection never owns an editable draft.
    pub editing: Option<PathBuf>,
    pub page: ConflictPage,
    pub sidebar_hidden: bool,
    panel_motion: super::panel_motion::PanelMotions,
    pub sidebar_expanded: HashSet<PathBuf>,
    pub selected_saved: Option<PathBuf>,
    pub saved_preview: Option<SavedPreview>,
    pub save_refused: Option<PathBuf>,
    pub(crate) save_attempt: Option<PathBuf>,
    /// Live decisions have not reached their recovery document; host close must ask.
    pub draft_persist_failed: Option<String>,
    draft_failure_pending: Option<String>,
    draft_save: Option<kagi_git::resolution::DraftSave>,
    draft_watch_running: bool,
    draft_watched_generation: u64,
    pub(crate) draft_flushes: usize,
    /// Presentation projection from the existing accepted-observation stale path.
    pub(crate) saved_review_needed: bool,
    sidebar: sidebar::SidebarState,
    saved_cache: HashMap<(PathBuf, String), SavedPreview>,
    saved_read: Option<(PathBuf, String)>,
    saved_error: bool,
    sidebar_toggle_focus: Option<gpui::FocusHandle>,
    navigation_focus: Option<gpui::FocusHandle>,
    pub show_tools: bool,
    reset_all_focus: Option<gpui::FocusHandle>,
    /// Same-pane checkbox painting; resolution state stays in `line_select`.
    pub(crate) line_paint: Option<(kagi_git::resolution::SelectionSide, bool)>,
    /// W32: last-saved resolved text per file, so a Save can log a before→after
    /// file-content hash pair (T-035) without re-reading the working tree.
    pub editing_before_text: HashMap<PathBuf, String>,
    /// T-CONFLICT-UI-001/005: the three CodeEditor `InputState`s backing the
    /// A / B / Result panes (ADR-0069). Lazily created in a `Window` context.
    pub editor_inputs: Option<ConflictEditorInputs>,
    result_subscription: Option<gpui::Subscription>,
    /// T-CONFLICT-UX-015: whether the Result pane is in Edit mode (editable)
    /// rather than Preview (read-only).
    pub result_editing: bool,
    /// The edited path whose first-stage Reset all approval is still current.
    pub reset_all_target: Option<PathBuf>,
    /// T-CONFLICT-UI-003: A|B pane width split ratio (fraction given to A).
    pub ab_split: f32,
    /// Measured (left, right) screen-px bounds for A|B divider dragging.
    /// Shared with `KagiApp`: render writes bounds, the root drag handler reads.
    pub ab_geom: std::rc::Rc<std::cell::Cell<(f32, f32)>>,
    /// Stable raw-preview scroll owner, shared by production and E2E renders.
    pub raw_preview_scroll: gpui::ScrollHandle,
    pub(crate) raw_preview: conflict_binary_view::RawPreview,
    /// T-CONFLICT-UX-010/012: index (among conflict hunks) of the focused hunk
    /// in the per-hunk Conflict Editor.
    pub selected_hunk: usize,
    /// ADR-0070: shared A/B uniform-list scroll handle for synchronized vertical
    /// scrolling in the Conflict Editor.
    pub ab_scroll_handle: UniformListScrollHandle,
    pub(crate) resolver_keyboard:
        std::rc::Rc<std::cell::RefCell<super::conflict_editor_keys::ResolverKeyboard>>,
    /// See [`EditorChrome::result_scroll`] / [`EditorChrome::result_hl`]; both
    /// live on the entity so they survive across frames.
    pub result_scroll_handle: UniformListScrollHandle,
    pub result_hl:
        std::rc::Rc<std::cell::RefCell<Option<crate::ui::conflict_editor::ResultHlCache>>>,
    /// See [`EditorChrome::hl_cache`]; the cell lives on the entity so the
    /// cache survives across frames.
    pub hl_cache: std::rc::Rc<std::cell::RefCell<Option<crate::ui::conflict_editor::SideHlCache>>>,
    /// T-CONFLICT-DASH-022: the open per-file "…" context menu, as
    /// `(file_index, anchor)` where `anchor` is the click position in window px.
    /// `None` when no menu is open. Rendered as a top-level overlay.
    pub file_menu: Option<(usize, gpui::Point<gpui::Pixels>)>,
    /// Presentation-only projection used to disable Save/D-F controls while
    /// the application owns a conflict writer lease.
    pub writer_busy: bool,
    pub(crate) intent_token: u64,
    pub(crate) owner: crate::app::Attachment,
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
    pub fn new(
        app: WeakEntity<KagiApp>,
        repo_path: PathBuf,
        owner: crate::app::Attachment,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            window_activation_subscription: None,
            _draft_quit_subscription: cx.on_app_quit(|view, cx| view.flush_drafts_on_quit(cx)),
            operation_chrome_subscription: None,
            #[cfg(feature = "gui-e2e")]
            window_active_override: None,
            mode: None,
            current_commit: String::new(),
            editing: None,
            page: ConflictPage::Hunk,
            sidebar_hidden: false,
            panel_motion: Default::default(),
            sidebar_expanded: HashSet::new(),
            selected_saved: None,
            saved_preview: None,
            save_refused: None,
            save_attempt: None,
            draft_persist_failed: None,
            draft_failure_pending: None,
            draft_save: None,
            draft_watch_running: false,
            draft_watched_generation: 0,
            draft_flushes: 0,
            saved_review_needed: false,
            sidebar: Default::default(),
            saved_cache: HashMap::new(),
            saved_read: None,
            saved_error: false,
            sidebar_toggle_focus: None,
            navigation_focus: None,
            show_tools: false,
            reset_all_focus: None,
            line_paint: None,
            editing_before_text: HashMap::new(),
            editor_inputs: None,
            result_subscription: None,
            result_editing: false,
            reset_all_target: None,
            ab_split: super::CONFLICT_AB_DEFAULT,
            ab_geom: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
            raw_preview_scroll: gpui::ScrollHandle::new(),
            raw_preview: Default::default(),
            hl_cache: std::rc::Rc::new(std::cell::RefCell::new(None)),
            selected_hunk: 0,
            ab_scroll_handle: UniformListScrollHandle::new(),
            resolver_keyboard: Default::default(),
            result_scroll_handle: UniformListScrollHandle::new(),
            result_hl: std::rc::Rc::new(std::cell::RefCell::new(None)),
            file_menu: None,
            writer_busy: false,
            intent_token: NEXT_INTENT_TOKEN.fetch_add(1, Ordering::Relaxed),
            owner,
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
}

/// Pure UI-side data: the `ConflictSession` describes the in-progress operation
/// and its files; the `ResolutionBuffer` holds the in-memory Result drafts (the
/// repository is untouched until Continue/Abort/Skip execute through the plan
/// pipeline).  `current_branch` is captured once at detection time for the
/// `side_labels` left role.
#[derive(Clone)]
pub struct ConflictMode {
    /// Full repository conflict identity accepted from the Backend. Save, D/F
    /// and Continue approvals freeze this value, including all staged content.
    /// Same-file input retention instead uses the narrower file identity.
    pub revision: kagi_domain::conflict_family::ConflictRevision,
    /// The detected conflict session (operation + files), with per-file `status`
    /// recomputed from the buffer at detection time.
    pub session: kagi_git::conflicts::ConflictSession,
    /// The resolution buffer (in-memory Result drafts + materialized sides).
    pub buffer: kagi_git::resolution::ResolutionBuffer,
    /// Current branch short name, for the `side_labels` left role.
    pub current_branch: String,
    /// Index into `session.files` of the file whose detail/preview is open.
    pub selected_file: Option<usize>,
    /// Index into `session.files` of the text file open in the Conflict Editor.
    /// `None` for a whole-file conflict or saved-file inspection.
    pub editing_file: Option<usize>,
    /// Two-stage Abort used to be armed here (#704 removed it): the arming
    /// belonged to the entity, so an abort was only reachable while the entity
    /// existed. It is the plan confirmation now — same two stages, no
    /// dependency on this view being alive. Skip is still entity-local; it is
    /// a sequencer step, not an escape hatch.
    ///
    /// Full step identity frozen on the first Skip click. A refreshed mode
    /// disarms confirmation; the backend checks this same revision again
    /// immediately before spawning Git.
    pub skip_approval: Option<kagi_domain::conflict_family::ConflictRevision>,
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
        if self.buffer.has_cached_marker_residue() {
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

// ────────────────────────────────────────────────────────────
// Banner (persistent, under the header)
// ────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────
// ADR-0118 / T-ENTITY-CONFLICT-001: ConflictView entity render + listeners.
//
// The entity renders exactly one presentation page at a time.
// ────────────────────────────────────────────────────────────

impl Render for ConflictView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(feature = "gui-e2e")]
        let _render_io = kagi_git::backend::render_probe::enter();
        self.sync_editor_inputs(window, cx);
        self.sync_raw_preview(cx);
        #[cfg(feature = "gui-e2e")]
        super::conflict_editor_a11y::begin_frame();
        if self.reset_all_focus.is_none() {
            self.reset_all_focus = Some(cx.focus_handle());
        }
        if self.navigation_focus.is_none() {
            self.navigation_focus = Some(cx.focus_handle().tab_stop(false));
        }
        if self.sidebar_toggle_focus.is_none() {
            self.sidebar_toggle_focus = Some(cx.focus_handle());
        }
        if self.reset_all_target.is_some()
            && !self
                .reset_all_focus
                .as_ref()
                .is_some_and(|handle| handle.is_focused(window))
        {
            self.reset_all_target = None;
        }
        if self.window_activation_subscription.is_none() {
            self.window_activation_subscription =
                Some(cx.observe_window_activation(window, |view, window, cx| {
                    if !window.is_window_active() {
                        view.reset_all_target = None;
                        view.line_paint = None;
                    }
                    cx.notify();
                }));
        }
        let window_active = window.is_window_active();
        #[cfg(feature = "gui-e2e")]
        let window_active = self.window_active_override.unwrap_or(window_active);
        // entity-exists ⟺ mode-is-some in practice (the parent drops the entity on
        // clear), but guard defensively so a transient empty entity paints nothing.
        // `as_ref`, not `clone`: `ConflictMode` owns the whole `ResolutionBuffer`
        // — every conflicted file's current/incoming text, its resolved lines,
        // and the full undo *and* redo stacks, each entry another owned
        // `Vec<ResolvedLine>`. Cloning that per frame is megabytes of `String`
        // allocation on the one screen where the user types, so every keystroke
        // paid for it. `render_body` already takes a reference, and `cx` is a
        // separate parameter from `self`, so the borrow is fine.
        let resolver = self.mode.as_ref().and_then(|mode| {
            let file = mode.selected_file.and_then(|i| mode.session.files.get(i))?;
            Some((file.path.as_path(), mode.buffer.hunk_model(&file.path)?))
        });
        self.resolver_keyboard.borrow_mut().prepare(
            resolver,
            self.selected_hunk,
            &self.ab_scroll_handle,
            window,
            cx,
        );
        self.render_workspace(window_active, window, cx)
    }
}

impl ConflictView {
    pub(crate) fn freeze_save_intent(&self, path: &std::path::Path) -> FrozenConflictIntent {
        let mode = self.mode.as_ref().expect("rendered conflict has a mode");
        let before = self
            .editing_before_text
            .get(path)
            .map(String::as_str)
            .unwrap_or_default();
        match kagi_git::Backend::conflict_save_request(
            mode.revision.clone(),
            &mode.buffer,
            path,
            mode.session.op.kind(),
            before,
        ) {
            Ok(request) => FrozenConflictIntent::Request {
                token: self.intent_token,
                request: crate::app::ConflictAppRequest {
                    owner: self.owner.clone(),
                    request,
                },
            },
            Err(error) => FrozenConflictIntent::SaveRefusal {
                token: self.intent_token,
                owner: self.owner.clone(),
                revision: mode.revision.clone(),
                operation: mode.session.op.slug().to_string(),
                path: path.to_path_buf(),
                error: error.to_string(),
            },
        }
    }

    pub(crate) fn freeze_dir_file_intent(
        &self,
        path: &std::path::Path,
        choice: kagi_git::DirFileChoice,
    ) -> FrozenConflictIntent {
        let mode = self.mode.as_ref().expect("rendered conflict has a mode");
        FrozenConflictIntent::Request {
            token: self.intent_token,
            request: crate::app::ConflictAppRequest {
                owner: self.owner.clone(),
                request: kagi_domain::conflict_family::ConflictRequest::ResolveDirFile {
                    path: path.to_path_buf(),
                    revision: mode.revision.clone(),
                    choice,
                },
            },
        }
    }

    /// Build the [`EditorChrome`] the render functions thread through, from this
    /// entity's own fields (the editors + split ratios + Result mode + geometry
    /// all live on the entity now).
    fn editor_chrome(&self) -> EditorChrome {
        let manual_override = self.editing.as_deref().is_some_and(|path| {
            self.mode
                .as_ref()
                .is_some_and(|mode| flow::manual_override(mode, path))
        });
        EditorChrome {
            current_commit: self.current_commit.clone(),
            writer_busy: self.writer_busy,
            save_refused: self.save_refused.is_some() && self.save_refused == self.editing,
            source_choices_enabled: !self.writer_busy && !manual_override,
            manual_override,
            page: self.page,
            navigation_focus: self.navigation_focus.clone(),
            show_tools: self.show_tools,
            reset_all_armed: self.reset_all_target.is_some(),
            reset_all_focus: self.reset_all_focus.clone(),
            inputs: self.editor_inputs.as_ref().map(|i| EditorInputs {
                path: i.path.clone(),
                result: i.result.clone(),
            }),
            ab_scroll: self.ab_scroll_handle.clone(),
            resolver_keyboard: self.resolver_keyboard.clone(),
            result_scroll: self.result_scroll_handle.clone(),
            result_hl: self.result_hl.clone(),
            result_editing: self.result_editing,
            ab_split: self.ab_split,
            selected_hunk: self.selected_hunk,
            ab_geom: self.ab_geom.clone(),
            raw_preview_scroll: self.raw_preview_scroll.clone(),
            raw_preview: self.raw_preview.state.clone(),
            hl_cache: self.hl_cache.clone(),
        }
    }
}

pub(crate) enum FrozenConflictIntent {
    Request {
        token: u64,
        request: crate::app::ConflictAppRequest,
    },
    SaveRefusal {
        token: u64,
        owner: crate::app::Attachment,
        revision: kagi_domain::conflict_family::ConflictRevision,
        operation: String,
        path: PathBuf,
        error: String,
    },
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
