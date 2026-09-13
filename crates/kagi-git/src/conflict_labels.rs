//! Role-based side terminology for a conflict (T-CONFLICT-010 / ADR-0058).
//!
//! The words "ours" and "theirs" never reach a user-facing string; a side
//! is named by the role it plays in *this* operation plus the real branch
//! or commit it is. Split out of `conflicts.rs` on that feature boundary
//! (#707 review made room there); re-exported from `conflicts` so callers
//! keep their path.

use super::conflicts::ConflictOp;

/// A single role + real-name label pair (ADR-0058 two-line label).
///
/// `role` is the translatable role word (e.g. "Current branch", "New base");
/// `name` is the real branch / commit name shown verbatim (never translated).
/// The words "ours" / "theirs" must never appear in `role`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideLabel {
    /// Role word (translatable via Msg in the UI lane).
    pub role: String,
    /// Real branch / commit name (verbatim, not translated).
    pub name: String,
}

impl SideLabel {
    fn new(role: &str, name: impl Into<String>) -> Self {
        SideLabel {
            role: role.to_string(),
            name: name.into(),
        }
    }
}

/// The current + incoming side labels for an operation, plus the base and result
/// roles (the four roles of §2: Base, current, incoming, Result).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideLabels {
    /// Left side (index stage 2 = libgit2 "ours") translated to a role name.
    pub current: SideLabel,
    /// Right side (index stage 3 = libgit2 "theirs") translated to a role name.
    pub incoming: SideLabel,
    /// Base (common ancestor) role label.
    pub base: SideLabel,
    /// Result (editable resolution) role label.
    pub result: SideLabel,
}

/// Produce the role + real-name labels for an operation (ADR-0058 §2 table).
///
/// `current_branch` is the short name of the branch HEAD is on (used for the
/// merge / cherry-pick / revert "Current branch" / "New base" left label).
///
/// The rebase direction swap (libgit2 reports onto as "ours", the replayed
/// commit as "theirs") is translated here so the UI never has to know: the
/// left/current label becomes **New base** and the right/incoming label becomes
/// **Your commit being replayed**.  The strings "ours"/"theirs" never appear.
pub fn side_labels(op: &ConflictOp, current_branch: &str) -> SideLabels {
    let base = SideLabel::new("Base", "common ancestor");
    let result = SideLabel::new("Result", "your resolution");

    match op {
        ConflictOp::Merge {
            incoming,
            incoming_summary,
        } => SideLabels {
            current: SideLabel::new("Current branch", current_branch),
            incoming: SideLabel::new("Merging in", commit_display(incoming, incoming_summary)),
            base,
            result,
        },
        ConflictOp::Rebase {
            commit,
            commit_summary,
            ..
        } => SideLabels {
            // Direction translation: libgit2 "ours" == the rebase target (onto),
            // surfaced to the user as the New base.
            current: SideLabel::new("New base", current_branch),
            // libgit2 "theirs" == the commit being replayed.
            incoming: SideLabel::new(
                "Your commit being replayed",
                commit_display(commit, commit_summary),
            ),
            base,
            result,
        },
        ConflictOp::CherryPick {
            source,
            source_summary,
        } => SideLabels {
            current: SideLabel::new("Current branch", current_branch),
            incoming: SideLabel::new(
                "Commit being applied",
                commit_display(source, source_summary),
            ),
            base,
            result,
        },
        ConflictOp::Revert {
            source,
            source_summary,
        } => SideLabels {
            current: SideLabel::new("Current branch", current_branch),
            incoming: SideLabel::new(
                "Changes being undone",
                commit_display(source, source_summary),
            ),
            base,
            result,
        },
        // #309: a stash apply/pop has no incoming commit — the "incoming" side is
        // the stashed changes themselves. Never "ours"/"theirs" (ADR-0058).
        ConflictOp::StashConflict => SideLabels {
            current: SideLabel::new("Current branch", current_branch),
            incoming: SideLabel::new("Stashed changes", "your stash"),
            base,
            result,
        },
    }
}

/// Real-name display for a commit: `"<sha> <summary>"`, `"<sha>"`, or
/// `"(unknown commit)"` — built with `chars()`-safe concatenation only.
fn commit_display(sha: &Option<String>, summary: &Option<String>) -> String {
    match (sha, summary) {
        (Some(s), Some(sum)) => format!("{} {}", s, sum),
        (Some(s), None) => s.clone(),
        (None, Some(sum)) => sum.clone(),
        (None, None) => "(unknown commit)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_display_variants() {
        assert_eq!(
            commit_display(&Some("abc".to_string()), &Some("msg".to_string())),
            "abc msg"
        );
        assert_eq!(commit_display(&Some("abc".to_string()), &None), "abc");
        assert_eq!(commit_display(&None, &Some("msg".to_string())), "msg");
        assert_eq!(commit_display(&None, &None), "(unknown commit)");
    }
}
