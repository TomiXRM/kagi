//! JA strings for `WorktreeNote` (filled by the ops/worktree fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::WorktreeNote;

/// Japanese rendering of one worktree note.
pub fn note_ja(note: &WorktreeNote) -> String {
    match *note {}
}
