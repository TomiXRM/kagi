//! Discard-file plan notes (ADR-0129 appendix §B-11) — the first structured
//! producer (Phase 1).

/// Discard-file plan notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscardNote {
    /// blocker — no files were selected (no-op family).
    NothingSelected,
    /// blocker — the target file is conflicted.
    TargetConflicted { path: String },
    /// blocker — the target file has no unstaged changes.
    NoUnstagedChanges { path: String },
    /// warning — N untracked targets are deleted from disk (with ODB backup).
    UntrackedWillBeDeleted { count: usize },
}

impl DiscardNote {
    /// Byte-identical to the legacy discard.rs strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            DiscardNote::NothingSelected => "Nothing to discard: no files selected.".to_string(),
            DiscardNote::TargetConflicted { path } => format!(
                "'{}' is conflicted. Resolve the conflict instead of discarding it.",
                path
            ),
            DiscardNote::NoUnstagedChanges { path } => {
                format!("'{}' has no unstaged changes to discard.", path)
            }
            DiscardNote::UntrackedWillBeDeleted { count } => format!(
                "⚠️ {} untracked file(s) will be PERMANENTLY DELETED from disk (and any \
                 now-empty folders removed). A backup blob is saved to the oplog first — \
                 recover with `git cat-file -p <blob-sha>`.",
                count
            ),
        }
    }
}
