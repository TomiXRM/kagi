//! CommitNote / CommitTitle / CommitRecovery — ADR-0129 Phase 2.
//!
//! Source of truth is `crates/kagi-git/src/staging.rs::plan_commit` (a
//! producer that lives outside `ops/` — see the appendix's "staging.rs(ops
//! 外)" row, discovered mid-Phase-1). `backend.rs::plan_merge_commit` reuses
//! `plan_commit` and overrides only the title with the
//! [`CommitTitle::FinalizeMergeCommit`] variant added here.

/// The `"{n} modified" / "{n} untracked"` fragment of the leftover-changes
/// warning. Only the non-zero parts are rendered, joined by `", "`, exactly
/// like the legacy `staging.rs::plan_commit` local `parts` builder. This is
/// deliberately distinct from `CommonNote`'s `DirtyParts` (staged/modified):
/// `plan_commit`'s leftover warning covers what will NOT be committed
/// (unstaged + untracked), never the staged count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitLeftoverParts {
    pub modified: usize,
    pub untracked: usize,
}

impl CommitLeftoverParts {
    /// `"2 modified, 1 untracked"` / `"2 modified"` / `"1 untracked"`.
    pub fn parts_en(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.modified > 0 {
            parts.push(format!("{} modified", self.modified));
        }
        if self.untracked > 0 {
            parts.push(format!("{} untracked", self.untracked));
        }
        parts.join(", ")
    }
}

/// Plan notes for the commit op family (`staging.rs::plan_commit`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitNote {
    /// blocker — the commit message is empty after trimming.
    EmptyMessage,
    /// blocker — nothing is staged in the index.
    NothingStaged,
    /// blocker — the repository has conflicted files.
    ConflictedFiles { count: usize },
    /// warning — unstaged/untracked changes will not be part of this commit.
    LeftoverNotIncluded {
        count: usize,
        parts: CommitLeftoverParts,
    },
}

impl CommitNote {
    /// Sole English renderer (byte-identical to the legacy `staging.rs`
    /// strings — golden-tested below).
    pub fn message_en(&self) -> String {
        match self {
            CommitNote::EmptyMessage => "Commit message must not be empty.".to_string(),
            CommitNote::NothingStaged => "Nothing to commit: no files are staged. Use \
                 stage_file() to stage changes before committing."
                .to_string(),
            CommitNote::ConflictedFiles { count } => format!(
                "Repository has {} conflicted file(s). Resolve all conflicts before committing.",
                count
            ),
            CommitNote::LeftoverNotIncluded { count, parts } => format!(
                "{} file(s) ({}) will NOT be included in this commit.",
                count,
                parts.parts_en()
            ),
        }
    }
}

/// Plan titles for the commit op family (`staging.rs::plan_commit` +
/// `backend.rs::plan_merge_commit`'s override).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitTitle {
    /// `Commit: "{summary}"` — `summary` is the message, trimmed and
    /// truncated to 72 chars (matching the legacy `msg_summary` builder).
    Commit { summary: String },
    /// `backend.rs::plan_merge_commit`'s hardcoded title override.
    FinalizeMergeCommit,
}

impl CommitTitle {
    /// Sole English renderer (byte-identical to the legacy strings).
    pub fn message_en(&self) -> String {
        match self {
            CommitTitle::Commit { summary } => format!("Commit: \"{}\"", summary),
            CommitTitle::FinalizeMergeCommit => "Finalize merge commit".to_string(),
        }
    }
}

/// Recovery kinds for the commit op family (`staging.rs::plan_commit`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitRecovery {
    /// The sole commit recovery template: amend / revert HEAD, listing the
    /// staged file paths that were just committed.
    AfterCommit { staged_files: Vec<String> },
}

impl CommitRecovery {
    /// Sole English renderer (byte-identical to the legacy strings).
    pub fn message_en(&self) -> String {
        match self {
            CommitRecovery::AfterCommit { staged_files } => format!(
                "To amend the commit message immediately after:\n  git commit --amend\n\
                 To undo the commit while keeping changes staged:\n  git revert HEAD\n\
                 (Staged files: {})",
                staged_files.join(", ")
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // message_en golden tests (ADR-0129 §3): byte-exact vs the legacy
    // staging.rs::plan_commit / backend.rs::plan_merge_commit strings.

    #[test]
    fn empty_message() {
        assert_eq!(
            CommitNote::EmptyMessage.message_en(),
            "Commit message must not be empty."
        );
    }

    #[test]
    fn nothing_staged() {
        assert_eq!(
            CommitNote::NothingStaged.message_en(),
            "Nothing to commit: no files are staged. Use stage_file() to stage changes before committing."
        );
    }

    #[test]
    fn conflicted_files_singular_and_plural() {
        assert_eq!(
            CommitNote::ConflictedFiles { count: 1 }.message_en(),
            "Repository has 1 conflicted file(s). Resolve all conflicts before committing."
        );
        assert_eq!(
            CommitNote::ConflictedFiles { count: 3 }.message_en(),
            "Repository has 3 conflicted file(s). Resolve all conflicts before committing."
        );
    }

    #[test]
    fn leftover_not_included_both_parts() {
        assert_eq!(
            CommitNote::LeftoverNotIncluded {
                count: 3,
                parts: CommitLeftoverParts {
                    modified: 2,
                    untracked: 1
                },
            }
            .message_en(),
            "3 file(s) (2 modified, 1 untracked) will NOT be included in this commit."
        );
    }

    #[test]
    fn leftover_not_included_modified_only() {
        assert_eq!(
            CommitNote::LeftoverNotIncluded {
                count: 2,
                parts: CommitLeftoverParts {
                    modified: 2,
                    untracked: 0
                },
            }
            .message_en(),
            "2 file(s) (2 modified) will NOT be included in this commit."
        );
    }

    #[test]
    fn leftover_not_included_untracked_only() {
        assert_eq!(
            CommitNote::LeftoverNotIncluded {
                count: 1,
                parts: CommitLeftoverParts {
                    modified: 0,
                    untracked: 1
                },
            }
            .message_en(),
            "1 file(s) (1 untracked) will NOT be included in this commit."
        );
    }

    #[test]
    fn title_commit_with_quotes_and_path_like_summary() {
        assert_eq!(
            CommitTitle::Commit {
                summary: "fix: handle \"quoted\" path/to/file.rs".into()
            }
            .message_en(),
            "Commit: \"fix: handle \"quoted\" path/to/file.rs\""
        );
    }

    #[test]
    fn title_commit_empty_summary() {
        assert_eq!(
            CommitTitle::Commit {
                summary: String::new()
            }
            .message_en(),
            "Commit: \"\""
        );
    }

    #[test]
    fn title_finalize_merge_commit() {
        assert_eq!(
            CommitTitle::FinalizeMergeCommit.message_en(),
            "Finalize merge commit"
        );
    }

    #[test]
    fn recovery_after_commit_with_staged_files() {
        assert_eq!(
            CommitRecovery::AfterCommit {
                staged_files: vec!["src/a b.rs".to_string(), "README.md".to_string()],
            }
            .message_en(),
            "To amend the commit message immediately after:\n  git commit --amend\nTo undo the commit while keeping changes staged:\n  git revert HEAD\n(Staged files: src/a b.rs, README.md)"
        );
    }

    #[test]
    fn recovery_after_commit_no_staged_files() {
        assert_eq!(
            CommitRecovery::AfterCommit {
                staged_files: Vec::new(),
            }
            .message_en(),
            "To amend the commit message immediately after:\n  git commit --amend\nTo undo the commit while keeping changes staged:\n  git revert HEAD\n(Staged files: )"
        );
    }
}
