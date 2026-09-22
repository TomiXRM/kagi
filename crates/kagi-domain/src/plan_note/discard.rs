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
    /// blocker — the target is a submodule / gitlink (mode 160000). The discard
    /// backup-then-restore path cannot handle gitlinks (#324), so reject it at
    /// plan time rather than aborting the whole batch at the backup read.
    TargetSubmodule { path: String },
    /// warning — N untracked targets are deleted from disk (with ODB backup).
    UntrackedWillBeDeleted { count: usize },
}

impl DiscardNote {
    /// Byte-identical to the legacy discard.rs strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            DiscardNote::NothingSelected => "Nothing to discard: no files selected.".to_string(),
            DiscardNote::TargetConflicted { path } => {
                format!(crate::advice_template_en!(DiscardTargetConflicted), path)
            }
            DiscardNote::NoUnstagedChanges { path } => {
                format!("'{}' has no unstaged changes to discard.", path)
            }
            DiscardNote::TargetSubmodule { path } => {
                format!(crate::advice_template_en!(DiscardTargetSubmodule), path)
            }
            DiscardNote::UntrackedWillBeDeleted { count } => format!(
                crate::advice_template_en!(DiscardUntrackedWillBeDeleted),
                count
            ),
        }
    }
}
