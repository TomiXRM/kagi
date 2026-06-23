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
#[derive(Default)]
pub struct DiffCaches {
    /// Changed-files list per commit row (`None` = load attempted but failed). (was `diff_cache`)
    pub changed_files: HashMap<usize, Option<Vec<FileStatus>>>,
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
    pub diffstat: HashMap<usize, Vec<FileDiffStat>>,
}

impl DiffCaches {
    /// Drop every cached diff/changed-files entry as one unit. Single
    /// invalidation point for `reload` / `reload_external` /
    /// `reset_per_repo_ui` / `show_welcome` so no field can be forgotten.
    pub fn clear(&mut self) {
        self.changed_files.clear();
        self.file_content.clear();
        self.remote_inflight.clear();
        self.local_inflight.clear();
        self.diffstat.clear();
    }
}
