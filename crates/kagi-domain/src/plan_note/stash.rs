//! StashNote / StashTitle / StashRecovery — ADR-0129 appendix §B-7
//! (+ §A conflicted-files cross-op, §C title row, §D recovery row).
//!
//! Covers the five plan producers in `crates/kagi-git/src/ops/stash.rs`:
//! `plan_stash_push`, `plan_stash_apply`, `plan_stash_pop`,
//! `plan_stash_drop_remote` (SSH), and `plan_stash_drop`. Cross-op notes
//! (conflicted files) are NOT duplicated here — `plan_stash_push` /
//! `plan_stash_apply` / `plan_stash_pop` map to the existing
//! `CommonNote::ConflictedFiles` variant (appendix §A1) with
//! `OpPhrase::Stashing` / `OpPhrase::ApplyingAStash` from the ops file
//! directly.

use super::DirtyParts;

/// Which stash op a dirty-working-tree blocker fired for (§B-7: same
/// sentence shape, only the op word differs — "apply" vs "pop").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StashDirtyOp {
    /// "…stash apply is only allowed…"
    Apply,
    /// "…stash pop is only allowed…"
    Pop,
}

/// Plan notes for the stash op family (ADR-0129 appendix §B-7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StashNote {
    /// blocker (`plan_stash_push`, no-op family): working tree is already
    /// clean — nothing to stash.
    NothingToStash,
    /// warning (`plan_stash_push`, `include_untracked=true`): untracked
    /// files will be swept into the stash.
    UntrackedIncluded { count: usize },
    /// warning (`plan_stash_push`, `include_untracked=false`): untracked
    /// files are left behind in the working tree.
    UntrackedExcluded { count: usize },
    /// blocker (`plan_stash_apply` / `plan_stash_pop` / `plan_stash_drop`):
    /// `index` names a stash entry that doesn't exist. The singular/plural
    /// "entry"/"entries" tail is computed inside `message_en` — JA needs no
    /// singular/plural form, so callers never branch on it (this is the
    /// intended example of collapsing 3 call sites into 1 keyed variant).
    IndexOutOfRange { index: usize, count: usize },
    /// blocker (`plan_stash_apply` / `plan_stash_pop`): the working tree has
    /// staged or unstaged changes, which apply/pop refuse to run against.
    DirtyBlocksApply { parts: DirtyParts, op: StashDirtyOp },
    /// blocker (`plan_stash_pop`): the in-memory merge of the stash commit
    /// with HEAD predicts conflicts; pop is refused so the stash entry is
    /// not lost (recommends apply instead).
    PopWouldConflict { count: usize, files: Vec<String> },
    /// warning (`plan_stash_drop_remote`, SSH): the remote drop cannot be
    /// undone from Kagi.
    RemoteDropIrreversible,
}

impl StashNote {
    /// Byte-identical to the legacy `ops/stash.rs` strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            StashNote::NothingToStash => "Nothing to stash: working tree is already clean \
                 (no staged, modified, or untracked files)."
                .to_string(),
            StashNote::UntrackedIncluded { count } => format!(
                "{} untracked file(s) will be included in the stash \
                 (equivalent to `git stash push -u`).",
                count
            ),
            StashNote::UntrackedExcluded { count } => format!(
                "{} untracked file(s) will NOT be included in the stash \
                 (include_untracked=false). They will remain in the working tree.",
                count
            ),
            StashNote::IndexOutOfRange { index, count } => format!(
                "Stash index {} is out of range (only {} stash entr{} exist).",
                index,
                count,
                if *count == 1 { "y" } else { "ies" }
            ),
            StashNote::DirtyBlocksApply { parts, op } => {
                let op_word = match op {
                    StashDirtyOp::Apply => "apply",
                    StashDirtyOp::Pop => "pop",
                };
                format!(
                    "Working tree is dirty ({}) — stash {} is only allowed on a clean \
                     working tree to prevent accidental merge conflicts.",
                    parts.parts_en(),
                    op_word
                )
            }
            StashNote::PopWouldConflict { count, files } => {
                let files_label = if files.is_empty() {
                    "(unknown files)".to_string()
                } else {
                    files.join(", ")
                };
                format!(
                    "Stash pop would produce {} conflict(s): {}. \
                     Pop is blocked to prevent losing the stash entry. \
                     Use 'Stash Apply' instead: it applies the stash without removing it, \
                     allowing you to resolve conflicts safely.",
                    count, files_label
                )
            }
            StashNote::RemoteDropIrreversible => {
                "This permanently removes the stash entry on the remote host. \
                 It cannot be undone from Kagi."
                    .to_string()
            }
        }
    }
}

/// Plan titles for the stash op family (appendix §C stash-* rows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StashTitle {
    /// `plan_stash_push` — `next_count` is the stash count AFTER the push.
    Push { next_count: usize },
    /// `plan_stash_apply`.
    Apply { index: usize },
    /// `plan_stash_pop`.
    Pop { index: usize },
    /// `plan_stash_drop` (local).
    Drop { index: usize },
    /// `plan_stash_drop_remote` (SSH).
    DropRemote { label: String },
}

impl StashTitle {
    /// Byte-identical to the legacy `ops/stash.rs` title strings. NOTE the
    /// `{{{}}}` escaping: the runtime string is `stash@{0}` (a literal
    /// brace pair around the index), not a nested format placeholder.
    pub fn message_en(&self) -> String {
        match self {
            StashTitle::Push { next_count } => {
                format!("Stash push — save local modifications ({})", next_count)
            }
            StashTitle::Apply { index } => format!("Stash apply — restore stash@{{{}}}", index),
            StashTitle::Pop { index } => {
                format!("Stash pop — apply and remove stash@{{{}}}", index)
            }
            StashTitle::Drop { index } => format!("Stash drop — delete stash@{{{}}}", index),
            StashTitle::DropRemote { label } => format!("Drop {}", label),
        }
    }
}

/// Recovery kinds for the stash op family (appendix §D stash-* rows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StashRecovery {
    /// `plan_stash_push`. `message` is the stash message that will be used
    /// (or the literal `"(no message)"`).
    Push { message: String },
    /// `plan_stash_apply`.
    Apply { index: usize, message: String },
    /// `plan_stash_pop`.
    Pop { index: usize, message: String },
    /// `plan_stash_drop` (local). `oid` is `None` when the index resolved to
    /// no stash entry — the recovery text then falls back to the short form
    /// with no commands.
    Drop {
        message: String,
        oid: Option<String>,
    },
    /// `plan_stash_drop_remote` (SSH).
    DropRemote,
}

impl StashRecovery {
    /// Byte-identical to the legacy `ops/stash.rs` recovery strings.
    pub fn message_en(&self) -> String {
        match self {
            // NOTE: the legacy producer always renders the literal
            // "stash@{0}" here (the `{{0}}` escape is NOT filled with the
            // actual push index) — preserved verbatim.
            StashRecovery::Push { message } => format!(
                "To inspect stash entries:  git stash list\n\
                 To restore without removing the stash entry:  git stash apply stash@{{0}}\n\
                 Stash message that will be used: \"{}\"",
                message
            ),
            StashRecovery::Apply { index, message } => format!(
                "The stash entry stash@{{{}}} is NOT removed by apply — it remains in the list.\n\
                 If the apply caused conflicts, resolve them manually; the stash is safely preserved.\n\
                 To see remaining stash entries:  git stash list\n\
                 Stash message: \"{}\"",
                index, message
            ),
            StashRecovery::Pop { index, message } => format!(
                "WARNING: pop = apply + drop.  If apply succeeds, stash@{{{}}} is permanently removed.\n\
                 The stash entry \"{}\" will be consumed.\n\
                 To restore without removing the stash: use 'Stash Apply' instead.\n\
                 To see remaining stash entries:  git stash list",
                index, message
            ),
            StashRecovery::Drop { message, oid } => match oid {
                Some(oid) => format!(
                    "Drop removes the stash entry only — the working tree is NOT touched.\n\
                     The dropped stash commit {} stays reachable from the stash reflog until \
                     gc; restore it with:\n  git stash store -m \"{}\" {}\n\
                     To see remaining stash entries:  git stash list",
                    oid, message, oid
                ),
                None => {
                    "Drop removes the stash entry only — the working tree is NOT touched."
                        .to_string()
                }
            },
            StashRecovery::DropRemote => "A dropped stash commit may remain reachable from the remote's \
                 stash reflog until gc, but Kagi does not manage remote recovery."
                .to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // message_en golden tests (ADR-0129 §3) — byte-exact vs the legacy
    // ops/stash.rs strings (appendix §B-7 / §C / §D), including dynamic
    // values, quotes, and the `stash@{N}` brace escaping.

    #[test]
    fn nothing_to_stash() {
        assert_eq!(
            StashNote::NothingToStash.message_en(),
            "Nothing to stash: working tree is already clean (no staged, modified, or untracked files)."
        );
    }

    #[test]
    fn untracked_included() {
        assert_eq!(
            StashNote::UntrackedIncluded { count: 2 }.message_en(),
            "2 untracked file(s) will be included in the stash (equivalent to `git stash push -u`)."
        );
    }

    #[test]
    fn untracked_excluded() {
        assert_eq!(
            StashNote::UntrackedExcluded { count: 5 }.message_en(),
            "5 untracked file(s) will NOT be included in the stash (include_untracked=false). \
             They will remain in the working tree."
        );
    }

    #[test]
    fn index_out_of_range_singular_and_plural() {
        assert_eq!(
            StashNote::IndexOutOfRange { index: 0, count: 0 }.message_en(),
            "Stash index 0 is out of range (only 0 stash entries exist)."
        );
        assert_eq!(
            StashNote::IndexOutOfRange { index: 3, count: 1 }.message_en(),
            "Stash index 3 is out of range (only 1 stash entry exist)."
        );
        assert_eq!(
            StashNote::IndexOutOfRange { index: 5, count: 2 }.message_en(),
            "Stash index 5 is out of range (only 2 stash entries exist)."
        );
    }

    #[test]
    fn dirty_blocks_apply_both_ops_and_parts() {
        assert_eq!(
            StashNote::DirtyBlocksApply {
                parts: DirtyParts {
                    staged: 2,
                    modified: 1
                },
                op: StashDirtyOp::Apply
            }
            .message_en(),
            "Working tree is dirty (2 staged, 1 modified) — stash apply is only allowed on a \
             clean working tree to prevent accidental merge conflicts."
        );
        assert_eq!(
            StashNote::DirtyBlocksApply {
                parts: DirtyParts {
                    staged: 0,
                    modified: 3
                },
                op: StashDirtyOp::Pop
            }
            .message_en(),
            "Working tree is dirty (3 modified) — stash pop is only allowed on a clean working \
             tree to prevent accidental merge conflicts."
        );
    }

    #[test]
    fn pop_would_conflict_with_files() {
        assert_eq!(
            StashNote::PopWouldConflict {
                count: 2,
                files: vec!["src/a b.rs".to_string(), "src/c.rs".to_string()]
            }
            .message_en(),
            "Stash pop would produce 2 conflict(s): src/a b.rs, src/c.rs. Pop is blocked to \
             prevent losing the stash entry. Use 'Stash Apply' instead: it applies the stash \
             without removing it, allowing you to resolve conflicts safely."
        );
    }

    #[test]
    fn pop_would_conflict_unknown_files() {
        assert_eq!(
            StashNote::PopWouldConflict {
                count: 1,
                files: Vec::new()
            }
            .message_en(),
            "Stash pop would produce 1 conflict(s): (unknown files). Pop is blocked to prevent \
             losing the stash entry. Use 'Stash Apply' instead: it applies the stash without \
             removing it, allowing you to resolve conflicts safely."
        );
    }

    #[test]
    fn remote_drop_irreversible() {
        assert_eq!(
            StashNote::RemoteDropIrreversible.message_en(),
            "This permanently removes the stash entry on the remote host. \
             It cannot be undone from Kagi."
        );
    }

    // ── StashTitle golden tests ──────────────────────────────────────

    #[test]
    fn title_push() {
        assert_eq!(
            StashTitle::Push { next_count: 3 }.message_en(),
            "Stash push — save local modifications (3)"
        );
    }

    #[test]
    fn title_apply_brace_escaping() {
        assert_eq!(
            StashTitle::Apply { index: 0 }.message_en(),
            "Stash apply — restore stash@{0}"
        );
        assert_eq!(
            StashTitle::Apply { index: 12 }.message_en(),
            "Stash apply — restore stash@{12}"
        );
    }

    #[test]
    fn title_pop_brace_escaping() {
        assert_eq!(
            StashTitle::Pop { index: 1 }.message_en(),
            "Stash pop — apply and remove stash@{1}"
        );
    }

    #[test]
    fn title_drop_brace_escaping() {
        assert_eq!(
            StashTitle::Drop { index: 2 }.message_en(),
            "Stash drop — delete stash@{2}"
        );
    }

    #[test]
    fn title_drop_remote() {
        assert_eq!(
            StashTitle::DropRemote {
                label: "stash@{0}: WIP on main: x".to_string()
            }
            .message_en(),
            "Drop stash@{0}: WIP on main: x"
        );
    }

    // ── StashRecovery golden tests ───────────────────────────────────

    #[test]
    fn recovery_push_literal_stash_at_zero() {
        // NOTE: `stash@{0}` is ALWAYS literal here, even for a later index —
        // this mirrors the legacy producer's `{{0}}` escape bug/behavior.
        assert_eq!(
            StashRecovery::Push {
                message: "WIP on feature".to_string()
            }
            .message_en(),
            "To inspect stash entries:  git stash list\n\
             To restore without removing the stash entry:  git stash apply stash@{0}\n\
             Stash message that will be used: \"WIP on feature\""
        );
        assert_eq!(
            StashRecovery::Push {
                message: "(no message)".to_string()
            }
            .message_en(),
            "To inspect stash entries:  git stash list\n\
             To restore without removing the stash entry:  git stash apply stash@{0}\n\
             Stash message that will be used: \"(no message)\""
        );
    }

    #[test]
    fn recovery_apply() {
        assert_eq!(
            StashRecovery::Apply {
                index: 2,
                message: "WIP on main: abc123 fix bug".to_string()
            }
            .message_en(),
            "The stash entry stash@{2} is NOT removed by apply — it remains in the list.\n\
             If the apply caused conflicts, resolve them manually; the stash is safely preserved.\n\
             To see remaining stash entries:  git stash list\n\
             Stash message: \"WIP on main: abc123 fix bug\""
        );
    }

    #[test]
    fn recovery_pop() {
        assert_eq!(
            StashRecovery::Pop {
                index: 0,
                message: "WIP on main: abc123 fix bug".to_string()
            }
            .message_en(),
            "WARNING: pop = apply + drop.  If apply succeeds, stash@{0} is permanently removed.\n\
             The stash entry \"WIP on main: abc123 fix bug\" will be consumed.\n\
             To restore without removing the stash: use 'Stash Apply' instead.\n\
             To see remaining stash entries:  git stash list"
        );
    }

    #[test]
    fn recovery_drop_with_oid() {
        assert_eq!(
            StashRecovery::Drop {
                message: "WIP on main: abc123".to_string(),
                oid: Some("deadbeef1234567890deadbeef1234567890dead".to_string())
            }
            .message_en(),
            "Drop removes the stash entry only — the working tree is NOT touched.\n\
             The dropped stash commit deadbeef1234567890deadbeef1234567890dead stays reachable \
             from the stash reflog until gc; restore it with:\n  \
             git stash store -m \"WIP on main: abc123\" deadbeef1234567890deadbeef1234567890dead\n\
             To see remaining stash entries:  git stash list"
        );
    }

    #[test]
    fn recovery_drop_without_oid() {
        assert_eq!(
            StashRecovery::Drop {
                message: "stash@{0}".to_string(),
                oid: None
            }
            .message_en(),
            "Drop removes the stash entry only — the working tree is NOT touched."
        );
    }

    #[test]
    fn recovery_drop_remote() {
        assert_eq!(
            StashRecovery::DropRemote.message_en(),
            "A dropped stash commit may remain reachable from the remote's stash reflog until \
             gc, but Kagi does not manage remote recovery."
        );
    }
}
