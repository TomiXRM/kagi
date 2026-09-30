//! Per-row diff / changed-files cache cluster — T-DECOMP-002 (ADR-0118 Phase 5.2).
//!
//! Groups the five formerly-flat `KagiApp` cache fields so they move and
//! invalidate as a unit. Mirrors `src/ui/avatar.rs`'s `AvatarStore` layout
//! (Mechanism A — sub-struct consolidation).

use kagi_git::{FileDiff, FileDiffStat, FileStatus};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// ──────────────────────────────────────────────────────────────
// Diff / changed-files cache store
// ──────────────────────────────────────────────────────────────

/// Cohesive per-row diff / changed-files cache cluster (ADR-0118 Phase 5.2).
/// Read inside `KagiApp::render`; deliberately NOT an `Entity` (no notify-scope
/// to isolate — see ADR-0118 Mechanism A). Invalidated as a unit via `clear()`.
///
/// The three Inspector inputs (`changed_files` / `diffstat` / `generated`) are
/// private: every write goes through [`DiffCaches::insert_row`] or `clear`,
/// both of which advance [`DiffCaches::revision`] — the content revision the
/// Inspector's derived model is keyed on (issue #512).
#[derive(Clone, Default)]
pub struct DiffCaches {
    /// Changed-files list per commit row (`None` = load attempted but failed). (was `diff_cache`)
    changed_files: HashMap<usize, Option<Vec<FileStatus>>>,
    /// Per-(row, file-index) `FileDiff` content cache (T-REARCH-031).
    /// `changed_files` only holds the file *list*; without this content cache,
    /// toggling between two commits recomputes the full git2 tree-diff + hunk
    /// extraction every time. Key is `(selected_row, file_index)`. (was `file_diff_cache`)
    pub file_content: HashMap<(usize, usize), Arc<FileDiff>>,
    /// Rows whose REMOTE changed-files load is in flight over SSH (ADR-0089
    /// Phase 2c), so the render trigger spawns it only once. (was `remote_diff_inflight`)
    pub remote_inflight: HashSet<usize>,
    /// Rows whose LOCAL changed-files+diffstat load is in flight off the UI
    /// thread, so the render trigger spawns it only once. The local counterpart
    /// of `remote_inflight`. (was `local_diff_inflight`)
    pub local_inflight: HashSet<usize>,
    /// Per-row diffstat (additions/deletions) for the Inspector changed-files
    /// list (W16-DIFFSTAT). Computed lazily alongside `changed_files`. (was `diffstat_cache`)
    diffstat: HashMap<usize, Vec<FileDiffStat>>,
    /// Per-row "is generated" flags aligned with `changed_files` (issue #348 —
    /// auto-fold lockfiles / generated files). Computed lazily alongside
    /// `changed_files`; index i is `true` when that file is classified generated.
    generated: HashMap<usize, Vec<bool>>,
    /// Advanced by every write to the three row maps above.
    revision: u64,
}

impl DiffCaches {
    /// Drop every cached diff/changed-files entry as one unit. Single
    /// invalidation point for reloads and session activation so no field can
    /// be forgotten.
    pub fn clear(&mut self) {
        self.changed_files.clear();
        self.file_content.clear();
        self.remote_inflight.clear();
        self.local_inflight.clear();
        self.diffstat.clear();
        self.generated.clear();
        self.revision = self.revision.wrapping_add(1);
    }

    /// Record one row's changed files together with whatever diffstat and
    /// generated flags its load produced. An absent `stats` / `generated`
    /// leaves that row's previous entry in place.
    pub fn insert_row(
        &mut self,
        row: usize,
        files: Option<Vec<FileStatus>>,
        stats: Option<Vec<FileDiffStat>>,
        generated: Option<Vec<bool>>,
    ) {
        self.changed_files.insert(row, files);
        if let Some(stats) = stats {
            self.diffstat.insert(row, stats);
        }
        if let Some(generated) = generated {
            self.generated.insert(row, generated);
        }
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn changed_files(&self) -> &HashMap<usize, Option<Vec<FileStatus>>> {
        &self.changed_files
    }

    pub fn diffstat(&self) -> &HashMap<usize, Vec<FileDiffStat>> {
        &self.diffstat
    }

    pub fn generated(&self) -> &HashMap<usize, Vec<bool>> {
        &self.generated
    }

    /// Content revision of the row maps: equal revisions mean equal contents.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #286: caches are keyed by COMMIT ROW INDEX. After a graph renumber
    /// (tab switch / external reload / solo toggle) row 5 points at a *different*
    /// commit, so `clear()` — the single invalidation the fix routes every
    /// renumber through (`on_view_published` / `graph_solo`) — must drop every
    /// row-keyed field so a stale entry can never be served under a reused index.
    #[test]
    fn clear_drops_every_row_keyed_field() {
        let mut c = DiffCaches::default();
        c.changed_files.insert(5, None);
        c.diffstat.insert(5, Vec::new());
        c.generated.insert(5, Vec::new());
        c.remote_inflight.insert(5);
        c.local_inflight.insert(5);
        c.file_content.insert(
            (5, 0),
            Arc::new(FileDiff {
                old_path: None,
                new_path: None,
                change: kagi_git::ChangeKind::Modified,
                hunks: Vec::new(),
                is_binary: false,
            }),
        );

        c.clear();

        // Row 5 must no longer resolve to the old commit's data on ANY field.
        assert!(c.changed_files.is_empty());
        assert!(c.diffstat.is_empty());
        assert!(c.generated.is_empty());
        assert!(c.remote_inflight.is_empty());
        assert!(c.local_inflight.is_empty());
        assert!(c.file_content.is_empty());
        assert!(c.changed_files.get(&5).is_none());
    }
}
