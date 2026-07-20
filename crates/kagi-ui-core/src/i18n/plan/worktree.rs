//! JA strings for `WorktreeNote` (filled by the ops/worktree fan-out PR — ADR-0129 Phase 2).

use kagi_domain::plan_note::{WorktreeNote, WorktreeRecovery, WorktreeTitle};

/// Japanese rendering of one worktree note.
pub fn note_ja(note: &WorktreeNote) -> String {
    match *note {}
}

/// Japanese rendering of one worktree title.
pub fn title_ja(title: &WorktreeTitle) -> String {
    match *title {}
}

/// Japanese rendering of one worktree recovery block.
pub fn recovery_ja(recovery: &WorktreeRecovery) -> String {
    match *recovery {}
}
