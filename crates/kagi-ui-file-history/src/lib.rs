//! kagi-ui-file-history (ADR-0121 Phase C3): the File History pane, extracted
//! from the bin's `src/ui/file_history{,_render}.rs`.
//!
//! Git-free by construction (CI grep gate): data flows **inward** as seeded
//! [`kagi_domain::file_history::FileHistory`] results, and **outward** as
//! [`FileHistoryEvent`]s the bin subscribes to. The history and diff loads
//! themselves (which need the Git backend) are host-owned: the pane emits
//! `HistoryLoadRequested` / `DiffLoadRequested` and the host marshals results
//! back via [`FileHistoryView::seed_history`] (history) or by updating the
//! host-provided diff pane entity (diff).
//!
//! The diff viewer is deliberately NOT in this crate: it reuses the bin's
//! shared `MainDiffView` / `render_diff_list` pipeline, so the host passes an
//! opaque `AnyView` (`diff_pane`) that this crate embeds below the commit list.

mod detail;
mod render;

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{AnyView, Context, EventEmitter, Pixels, Point, SharedString};

use kagi_domain::commit::CommitId;
use kagi_domain::file_history::{FileHistory, FileHistoryEntry, FileHistoryEntryKind};
use kagi_ui_core::klog;
use kagi_ui_core::theme;

pub use render::render_file_history_view;

/// Full row height for a File History commit row.
///
/// ponytail: copy of the bin's `row_height(false)` (= `graph_view::ROW_H` =
/// 24.0 * 1.2); keep in sync if the graph row height ever changes.
const ROW_H_FULL: f32 = 29.0;

/// Zoom-scaled height of a File History commit row (matches the bin's
/// `row_height(false)`).
pub(crate) fn fh_row_height() -> f32 {
    theme::scaled(ROW_H_FULL)
}

/// State backing the File History center view (ADR-0089).
///
/// `history == None && error == None` means the load is in flight (Loading).
pub struct FileHistoryState {
    /// Target file, repo-relative (the path the view was opened for).
    pub rel_path: PathBuf,
    /// Branch the view was opened against (display only).
    pub branch: SharedString,
    /// Whether rename-following is enabled (`git log --follow`).
    pub follow_renames: bool,
    /// Loaded history; `None` while loading or on error.
    pub history: Option<FileHistory>,
    /// Error detail, if the load failed.
    pub error: Option<String>,
    /// Index of the selected entry within `history.entries`.
    pub selected: usize,
    /// List/diff vertical split — fraction of the region given to the list.
    pub split: f32,
    /// Monotonic load generation, used to discard stale async *history* results.
    pub generation: u64,
}

impl FileHistoryState {
    /// Fresh (Loading) state for `rel_path`, with the ADR-0089 defaults
    /// (follow renames on, 25% list split).
    pub fn new(rel_path: PathBuf, branch: SharedString) -> Self {
        Self {
            rel_path,
            branch,
            follow_renames: true,
            history: None,
            error: None,
            selected: 0,
            split: 0.25,
            generation: 0,
        }
    }

    /// `true` while the async load is still in flight.
    pub fn is_loading(&self) -> bool {
        self.history.is_none() && self.error.is_none()
    }

    /// The currently-selected entry, if any.
    pub fn selected_entry(&self) -> Option<&FileHistoryEntry> {
        self.history
            .as_ref()
            .and_then(|h| h.entries.get(self.selected))
    }

    /// `true` when the file is untracked: the only entry is a WIP/untracked
    /// row and there are no committed entries.
    pub fn is_untracked(&self) -> bool {
        match &self.history {
            Some(h) => {
                !h.entries.is_empty()
                    && h.entries
                        .iter()
                        .all(|e| e.kind == FileHistoryEntryKind::Wip)
            }
            None => false,
        }
    }

    /// `true` when the load finished with no entries at all.
    pub fn is_empty(&self) -> bool {
        matches!(&self.history, Some(h) if h.entries.is_empty())
    }

    /// Number of committed entries (excludes the synthetic WIP row).
    pub fn commit_count(&self) -> usize {
        self.history
            .as_ref()
            .map(|h| {
                h.entries
                    .iter()
                    .filter(|e| e.kind == FileHistoryEntryKind::Commit)
                    .count()
            })
            .unwrap_or(0)
    }
}

/// The pane's outward surface (ADR-0121 C3): everything File History needs
/// from the app travels as one of these events. The bin subscribes in
/// `open_file_history` and maps them onto `KagiApp`.
#[derive(Debug, Clone)]
pub enum FileHistoryEvent {
    /// Back / Open File clicked → host drops the entity (ADR-0117 shape kept).
    CloseRequested,
    /// Double-click / Open Commit / Show in Graph → host closes the pane and
    /// jumps the commit graph to this commit.
    JumpToCommit(CommitId),
    /// A history (re)load is needed. The host runs the Git-side
    /// `file_history` query off-thread and marshals the result back via
    /// [`FileHistoryView::seed_history`], echoing `generation` / `origin` /
    /// `emit_loaded` so the seed can guard staleness and keep the `[kagi]`
    /// log contract.
    HistoryLoadRequested {
        /// The pane's load generation at request time (staleness guard).
        generation: u64,
        /// Commit to re-select after the load (reload keeps the selection).
        origin: Option<CommitId>,
        /// Whether the load should emit `[kagi] file-history: loaded N entries`
        /// (initial open / Refresh: yes; follow-toggle reload: no).
        emit_loaded: bool,
    },
    /// The selection changed (or a load landed) → host loads the selected
    /// entry's diff into the `diff_pane` slot.
    DiffLoadRequested,
}

/// ADR-0117: the File History panel promoted to its own `Entity<T>` (Phase 5.1),
/// extracted to this crate in ADR-0121 C3 (the `WeakEntity<KagiApp>` back-ref
/// became [`FileHistoryEvent`]s).
pub struct FileHistoryView {
    /// The view-model data (loaded history, selection, split, generation).
    pub data: FileHistoryState,
    /// Commit-row context menu: (entry index, anchor). Moved off `KagiApp`.
    pub menu: Option<(usize, Point<Pixels>)>,
    /// Shared with `KagiApp.file_history_geom` (the *same* `Rc<Cell>`): the
    /// render writes the measured list+diff screen bounds here; the divider-drag
    /// handler (on `KagiApp`) reads them so the list/diff split maps exactly.
    pub geom: Rc<Cell<(f32, f32)>>,
    /// Right detail-pane width, synced from `KagiApp.panel_width` (the FH detail
    /// divider drags `panel_width`, which `KagiApp` pushes back into the entity).
    pub panel_width: f32,
    /// Host-provided diff viewer (the bin's `FhDiffPane` entity, which renders
    /// the shared `MainDiffView` pipeline or its "no diff" placeholder). This
    /// crate only embeds it below the commit list; the host fills it on
    /// [`FileHistoryEvent::DiffLoadRequested`].
    pub diff_pane: AnyView,
    /// Resolved commit-author avatars, pushed in by the host each frame
    /// (`FileHistoryItem::render`) — this crate can't reach `KagiApp.avatars`,
    /// and the map fills in asynchronously as the background resolution pass
    /// lands. Empty is a fine steady state: the detail pane's shared commit
    /// header just draws initial circles.
    pub avatars: kagi_ui_core::avatar::AvatarImages,
}

impl EventEmitter<FileHistoryEvent> for FileHistoryView {}

impl FileHistoryView {
    /// Construct the entity from freshly-built (loading) state. Created in
    /// `KagiApp::open_file_history` via `cx.new`; the caller then subscribes to
    /// [`FileHistoryEvent`] and kicks off the initial load via
    /// [`FileHistoryView::request_load`].
    pub fn new(
        data: FileHistoryState,
        geom: Rc<Cell<(f32, f32)>>,
        panel_width: f32,
        diff_pane: AnyView,
    ) -> Self {
        Self {
            data,
            menu: None,
            geom,
            panel_width,
            diff_pane,
            avatars: Default::default(),
        }
    }

    /// Ask the host to run the async history load for the current `rel_path` /
    /// `follow_renames` (initial open / reload). The host answers via
    /// [`FileHistoryView::seed_history`].
    pub fn request_load(
        &mut self,
        origin: Option<CommitId>,
        emit_loaded: bool,
        cx: &mut Context<Self>,
    ) {
        cx.emit(FileHistoryEvent::HistoryLoadRequested {
            generation: self.data.generation,
            origin,
            emit_loaded,
        });
    }

    /// Marshal a finished history load back into the pane, guarded by the
    /// per-entity `generation` so a superseded load (rapid refresh) is
    /// discarded.
    ///
    /// `emit_loaded` preserves the pre-extraction contract: the initial open and
    /// Refresh emit `[kagi] file-history: loaded N entries`; the follow-toggle
    /// reload does NOT (it only emits the `open` line).
    pub fn seed_history(
        &mut self,
        generation: u64,
        result: Result<FileHistory, String>,
        origin: Option<CommitId>,
        emit_loaded: bool,
        cx: &mut Context<Self>,
    ) {
        // Per-entity generation guard: discard a superseded load.
        if self.data.generation != generation {
            return;
        }
        match result {
            Ok(history) => {
                if emit_loaded {
                    klog!("file-history: loaded {} entries", history.entries.len());
                }
                let initial = Self::pick_initial_index(&history, &origin);
                self.data.history = Some(history);
                self.data.error = None;
                self.data.selected = initial;
                cx.emit(FileHistoryEvent::DiffLoadRequested);
            }
            Err(e) => {
                self.data.history = None;
                self.data.error = Some(e);
            }
        }
        cx.notify();
    }

    /// Re-run the history load for the current file (Refresh / Retry / Follow
    /// toggle), preserving the current selection's commit as the re-selection
    /// origin. Bumps `generation` so any in-flight older load is discarded (the
    /// host additionally clears + invalidates the diff pane on the request).
    pub fn reload(&mut self, emit_loaded: bool, cx: &mut Context<Self>) {
        let origin = self
            .data
            .selected_entry()
            .and_then(|e| e.commit.as_ref())
            .map(|c| CommitId(c.full_hash.clone()));
        klog!("file-history: open {}", self.data.rel_path.display());
        self.data.history = None;
        self.data.error = None;
        self.data.selected = 0;
        self.data.generation = self.data.generation.wrapping_add(1);
        cx.notify();
        self.request_load(origin, emit_loaded, cx);
    }

    /// Select a history entry and request its diff load.
    pub fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        let valid = self
            .data
            .history
            .as_ref()
            .is_some_and(|h| index < h.entries.len());
        if !valid {
            return;
        }
        self.data.selected = index;
        cx.emit(FileHistoryEvent::DiffLoadRequested);
        cx.notify();
    }

    /// Move the entry selection up/down by `delta` (arrow keys), clamped.
    pub fn step(&mut self, delta: i64, cx: &mut Context<Self>) {
        let len = self
            .data
            .history
            .as_ref()
            .map(|h| h.entries.len())
            .unwrap_or(0);
        if len == 0 {
            return;
        }
        let cur = self.data.selected;
        let next = (cur as i64 + delta).clamp(0, len as i64 - 1) as usize;
        if next != cur {
            self.select(next, cx);
        }
    }

    /// Update the list/diff vertical split ratio (divider drag), child-scoped.
    pub fn set_split(&mut self, ratio: f32, cx: &mut Context<Self>) {
        if (ratio - self.data.split).abs() > 0.002 {
            self.data.split = ratio;
            cx.notify();
        }
    }

    /// Choose the initial selected index: the WIP row (always index 0 if
    /// present), else the `origin` commit if it appears, else the newest entry.
    fn pick_initial_index(history: &FileHistory, origin: &Option<CommitId>) -> usize {
        if let Some(first) = history.entries.first() {
            if first.kind == FileHistoryEntryKind::Wip {
                return 0;
            }
        }
        if let Some(id) = origin {
            if let Some(ix) = history.entries.iter().position(|e| {
                e.commit
                    .as_ref()
                    .is_some_and(|c| c.full_hash == id.0 || c.full_hash.starts_with(&id.0))
            }) {
                return ix;
            }
        }
        0
    }
}

// ADR-0121: the change-badge trio (`entry_badge` / `change_type_badge` /
// `change_type_label`) moved verbatim to `kagi-ui-core` so
// `kagi-ui-editor`'s History tab can badge its own commit rows with the same
// letters and colours — sibling pane crates can't depend on each other, only
// on `kagi-ui-core`. Re-exported here so existing
// `kagi_ui_file_history::entry_badge` paths keep resolving (same shim recipe
// as `theme`/`i18n`/`settings` in the bin).
pub use kagi_ui_core::change_badge::{change_type_badge, change_type_label, entry_badge};

// ADR-0121: `iso_to_epoch` moved to `kagi-ui-core::time_parse` so the shared
// commit-row model can compute relative dates. Re-exported for existing paths.
pub use kagi_ui_core::time_parse::iso_to_epoch;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_utc_epoch() {
        // 1970-01-01T00:00:00Z == 0
        assert_eq!(iso_to_epoch("1970-01-01T00:00:00Z"), Some(0));
        // 2000-01-01T00:00:00Z == 946684800
        assert_eq!(iso_to_epoch("2000-01-01T00:00:00Z"), Some(946_684_800));
    }

    #[test]
    fn iso_with_offset() {
        // 2000-01-01T09:00:00+09:00 is the same instant as 00:00:00Z.
        assert_eq!(iso_to_epoch("2000-01-01T09:00:00+09:00"), Some(946_684_800));
        assert_eq!(
            iso_to_epoch("2000-01-01T00:00:00-05:00"),
            Some(946_684_800 + 5 * 3_600)
        );
    }

    #[test]
    fn iso_malformed_is_none() {
        assert_eq!(iso_to_epoch(""), None);
        assert_eq!(iso_to_epoch("not-a-date"), None);
    }
}
