//! Conflict Editor plan notes (ADR-0129 Phase 2) — `crates/kagi-git/src/conflicts.rs`.
//!
//! This producer lives **outside** `ops/` (discovered mid-Phase-1, see the
//! appendix's `conflicts.rs(ops 外)` row) but follows the same pipeline: it
//! plans `continue` (finish resolving — merge/rebase/cherry-pick/revert),
//! `abort` (bail out, restoring `ORIG_HEAD`), and `skip` (sequencer-only, drop
//! the current step). `continue`'s blockers come from the ADR-0067
//! [`ContinueBlocker`](../../../kagi_git/enum.ContinueBlocker.html) checklist,
//! rendered here 1:1 (the UI's separate gate-reason `Msg` mapping in
//! `src/ui/conflict_view.rs::blocker_msg` is untouched — it keys off the same
//! `ContinueBlocker` enum directly and is out of scope for this note).

/// Plan notes for the conflicts (continue/abort/skip) op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictsNote {
    /// blocker (continue) — one or more files have no resolution draft.
    UnresolvedFiles { files: Vec<String> },
    /// blocker (continue) — one or more resolved buffer texts still contain
    /// conflict markers.
    MarkerResidue { files: Vec<String> },
    /// blocker (continue) — the index has unmerged entries the session does
    /// not track.
    IndexUnmerged { files: Vec<String> },
    /// blocker (continue) — a binary conflict has no side chosen.
    BinaryUnresolved { files: Vec<String> },
    /// blocker (continue) — a modify/delete or rename/delete file's
    /// keep-or-delete decision is still pending.
    DeletionUndecided { files: Vec<String> },
    /// blocker (continue) — a merge commit is required but its message is
    /// empty.
    EmptyMergeMessage,
    /// blocker (continue) — the commit checklist (ADR-0043) reports a hard
    /// blocker; the message is passed through verbatim (checklist prose is
    /// out of scope for this migration, mirrors `CommonNote::GitErrorPassthrough`).
    ChecklistBlocker { message: String },
    /// warning (continue) — no conflicting files were detected; continue will
    /// finish the operation as-is.
    NoConflictingFilesDetected,
    /// warning (abort) — the partial resolution buffer is preserved, not
    /// discarded.
    PartialResolutionsPreserved,
    /// warning (skip) — the current sequencer step's changes are discarded,
    /// but the partial resolution is preserved.
    SkipDiscardsStep,
}

impl ConflictsNote {
    /// Byte-identical to the legacy `conflicts.rs` strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            ConflictsNote::UnresolvedFiles { files } => format!(
                "{} file(s) still unresolved: {}. Resolve every file before continuing.",
                files.len(),
                files.join(", ")
            ),
            ConflictsNote::MarkerResidue { files } => format!(
                "Conflict marker(s) remain in: {}. Remove all <<<<<<< ======= >>>>>>> markers before continuing.",
                files.join(", ")
            ),
            ConflictsNote::IndexUnmerged { files } => format!(
                "The index still has unmerged entries not tracked by this session: {}. Re-scan the repository.",
                files.join(", ")
            ),
            ConflictsNote::BinaryUnresolved { files } => format!(
                "Binary conflict(s) still need a side chosen: {}.",
                files.join(", ")
            ),
            ConflictsNote::DeletionUndecided { files } => format!(
                "Keep-or-delete decision still pending for: {}.",
                files.join(", ")
            ),
            ConflictsNote::EmptyMergeMessage => {
                "The merge commit message is empty. Provide a commit message before continuing."
                    .to_string()
            }
            ConflictsNote::ChecklistBlocker { message } => message.clone(),
            ConflictsNote::NoConflictingFilesDetected => {
                "No conflicting files detected; continue will finish the operation as-is."
                    .to_string()
            }
            ConflictsNote::PartialResolutionsPreserved => {
                "Your partial resolutions are preserved in the autosave directory and \
                 referenced in the operation log; they are not discarded."
                    .to_string()
            }
            ConflictsNote::SkipDiscardsStep => {
                "Skip discards the current step's changes (the conflicting pick is dropped, \
                 not committed). Your partial resolution is preserved in the autosave directory."
                    .to_string()
            }
        }
    }
}

/// Plan titles for the conflicts op family — `Continue {op}` / `Abort {op}` /
/// `Skip {op} step`, where `op` is [`ConflictOp::slug()`](../../../kagi_git/enum.ConflictOp.html#method.slug)
/// (`"merge"` / `"rebase"` / `"cherry-pick"` / `"revert"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictsTitle {
    /// `Continue {op}` — finish resolving (sequencer `--continue` plan, or the
    /// merge commit-panel route which never reaches this title).
    Continue { op: String },
    /// `Abort {op}` — bail out to the pre-operation state.
    Abort { op: String },
    /// `Skip {op} step` — sequencer-only: drop the current step.
    Skip { op: String },
}

impl ConflictsTitle {
    /// Byte-identical to the legacy `conflicts.rs` title strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            ConflictsTitle::Continue { op } => format!("Continue {}", op),
            ConflictsTitle::Abort { op } => format!("Abort {}", op),
            ConflictsTitle::Skip { op } => format!("Skip {} step", op),
        }
    }
}

/// Recovery kinds for the conflicts op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictsRecovery {
    /// continue: abort back to the pre-operation state via `git {op} --abort`.
    Continue { op: String },
    /// abort: restores the pre-`{op}` state from `ORIG_HEAD`.
    Abort { op: String },
    /// skip: drops the current `{op}` step.
    Skip { op: String },
}

impl ConflictsRecovery {
    /// Byte-identical to the legacy `conflicts.rs` recovery strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            ConflictsRecovery::Continue { op } => format!(
                "If the continuation goes wrong you can abort back to the pre-operation state:\n  git {} --abort\nThe pre-operation HEAD is recorded in ORIG_HEAD and the reflog.",
                op
            ),
            ConflictsRecovery::Abort { op } => format!(
                "Abort restores the pre-{} state from ORIG_HEAD. If you change your mind, the reflog still records every HEAD movement.",
                op
            ),
            ConflictsRecovery::Skip { op } => format!(
                "Skip drops the current {} step. The reflog still records every HEAD movement, and the pre-operation HEAD is in ORIG_HEAD if you need to abort entirely.",
                op
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── message_en golden tests (ADR-0129 §3): every variant, dynamic values,
    //    joined file lists, `\n` recovery bodies, byte-exact vs. the legacy
    //    `conflicts.rs` producer strings. ──

    #[test]
    fn unresolved_files_note() {
        assert_eq!(
            ConflictsNote::UnresolvedFiles {
                files: vec!["a.rs".to_string()]
            }
            .message_en(),
            "1 file(s) still unresolved: a.rs. Resolve every file before continuing."
        );
        assert_eq!(
            ConflictsNote::UnresolvedFiles {
                files: vec!["a.rs".to_string(), "b/c.rs".to_string()]
            }
            .message_en(),
            "2 file(s) still unresolved: a.rs, b/c.rs. Resolve every file before continuing."
        );
    }

    #[test]
    fn marker_residue_note() {
        assert_eq!(
            ConflictsNote::MarkerResidue {
                files: vec!["file.txt".to_string()]
            }
            .message_en(),
            "Conflict marker(s) remain in: file.txt. Remove all <<<<<<< ======= >>>>>>> markers before continuing."
        );
    }

    #[test]
    fn index_unmerged_note() {
        assert_eq!(
            ConflictsNote::IndexUnmerged {
                files: vec!["x.rs".to_string()]
            }
            .message_en(),
            "The index still has unmerged entries not tracked by this session: x.rs. Re-scan the repository."
        );
    }

    #[test]
    fn binary_unresolved_note() {
        assert_eq!(
            ConflictsNote::BinaryUnresolved {
                files: vec!["img.png".to_string()]
            }
            .message_en(),
            "Binary conflict(s) still need a side chosen: img.png."
        );
    }

    #[test]
    fn deletion_undecided_note() {
        assert_eq!(
            ConflictsNote::DeletionUndecided {
                files: vec!["old/path.rs".to_string()]
            }
            .message_en(),
            "Keep-or-delete decision still pending for: old/path.rs."
        );
    }

    #[test]
    fn empty_merge_message_note() {
        assert_eq!(
            ConflictsNote::EmptyMergeMessage.message_en(),
            "The merge commit message is empty. Provide a commit message before continuing."
        );
    }

    #[test]
    fn checklist_blocker_passthrough() {
        assert_eq!(
            ConflictsNote::ChecklistBlocker {
                message: "custom checklist prose".to_string()
            }
            .message_en(),
            "custom checklist prose"
        );
    }

    #[test]
    fn no_conflicting_files_detected_note() {
        assert_eq!(
            ConflictsNote::NoConflictingFilesDetected.message_en(),
            "No conflicting files detected; continue will finish the operation as-is."
        );
    }

    #[test]
    fn partial_resolutions_preserved_note() {
        assert_eq!(
            ConflictsNote::PartialResolutionsPreserved.message_en(),
            "Your partial resolutions are preserved in the autosave directory and \
             referenced in the operation log; they are not discarded."
        );
    }

    #[test]
    fn skip_discards_step_note() {
        assert_eq!(
            ConflictsNote::SkipDiscardsStep.message_en(),
            "Skip discards the current step's changes (the conflicting pick is dropped, \
             not committed). Your partial resolution is preserved in the autosave directory."
        );
    }

    #[test]
    fn titles_all_ops() {
        for op in ["merge", "rebase", "cherry-pick", "revert"] {
            assert_eq!(
                ConflictsTitle::Continue { op: op.to_string() }.message_en(),
                format!("Continue {}", op)
            );
            assert_eq!(
                ConflictsTitle::Abort { op: op.to_string() }.message_en(),
                format!("Abort {}", op)
            );
            assert_eq!(
                ConflictsTitle::Skip { op: op.to_string() }.message_en(),
                format!("Skip {} step", op)
            );
        }
    }

    #[test]
    fn continue_recovery_matches_legacy_string() {
        assert_eq!(
            ConflictsRecovery::Continue {
                op: "cherry-pick".to_string()
            }
            .message_en(),
            "If the continuation goes wrong you can abort back to the pre-operation state:\n  \
             git cherry-pick --abort\nThe pre-operation HEAD is recorded in ORIG_HEAD and the reflog."
        );
    }

    #[test]
    fn abort_recovery_matches_legacy_string() {
        assert_eq!(
            ConflictsRecovery::Abort {
                op: "rebase".to_string()
            }
            .message_en(),
            "Abort restores the pre-rebase state from ORIG_HEAD. If you change your mind, \
             the reflog still records every HEAD movement."
        );
    }

    #[test]
    fn skip_recovery_matches_legacy_string() {
        assert_eq!(
            ConflictsRecovery::Skip {
                op: "revert".to_string()
            }
            .message_en(),
            "Skip drops the current revert step. The reflog still records every HEAD movement, \
             and the pre-operation HEAD is in ORIG_HEAD if you need to abort entirely."
        );
    }
}
