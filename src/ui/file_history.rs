//! File History view state + small pure helpers (ADR-0089).
//!
//! The heavy rendering (`render_file_history_view`) lives in `ui::mod` so it can
//! reuse the private `render_main_diff_view` diff renderer.  This module holds
//! the [`FileHistoryState`] struct plus presentation helpers that are pure
//! enough to unit-test and keep out of the already-huge `mod.rs`.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{ListState, Pixels, Point, SharedString, WeakEntity};

use kagi_git::{FileChangeType, FileHistory, FileHistoryEntry, FileHistoryEntryKind};

use super::diff_view::MainDiffView;
use super::theme::theme;

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
    /// Diff of the selected entry, reusing the existing diff renderer.
    pub diff: Option<MainDiffView>,
    /// T-DIFF-WRAP-001: `ListState` (variable-height) for the diff viewer
    /// list — see `render_helpers::render_diff_list` for the item-count
    /// sync/reset lifecycle.
    pub diff_scroll: ListState,
    /// List/diff vertical split — fraction of the region given to the list.
    pub split: f32,
    /// Monotonic load generation, used to discard stale async *history* results.
    pub generation: u64,
    /// Monotonic per-diff request token, bumped on every `load_diff`. Discards a
    /// superseded async *diff* result (rapid arrowing — including A→B→A on a WIP
    /// entry whose working-tree contents can change between reads) instead of
    /// letting an older load overwrite the current selection's diff.
    pub diff_req: u64,
}

impl FileHistoryState {
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

/// ADR-0117: the File History panel promoted to its own `Entity<T>` (Phase 5.1).
///
/// Holds the [`FileHistoryState`] data plus the small amount of plumbing the
/// entity needs to drive itself and call back to the parent. The Backend-driving
/// logic (history + diff loads) lives on this entity (it holds `repo_path`) — see
/// the `impl FileHistoryView` in `file_history_render.rs` — so entity-initiated
/// actions update *self* and never re-enter `KagiApp.file_history` (which would
/// double-borrow the leased entity and panic). The only parent callbacks are
/// `close` and `jump_to_commit`, neither of which touches this entity's lease.
pub struct FileHistoryView {
    /// The view-model data (loaded history, selection, diff, split, generation).
    pub data: FileHistoryState,
    /// Weak back-reference to the parent. Used ONLY from event/listener closures
    /// (close / jump-to-commit) — NEVER read in a `Render` path (ADR-0117).
    pub(crate) app: WeakEntity<super::KagiApp>,
    /// Commit-row context menu: (entry index, anchor). Moved off `KagiApp`.
    pub menu: Option<(usize, Point<Pixels>)>,
    /// Shared with `KagiApp.file_history_geom` (the *same* `Rc<Cell>`): the
    /// render writes the measured list+diff screen bounds here; the divider-drag
    /// handler (on `KagiApp`) reads them so the list/diff split maps exactly.
    pub geom: Rc<Cell<(f32, f32)>>,
    /// Right detail-pane width, synced from `KagiApp.panel_width` (the FH detail
    /// divider drags `panel_width`, which `KagiApp` pushes back into the entity).
    pub panel_width: f32,
    /// Repo root for this FH session. Constant for the entity's life (FH closes
    /// on repo/tab switch). Used for the read-only history + diff loads.
    pub(crate) repo_path: PathBuf,
}

impl FileHistoryView {
    /// Construct the entity from freshly-built (loading) state. Created in
    /// `KagiApp::open_file_history` via `cx.new`; the caller then kicks off the
    /// initial load via [`FileHistoryView::start_load`].
    pub fn new(
        data: FileHistoryState,
        app: WeakEntity<super::KagiApp>,
        geom: Rc<Cell<(f32, f32)>>,
        panel_width: f32,
        repo_path: PathBuf,
    ) -> Self {
        Self {
            data,
            app,
            menu: None,
            geom,
            panel_width,
            repo_path,
        }
    }
}

/// The one-letter change badge + its colour for a history entry.
///
/// WIP rows render an orange `WIP`-style `●`; commit rows use the per-type
/// letter (A/M/D/R/C) coloured from the theme's change palette.
pub fn entry_badge(entry: &FileHistoryEntry) -> (&'static str, u32) {
    if entry.kind == FileHistoryEntryKind::Wip {
        return ("●", theme().color_warning);
    }
    change_type_badge(entry.change.change_type)
}

/// Map a [`FileChangeType`] to its display letter + colour.
pub fn change_type_badge(ct: FileChangeType) -> (&'static str, u32) {
    let t = theme();
    match ct {
        FileChangeType::Added => ("A", t.change_added),
        FileChangeType::Modified => ("M", t.change_modified),
        FileChangeType::Deleted => ("D", t.change_deleted),
        FileChangeType::Renamed => ("R", t.change_renamed),
        // Copied has no dedicated theme colour; reuse the rename (purple/blue).
        FileChangeType::Copied => ("C", t.change_renamed),
        FileChangeType::Unknown => ("?", t.text_muted),
    }
}

/// Human label for a change type (used in the diff banner / detail pane).
pub fn change_type_label(ct: FileChangeType) -> &'static str {
    match ct {
        FileChangeType::Added => "Added",
        FileChangeType::Modified => "Modified",
        FileChangeType::Deleted => "Deleted",
        FileChangeType::Renamed => "Renamed",
        FileChangeType::Copied => "Copied",
        FileChangeType::Unknown => "Changed",
    }
}

/// Parse an ISO-8601 / RFC-3339 timestamp (`git --date=iso-strict`, e.g.
/// `2026-01-02T15:04:05+09:00`) into seconds since the Unix epoch.
///
/// A tiny hand-rolled parser — the project has no chrono dependency in the UI
/// layer, and the format is fixed by our own `git log` invocation.  Returns
/// `None` on any malformed input so callers can fall back gracefully.
pub fn iso_to_epoch(s: &str) -> Option<i64> {
    let s = s.trim();
    // Expect at least "YYYY-MM-DDTHH:MM:SS".
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: i64 = s.get(5..7)?.parse().ok()?;
    let day: i64 = s.get(8..10)?.parse().ok()?;
    let hour: i64 = s.get(11..13)?.parse().ok()?;
    let min: i64 = s.get(14..16)?.parse().ok()?;
    let sec: i64 = s.get(17..19)?.parse().ok()?;

    // Days from the civil date (Howard Hinnant's algorithm), giving days since
    // 1970-01-01.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (month + 9) % 12; // [0, 11], Mar=0
    let doy = (153 * mp + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    let days = era * 146_097 + doe - 719_468;

    let mut secs = days * 86_400 + hour * 3_600 + min * 60 + sec;

    // Timezone offset suffix: 'Z' (UTC) or ±HH:MM / ±HHMM.
    if let Some(off) = s.get(19..) {
        let off = off.trim();
        if !off.is_empty() && off != "Z" && off != "z" {
            let sign = match off.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => 0,
            };
            if sign != 0 {
                let rest = &off[1..];
                let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.len() >= 4 {
                    let oh: i64 = digits[0..2].parse().ok()?;
                    let om: i64 = digits[2..4].parse().ok()?;
                    secs -= sign * (oh * 3_600 + om * 60);
                } else if digits.len() >= 2 {
                    let oh: i64 = digits[0..2].parse().ok()?;
                    secs -= sign * oh * 3_600;
                }
            }
        }
    }

    Some(secs)
}

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
