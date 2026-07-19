//! Workspace pane framework (ADR-0120).
//!
//! The single place that decides **what each layout slot shows**. Before this
//! module, the precedence between the scattered gate fields (`file_history`,
//! `ecosystem`, `loading_tab`, `main_diff`, `commit_panel_open`,
//! `inspector_visible`, `sidebar.visible`, …) was implicit in the if/else
//! ordering inside `render_body`. Adding a new pane content meant finding the
//! right branch by archaeology.
//!
//! Now: `render_body` snapshots the gates into [`WorkspaceInputs`], calls
//! [`resolve_workspace`], and routes each slot on the returned
//! [`WorkspaceLayout`]. Adding a new pane content = a new enum variant here +
//! a render arm in `render_body` (+ an open/close method on `KagiApp`).
//!
//! One case pushes rather than just routes: the `CenterPane::Editor` arm is a
//! single self-rendering entity (re-entrancy — see `editor_workspace.rs`), so
//! it can't be skipped slot-by-slot the way the sidebar/right-panel arms are.
//! Instead `render_body` pushes `layout.left` into the entity's `show_tree`
//! field before embedding it (same push-then-embed pattern as the Commit
//! Panel's `active_wip`/`panel_render_width` below), so the resolved layout
//! stays the single source of truth even though the render call itself can't
//! branch on it (T-WS-EDITOR-005 finding #3).
//!
//! Scope notes:
//! - The **Conflict Mode** body and the error/welcome screens are gated one
//!   level above (in `render.rs`) because they replace the whole body
//!   including the sidebar and the bottom panel; they are documented in
//!   ADR-0120 but deliberately not routed through this resolver.
//! - The **bottom panel** keeps its own `BottomTab` enum (`types.rs`) — it is
//!   already an explicit slot switch and doesn't interact with these slots.
//! - The resolver (`resolve_workspace`) is pure slot *policy* (unit-tested
//!   below). It stays in `src/ui/` rather than `kagi-domain` because its
//!   vocabulary (Inspector, CommitPanel, Navigator) is UI, not Git domain.
//!
//! ADR-0121 Phase B (B1): this module also defines [`WorkspaceItem`], the
//! kagi-minimal equivalent of zed's `Item` trait, plus the adapters that
//! bridge the existing entity-backed panes (FileHistory / Ecosystem /
//! EditorWorkspace) onto it. `render_body` routes entity panes through
//! [`center_item`] instead of hand-written per-field arms, and
//! `reset_per_repo_ui` disposes them through the same registry. B1 keeps the
//! `KagiApp` fields — the adapters read them; B2 migrates panes one by one.

use gpui::{div, px, AnyElement, Context, IntoElement, ParentElement, Styled};

use super::{commit_list, inspector, CommitId, CompareView, KagiApp, MainDiffSource};

/// Which layout slot a [`WorkspaceItem`] occupies when active (ADR-0121 B1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// Spans center + right; the resolver hides the right panel while active.
    CenterTakeover,
    /// The center pane only.
    Center,
    /// The right panel.
    Right,
}

/// ADR-0121 B1: zed-`Item`-equivalent minimal pane trait. Deliberately the
/// smallest face kagi's slot resolution needs — render + liveness + disposal.
/// No focus/event/serialization surface until a pane actually needs it.
pub trait WorkspaceItem {
    /// Which slot this item occupies when active.
    fn slot(&self) -> Slot;
    /// The resolved center variant this item is registered for (center-slot
    /// items only).
    fn center(&self) -> Option<CenterPane> {
        None
    }
    /// The resolved right variant this item is registered for (right-slot
    /// items only; ADR-0121 B2).
    fn right(&self) -> Option<RightPane> {
        None
    }
    /// Liveness gate: is this pane active right now? Feeds
    /// [`WorkspaceInputs`]; the precedence between live items stays in
    /// [`resolve_workspace`].
    fn is_open(&self, app: &KagiApp) -> bool;
    /// Render into the resolved slot. `None` means the gate raced closed
    /// between resolution and render; `render_body` falls back exactly as the
    /// old per-field arms did.
    fn render(
        &self,
        app: &mut KagiApp,
        layout: &WorkspaceLayout,
        cx: &mut Context<KagiApp>,
    ) -> Option<AnyElement>;
    /// Drop the pane's per-repo state (called from `reset_per_repo_ui`).
    fn dispose(&self, app: &mut KagiApp);
}

/// File History takeover (ADR-0089/0117) — bridges `KagiApp.file_history`.
pub struct FileHistoryItem;

impl WorkspaceItem for FileHistoryItem {
    fn slot(&self) -> Slot {
        Slot::CenterTakeover
    }
    fn center(&self) -> Option<CenterPane> {
        Some(CenterPane::FileHistory)
    }
    fn is_open(&self, app: &KagiApp) -> bool {
        app.file_history.is_some()
    }
    // ADR-0089 / ADR-0117: the entity renders its own center+right body;
    // embedding `Entity<FileHistoryView>` gives it an isolated `cx.notify()`
    // scope.
    fn render(
        &self,
        app: &mut KagiApp,
        _layout: &WorkspaceLayout,
        _cx: &mut Context<KagiApp>,
    ) -> Option<AnyElement> {
        Some(app.file_history.clone()?.into_any_element())
    }
    // ADR-0117: File History is per-repo; drop the entity on repo/tab switch
    // so its captured `repo_path` can't keep reading the previous repo (and
    // the stale view doesn't linger over the newly-activated tab).
    fn dispose(&self, app: &mut KagiApp) {
        app.file_history = None;
    }
}

/// Code Ecosystem / Analyze takeover (ADR-0119) — bridges `KagiApp.ecosystem`.
pub struct EcosystemItem;

impl WorkspaceItem for EcosystemItem {
    fn slot(&self) -> Slot {
        Slot::CenterTakeover
    }
    fn center(&self) -> Option<CenterPane> {
        Some(CenterPane::Ecosystem)
    }
    fn is_open(&self, app: &KagiApp) -> bool {
        app.ecosystem.is_some()
    }
    // ADR-0119: full-screen, read-only. Wrapped in a `flex_1` + `min_w(0)`
    // cell so the entity gets a *definite* width to fill (the body minus the
    // sidebar). Mounted bare, the entity is a flex item with
    // `flex-basis: auto`, so it sizes to its content — the longest hot-spot
    // path — and its inner `flex_1` columns never get a bounded width to
    // shrink into, pushing the numeric columns + risk bar off the right edge
    // (user report on deep STM32 build paths).
    fn render(
        &self,
        app: &mut KagiApp,
        _layout: &WorkspaceLayout,
        _cx: &mut Context<KagiApp>,
    ) -> Option<AnyElement> {
        let eco = app.ecosystem.clone()?;
        Some(div().flex_1().min_w(px(0.)).child(eco).into_any_element())
    }
    // ADR-0119: the EcosystemView entity captures the previous repo's
    // `repo_path`; drop the view on repo/tab switch like File History. The
    // mine CACHE (`ecosystem_cache`) is keyed by repo and deliberately kept
    // across tab switches, so returning to a repo and pressing Analyze reuses
    // its previous scan instead of recomputing from scratch.
    fn dispose(&self, app: &mut KagiApp) {
        app.ecosystem = None;
    }
}

/// Branch Cleanup takeover (ADR-0128) — bridges `KagiApp.branch_cleanup_open`.
/// Unlike the entity-backed takeovers, the table data lives in
/// `active_view.cleanup_rows` (snapshot-derived, per-tab), so the gate is a
/// plain bool and the pane re-renders from fresh rows after every reload.
pub struct BranchCleanupItem;

impl WorkspaceItem for BranchCleanupItem {
    fn slot(&self) -> Slot {
        Slot::CenterTakeover
    }
    fn center(&self) -> Option<CenterPane> {
        Some(CenterPane::BranchCleanup)
    }
    fn is_open(&self, app: &KagiApp) -> bool {
        app.branch_cleanup_open
    }
    fn render(
        &self,
        app: &mut KagiApp,
        _layout: &WorkspaceLayout,
        cx: &mut Context<KagiApp>,
    ) -> Option<AnyElement> {
        Some(
            div()
                .flex_1()
                .min_w(px(0.))
                .child(super::branch_cleanup::render_branch_cleanup(app, cx))
                .into_any_element(),
        )
    }
    fn dispose(&self, app: &mut KagiApp) {
        app.branch_cleanup_open = false;
    }
}

/// Editor workspace (T-WS-EDITOR-001) — bridges `KagiApp.editor_workspace`.
pub struct EditorWorkspaceItem;

impl WorkspaceItem for EditorWorkspaceItem {
    fn slot(&self) -> Slot {
        Slot::Center
    }
    fn center(&self) -> Option<CenterPane> {
        Some(CenterPane::Editor)
    }
    fn is_open(&self, app: &KagiApp) -> bool {
        app.editor_workspace.is_some()
    }
    // T-WS-EDITOR-001: the Editor workspace entity self-renders the WHOLE
    // left(file tree) + center(code viewer) + right(hunks) triple in one call
    // — like the FileHistory/Ecosystem takeovers, this is the only way a
    // click in the tree pane can mutate the same entity that owns the open
    // file's editor/diff state without re-entering `KagiApp` (ADR-0117
    // re-entrancy guard). The `layout.right` value (Hunks) exists for
    // resolver-level policy + tests; it routes to the no-op right-slot arm in
    // `render_body` (`RightPane::Hunks => {}`) and the sidebar
    // `.when(... Navigator ...)` naturally skips rendering for
    // `LeftPane::FileTree`. `layout.left`, though, is pushed into the entity
    // (T-WS-EDITOR-005 finding #3): the sidebar toggle's `LeftPane::Hidden`
    // still needs to hide the *in-entity* tree pane, which the outside can't
    // reach into.
    fn render(
        &self,
        app: &mut KagiApp,
        layout: &WorkspaceLayout,
        cx: &mut Context<KagiApp>,
    ) -> Option<AnyElement> {
        let ev = app.editor_workspace.clone()?;
        let show_tree = layout.left == LeftPane::FileTree;
        ev.update(cx, |v, _| {
            v.show_tree = show_tree;
        });
        Some(
            div()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .child(ev)
                .into_any_element(),
        )
    }
    // T-WS-EDITOR-001: the EditorWorkspaceView entity captures the previous
    // repo's `repo_path`; drop it on repo/tab switch like File History /
    // Ecosystem — a new tab always opens on Graph since the mode is derived
    // from `editor_workspace.is_some()` (T-WS-EDITOR-005 #11).
    fn dispose(&self, app: &mut KagiApp) {
        app.editor_workspace = None;
    }
}

/// Full-width main diff (T-UI-003 / ADR-0121 B2) — bridges `KagiApp.main_diff`
/// (now `Option<Entity<MainDiffPane>>`, see `main_diff_pane.rs`).
pub struct MainDiffItem;

impl WorkspaceItem for MainDiffItem {
    fn slot(&self) -> Slot {
        Slot::Center
    }
    fn center(&self) -> Option<CenterPane> {
        Some(CenterPane::Diff)
    }
    fn is_open(&self, app: &KagiApp) -> bool {
        app.main_diff.is_some()
    }
    // ADR-0121 B2: embedded bare like File History — the pane's root element
    // (see `render_helpers::render_diff_list`) already carries the
    // `flex_1().min_w(0)` sizing the old plain arm's element had, so no
    // wrapper cell is needed.
    fn render(
        &self,
        app: &mut KagiApp,
        _layout: &WorkspaceLayout,
        _cx: &mut Context<KagiApp>,
    ) -> Option<AnyElement> {
        Some(app.main_diff.clone()?.into_any_element())
    }
    // Per-repo: the shown diff belongs to the previous repo; drop the pane
    // (and its scroll state) on repo/tab switch, as the old
    // `main_diff = None` reset did.
    fn dispose(&self, app: &mut KagiApp) {
        app.main_diff = None;
    }
}

/// The registered entity-backed panes (ADR-0121 B1/B2). Loading / CommitList /
/// CommitPanel / Inspector are not items yet — B2 migrates panes one by one;
/// until then `render_body` keeps plain arms for them.
pub const CENTER_ITEMS: [&dyn WorkspaceItem; 5] = [
    &FileHistoryItem,
    &EcosystemItem,
    &BranchCleanupItem,
    &EditorWorkspaceItem,
    &MainDiffItem,
];

/// Slot → registered item resolution: the item registered for a resolved
/// center variant, if any.
pub fn center_item(center: CenterPane) -> Option<&'static dyn WorkspaceItem> {
    CENTER_ITEMS
        .iter()
        .copied()
        .find(|it| it.center() == Some(center))
}

/// Staging + commit message panel (T025 / ADR-0118) — bridges
/// `KagiApp.commit_panel` (+ its `commit_panel_open` visibility gate).
pub struct CommitPanelItem;

impl WorkspaceItem for CommitPanelItem {
    fn slot(&self) -> Slot {
        Slot::Right
    }
    fn right(&self) -> Option<RightPane> {
        Some(RightPane::CommitPanel)
    }
    fn is_open(&self, app: &KagiApp) -> bool {
        app.commit_panel_open && app.commit_panel.is_some()
    }
    // ADR-0118: push the parent-owned render inputs into the entity, then
    // embed it as a self-rendering child. `active_wip` mirrors the old
    // `cp_active_wip(this)` (derived from the open main diff); the entity may
    // not read the parent's `main_diff` from its own render path (re-entrancy).
    fn render(
        &self,
        app: &mut KagiApp,
        _layout: &WorkspaceLayout,
        cx: &mut Context<KagiApp>,
    ) -> Option<AnyElement> {
        let entity = app.commit_panel.clone()?;
        let active_wip = match app
            .main_diff
            .as_ref()
            .map(|d| d.read(cx).view.source.clone())
        {
            Some(MainDiffSource::Unstaged { path }) => Some((false, path)),
            Some(MainDiffSource::Staged { path }) => Some((true, path)),
            _ => None,
        };
        let smart = app.smart_commit.clone();
        let panel_width = app.panel_width;
        entity.update(cx, |v, _| {
            v.active_wip = active_wip;
            v.panel_render_width = panel_width;
            v.smart_snapshot = smart;
        });
        Some(entity.into_any_element())
    }
    fn dispose(&self, app: &mut KagiApp) {
        app.commit_panel_open = false;
        // ADR-0118: dropping the single `commit_panel` entity also drops its
        // `commit_input` / template inputs / draft state (all entity-owned).
        app.commit_panel = None;
    }
}

/// Shared body of the Inspector / Compare right-slot items: derive the
/// selected commit's detail + badges + active-file highlight and render
/// `inspector::render_inspector` with the given files / diffstat / compare
/// inputs. Exactly the old single Inspector arm, parameterized on the three
/// inputs that differed between its normal and compare modes.
fn render_inspector_body(
    app: &mut KagiApp,
    files: Option<Vec<super::FileStatus>>,
    diffstat: Option<Vec<kagi_git::FileDiffStat>>,
    compare: Option<CompareView>,
    cx: &mut Context<KagiApp>,
) -> Option<AnyElement> {
    // ── Commit metadata ─
    let selected = app.selected;
    let d = selected
        .and_then(|i| app.active_view.details.get(i))
        .cloned()?;
    let at = CommitId(d.full_sha.as_ref().to_string());
    let selected_badges: Vec<commit_list::RefBadge> = selected
        .and_then(|i| app.active_view.rows.get(i))
        .map(|r| r.badges.clone())
        .unwrap_or_default();
    // Active file (for list highlight) derived from the open main diff.
    let active_commit_file: Option<usize> = match app
        .main_diff
        .as_ref()
        .map(|d| d.read(cx).view.source.clone())
    {
        Some(MainDiffSource::Commit { file_index, .. }) => Some(file_index),
        Some(MainDiffSource::Compare { file_index, .. }) => Some(file_index),
        _ => None,
    };
    Some(
        inspector::render_inspector(
            d,
            at,
            selected_badges,
            files,
            diffstat,
            compare,
            active_commit_file,
            app.inspector_tree_view,
            app.inspector_split,
            app.inspector_geom.clone(),
            app.panel_width,
            // W11-AVATAR: resolved avatar images so the inspector can swap
            // the initial circle for a real image.
            &app.avatars.images,
            cx,
        )
        .into_any_element(),
    )
}

/// Commit Inspector panel (W2-INSPECTOR) — bridges the `inspector_visible`
/// toggle + the selection-derived detail. Still function-rendered
/// (`inspector::render_inspector`); this adapter is the thin bridge —
/// Entity-conversion is out of ADR-0121 B2's scope.
pub struct InspectorItem;

impl WorkspaceItem for InspectorItem {
    fn slot(&self) -> Slot {
        Slot::Right
    }
    fn right(&self) -> Option<RightPane> {
        Some(RightPane::Inspector)
    }
    fn is_open(&self, app: &KagiApp) -> bool {
        app.inspector_visible
    }
    fn render(
        &self,
        app: &mut KagiApp,
        _layout: &WorkspaceLayout,
        cx: &mut Context<KagiApp>,
    ) -> Option<AnyElement> {
        // Changed files + diffstat for the selected commit (vs parent). A cache
        // miss or an unavailable diff both collapse to `None` ("(diff
        // unavailable)"), as the old `Option<Option<..>>` plumbing did.
        let selected = app.selected;
        let files: Option<Vec<super::FileStatus>> = selected
            .and_then(|i| app.diff_caches.changed_files.get(&i).cloned())
            .flatten();
        let diffstat = selected.and_then(|i| app.diff_caches.diffstat.get(&i).cloned());
        render_inspector_body(app, files, diffstat, None, cx)
    }
    // The inspector has no per-repo entity: `inspector_visible` is a global
    // View-menu toggle and the detail derives from `selected` (cleared in
    // `reset_per_repo_ui` itself).
    fn dispose(&self, _app: &mut KagiApp) {}
}

/// Compare mode (ADR-0026 / ADR-0121 B2) — bridges `KagiApp.compare_view`
/// (now `Option<Entity<ComparePane>>`, see `compare_pane.rs`). Draws the same
/// Inspector body with the compare inputs (banner + compare file list, no
/// per-file diffstat — W16-DIFFSTAT keeps compare out of scope), so it stays
/// function-rendered like `InspectorItem`; the entity owns the state.
pub struct CompareItem;

impl WorkspaceItem for CompareItem {
    fn slot(&self) -> Slot {
        Slot::Right
    }
    fn right(&self) -> Option<RightPane> {
        Some(RightPane::Compare)
    }
    fn is_open(&self, app: &KagiApp) -> bool {
        app.compare_view.is_some()
    }
    fn render(
        &self,
        app: &mut KagiApp,
        _layout: &WorkspaceLayout,
        cx: &mut Context<KagiApp>,
    ) -> Option<AnyElement> {
        let view = app.compare_view.as_ref()?.read(cx).view.clone();
        render_inspector_body(app, Some(view.files.clone()), None, Some(view), cx)
    }
    // Per-repo: the compared base/files belong to the previous repo; drop the
    // entity on repo/tab switch like the other registered panes.
    fn dispose(&self, app: &mut KagiApp) {
        app.compare_view = None;
        app.pending_headless_compare = None;
    }
}

/// The registered right-slot panes (ADR-0121 B2). Order documents the
/// precedence (CommitPanel > Compare > Inspector), but the precedence itself
/// stays in `resolve_workspace`.
pub const RIGHT_ITEMS: [&dyn WorkspaceItem; 3] = [&CommitPanelItem, &CompareItem, &InspectorItem];

/// Slot → registered item resolution: the item registered for a resolved
/// right variant, if any (Hunks / Hidden have none).
pub fn right_item(right: RightPane) -> Option<&'static dyn WorkspaceItem> {
    RIGHT_ITEMS
        .iter()
        .copied()
        .find(|it| it.right() == Some(right))
}

/// What the left pane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftPane {
    /// Repository Navigator (branches / remotes / tags / stashes / worktrees).
    Navigator,
    /// Working-tree changed-file tree (Editor mode, T-WS-EDITOR-001).
    FileTree,
    /// Sidebar toggled off (View → Toggle Sidebar).
    Hidden,
}

/// What the center (main) pane shows. Order of the variants documents the
/// precedence: earlier wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CenterPane {
    /// File History takeover (ADR-0089/0117) — spans center + right.
    FileHistory,
    /// Code Ecosystem / Analyze takeover (ADR-0119) — spans center + right.
    Ecosystem,
    /// Branch Cleanup table takeover (ADR-0128) — spans center + right.
    BranchCleanup,
    /// `Loading <repo>…` placeholder during an uncached tab open (W6-TABSPEED).
    Loading,
    /// Read-only code viewer (Editor mode, T-WS-EDITOR-001).
    /// Beats `Diff` — in Editor mode `main_diff` is ignored.
    Editor,
    /// Full-width diff view (T-UI-003).
    Diff,
    /// The commit graph list (default).
    CommitList,
}

/// What the right pane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightPane {
    /// Staging + commit message panel (T025 / ADR-0118).
    CommitPanel,
    /// Compare mode: Inspector body with the compare banner + compare file
    /// list (ADR-0026 / ADR-0121 B2). Shares the Inspector's visibility gates.
    Compare,
    /// Commit Inspector (W2-INSPECTOR).
    Inspector,
    /// The selected file's WIP hunks (Editor mode, T-WS-EDITOR-001).
    Hunks,
    /// No right panel (and no divider).
    Hidden,
}

/// Resolved slot contents for one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceLayout {
    pub left: LeftPane,
    pub center: CenterPane,
    pub right: RightPane,
}

/// Snapshot of the `KagiApp` gate fields the resolver needs. Plain bools so
/// the precedence policy is unit-testable without gpui.
#[derive(Clone, Copy, Debug, Default)]
pub struct WorkspaceInputs {
    /// `sidebar.visible` (View → Toggle Sidebar).
    pub sidebar_visible: bool,
    /// `file_history.is_some()`.
    pub file_history_open: bool,
    /// `ecosystem.is_some()`.
    pub ecosystem_open: bool,
    /// `branch_cleanup_open` (ADR-0128).
    pub branch_cleanup_open: bool,
    /// `loading_tab.is_some()`.
    pub loading: bool,
    /// `main_diff.is_some()`.
    pub diff_open: bool,
    /// `commit_panel_open` (the visibility gate; set by WIP-row click).
    pub commit_panel_open: bool,
    /// `commit_panel.is_some()` (the entity exists).
    pub commit_panel_present: bool,
    /// `compare_view.is_some()` (ADR-0026 compare mode; ADR-0121 B2).
    pub compare_open: bool,
    /// `inspector_visible` (View → Toggle Commit Details).
    pub inspector_visible: bool,
    /// A commit detail was resolved for the current selection.
    pub has_detail: bool,
    /// `editor_workspace.is_some()` (T-WS-EDITOR-001; derived, not a separate
    /// mode field — T-WS-EDITOR-005 finding #11). The most-upstream input:
    /// still beaten by the FileHistory/Ecosystem takeovers and by Loading,
    /// but beats `Diff` (Editor mode ignores `main_diff`) and overrides the
    /// right pane's CommitPanel/Inspector.
    pub editor_mode: bool,
}

/// Resolve the slot contents. This encodes, in one place, the precedence that
/// used to live in `render_body`'s branch ordering:
///
/// - center: FileHistory > Ecosystem > BranchCleanup > Loading > Editor > Diff > CommitList
/// - right:  hidden under a takeover; else Hunks (Editor mode) > CommitPanel
///   (when open AND the entity exists) > Compare/Inspector (when visible AND a
///   detail resolved; Compare replaces the Inspector body while a compare is
///   open — same gates, ADR-0121 B2) > Hidden. `commit_panel_open` without an
///   entity hides the panel *without* falling back to the Inspector
///   (pre-existing behavior, kept).
/// - left:   FileTree (Editor mode) or Navigator, unless toggled off
///   (independent of the center mode — takeovers replace center+right only).
pub fn resolve_workspace(i: &WorkspaceInputs) -> WorkspaceLayout {
    let center = if i.file_history_open {
        CenterPane::FileHistory
    } else if i.ecosystem_open {
        CenterPane::Ecosystem
    } else if i.branch_cleanup_open {
        CenterPane::BranchCleanup
    } else if i.loading {
        CenterPane::Loading
    } else if i.editor_mode {
        CenterPane::Editor
    } else if i.diff_open {
        CenterPane::Diff
    } else {
        CenterPane::CommitList
    };

    let left = if !i.sidebar_visible {
        LeftPane::Hidden
    } else if i.editor_mode {
        LeftPane::FileTree
    } else {
        LeftPane::Navigator
    };

    // ADR-0121 B1: "spans center + right" is now the registered item's `slot()`
    // metadata instead of a hard-coded variant list (same set: FileHistory,
    // Ecosystem).
    let takeover = center_item(center).is_some_and(|it| it.slot() == Slot::CenterTakeover);
    let right = if takeover {
        RightPane::Hidden
    } else if i.editor_mode {
        RightPane::Hunks
    } else if i.commit_panel_open {
        if i.commit_panel_present {
            RightPane::CommitPanel
        } else {
            RightPane::Hidden
        }
    } else if i.inspector_visible && i.has_detail {
        // ADR-0121 B2: an open compare replaces the Inspector body. Same
        // visibility gates — before the split the single Inspector arm
        // rendered the compare inputs itself.
        if i.compare_open {
            RightPane::Compare
        } else {
            RightPane::Inspector
        }
    } else {
        RightPane::Hidden
    };

    WorkspaceLayout {
        left,
        center,
        right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default inputs for the common case: repo open, nothing special.
    fn base() -> WorkspaceInputs {
        WorkspaceInputs {
            sidebar_visible: true,
            inspector_visible: true,
            has_detail: true,
            ..Default::default()
        }
    }

    #[test]
    fn default_is_navigator_list_inspector() {
        let l = resolve_workspace(&base());
        assert_eq!(l.left, LeftPane::Navigator);
        assert_eq!(l.center, CenterPane::CommitList);
        assert_eq!(l.right, RightPane::Inspector);
    }

    #[test]
    fn center_precedence_chain() {
        // FileHistory beats everything.
        let i = WorkspaceInputs {
            file_history_open: true,
            ecosystem_open: true,
            loading: true,
            diff_open: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).center, CenterPane::FileHistory);
        // Then Ecosystem.
        let i = WorkspaceInputs {
            file_history_open: false,
            ..i
        };
        assert_eq!(resolve_workspace(&i).center, CenterPane::Ecosystem);
        // Then Loading.
        let i = WorkspaceInputs {
            ecosystem_open: false,
            ..i
        };
        assert_eq!(resolve_workspace(&i).center, CenterPane::Loading);
        // Then Diff.
        let i = WorkspaceInputs {
            loading: false,
            ..i
        };
        assert_eq!(resolve_workspace(&i).center, CenterPane::Diff);
    }

    #[test]
    fn takeover_hides_right_but_keeps_sidebar() {
        let i = WorkspaceInputs {
            ecosystem_open: true,
            commit_panel_open: true,
            commit_panel_present: true,
            ..base()
        };
        let l = resolve_workspace(&i);
        assert_eq!(l.left, LeftPane::Navigator);
        assert_eq!(l.right, RightPane::Hidden);
    }

    #[test]
    fn commit_panel_beats_inspector() {
        let i = WorkspaceInputs {
            commit_panel_open: true,
            commit_panel_present: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::CommitPanel);
    }

    #[test]
    fn commit_panel_open_without_entity_hides_right_no_inspector_fallback() {
        let i = WorkspaceInputs {
            commit_panel_open: true,
            commit_panel_present: false,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hidden);
    }

    #[test]
    fn inspector_needs_visible_and_detail() {
        let i = WorkspaceInputs {
            inspector_visible: false,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hidden);
        let i = WorkspaceInputs {
            has_detail: false,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hidden);
    }

    #[test]
    fn sidebar_toggle_hides_left() {
        let i = WorkspaceInputs {
            sidebar_visible: false,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).left, LeftPane::Hidden);
    }

    // ── T-WS-EDITOR-001: Editor mode precedence ──────────────

    #[test]
    fn editor_mode_shows_file_tree_editor_hunks() {
        let i = WorkspaceInputs {
            editor_mode: true,
            ..base()
        };
        let l = resolve_workspace(&i);
        assert_eq!(l.left, LeftPane::FileTree);
        assert_eq!(l.center, CenterPane::Editor);
        assert_eq!(l.right, RightPane::Hunks);
    }

    #[test]
    fn editor_mode_ignores_open_diff() {
        // Editor mode ignores `main_diff` — center stays Editor, not Diff.
        let i = WorkspaceInputs {
            editor_mode: true,
            diff_open: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).center, CenterPane::Editor);
    }

    #[test]
    fn file_history_beats_editor_mode() {
        let i = WorkspaceInputs {
            editor_mode: true,
            file_history_open: true,
            ..base()
        };
        let l = resolve_workspace(&i);
        assert_eq!(l.center, CenterPane::FileHistory);
        // Takeover still hides the right panel even in Editor mode.
        assert_eq!(l.right, RightPane::Hidden);
        // Left is independent of the center takeover — still FileTree.
        assert_eq!(l.left, LeftPane::FileTree);
    }

    #[test]
    fn ecosystem_beats_editor_mode() {
        let i = WorkspaceInputs {
            editor_mode: true,
            ecosystem_open: true,
            ..base()
        };
        let l = resolve_workspace(&i);
        assert_eq!(l.center, CenterPane::Ecosystem);
        assert_eq!(l.right, RightPane::Hidden);
        assert_eq!(l.left, LeftPane::FileTree);
    }

    #[test]
    fn loading_beats_editor_mode() {
        let i = WorkspaceInputs {
            editor_mode: true,
            loading: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).center, CenterPane::Loading);
    }

    #[test]
    fn editor_mode_left_hidden_when_sidebar_toggled_off() {
        let i = WorkspaceInputs {
            editor_mode: true,
            sidebar_visible: false,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).left, LeftPane::Hidden);
    }

    #[test]
    fn editor_mode_hunks_beats_commit_panel_and_inspector() {
        let i = WorkspaceInputs {
            editor_mode: true,
            commit_panel_open: true,
            commit_panel_present: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hunks);

        let i = WorkspaceInputs {
            editor_mode: true,
            inspector_visible: true,
            has_detail: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hunks);
    }

    // ── ADR-0121 B2: Compare precedence ──────────────────────

    #[test]
    fn compare_replaces_inspector_with_same_gates() {
        // Compare wins over the plain Inspector...
        let i = WorkspaceInputs {
            compare_open: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Compare);
        // ...but only under the Inspector's own gates: hidden when the
        // inspector is toggled off or no detail resolved (pre-split behavior —
        // the compare rendered inside the Inspector arm).
        let i = WorkspaceInputs {
            inspector_visible: false,
            ..i
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hidden);
        let i = WorkspaceInputs {
            inspector_visible: true,
            has_detail: false,
            ..i
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hidden);
    }

    #[test]
    fn commit_panel_beats_compare() {
        let i = WorkspaceInputs {
            compare_open: true,
            commit_panel_open: true,
            commit_panel_present: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::CommitPanel);
    }

    #[test]
    fn takeover_hides_compare() {
        let i = WorkspaceInputs {
            compare_open: true,
            ecosystem_open: true,
            ..base()
        };
        assert_eq!(resolve_workspace(&i).right, RightPane::Hidden);
    }

    #[test]
    fn diff_keeps_right_panel() {
        // T-UI-003 + user request: the right panel stays visible while a diff
        // is open so files can be clicked through continuously.
        let i = WorkspaceInputs {
            diff_open: true,
            ..base()
        };
        let l = resolve_workspace(&i);
        assert_eq!(l.center, CenterPane::Diff);
        assert_eq!(l.right, RightPane::Inspector);
    }
}
