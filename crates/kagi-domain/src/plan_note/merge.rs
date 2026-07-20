//! MergeNote / MergeTitle / MergeRecovery — ADR-0129 appendix §B-6
//! (+ §C title row / §D recovery row).

/// Plan notes for the merge op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeNote {
    /// blocker — `target` is already the current branch (merge into self).
    TargetIsCurrent { target: String },
    /// blocker (no-op family) — `target` is already HEAD.
    TargetIsHead { target: String },
    /// blocker (no-op family) — the current branch already contains `target`.
    AlreadyContains { current: String, target: String },
    /// warning (W31) — a predicted merge conflict. NOT a blocker: the user
    /// still confirms and resolves in Conflict Mode.
    WillConflict { count: usize, files: Vec<String> },
    /// blocker (no-op family) — merging `target` would produce no changes.
    NoChanges { target: String },
}

impl MergeNote {
    /// Byte-identical to the legacy `ops/merge.rs` strings (golden-tested).
    pub fn message_en(&self) -> String {
        match self {
            MergeNote::TargetIsCurrent { target } => {
                format!("Branch '{}' is already the current branch.", target)
            }
            MergeNote::TargetIsHead { target } => {
                format!("{} is already HEAD. Nothing to merge.", target)
            }
            MergeNote::AlreadyContains { current, target } => format!(
                "Current branch '{}' already contains '{}'. Nothing to merge.",
                current, target
            ),
            MergeNote::WillConflict { count, files } => {
                let files_label = if files.is_empty() {
                    "(unknown files)".to_string()
                } else {
                    files.join(", ")
                };
                format!(
                    "Merge will produce {} conflict(s): {}. You will resolve them in Conflict Mode.",
                    count, files_label
                )
            }
            MergeNote::NoChanges { target } => {
                format!("Merging '{}' would produce no changes.", target)
            }
        }
    }
}

/// Plan titles for the merge op family (appendix §C `merge` row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeTitle {
    /// `Merge {target} into {current}` / `Merge {target} into current branch`
    /// (`current: None` when HEAD had no branch name at plan time).
    Into {
        target: String,
        current: Option<String>,
    },
}

impl MergeTitle {
    /// Byte-identical to the legacy `ops/merge.rs` title strings.
    pub fn message_en(&self) -> String {
        match self {
            MergeTitle::Into {
                target,
                current: Some(current),
            } => format!("Merge {} into {}", target, current),
            MergeTitle::Into {
                target,
                current: None,
            } => format!("Merge {} into current branch", target),
        }
    }
}

/// Recovery kinds for the merge op family (appendix §D `merge` row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeRecovery {
    /// The sole merge recovery template: reflog + `git revert -m 1`.
    AfterMerge,
}

impl MergeRecovery {
    /// Byte-identical to the legacy `ops/merge.rs` recovery string.
    pub fn message_en(&self) -> String {
        match self {
            MergeRecovery::AfterMerge => {
                "If this merge is not wanted after execution, use git reflog to find the \
                 previous HEAD.\nFast-forward merges can be undone by moving the branch back; \
                 merge commits can be reverted with git revert -m 1 <merge-commit>."
                    .to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // message_en golden tests (ADR-0129 §3) — byte-exact vs the legacy
    // ops/merge.rs strings (appendix §B-6 / §C / §D).

    #[test]
    fn target_is_current() {
        assert_eq!(
            MergeNote::TargetIsCurrent {
                target: "feature/x".into()
            }
            .message_en(),
            "Branch 'feature/x' is already the current branch."
        );
    }

    #[test]
    fn target_is_head() {
        assert_eq!(
            MergeNote::TargetIsHead {
                target: "origin/main".into()
            }
            .message_en(),
            "origin/main is already HEAD. Nothing to merge."
        );
    }

    #[test]
    fn already_contains() {
        assert_eq!(
            MergeNote::AlreadyContains {
                current: "main".into(),
                target: "feature/old".into()
            }
            .message_en(),
            "Current branch 'main' already contains 'feature/old'. Nothing to merge."
        );
    }

    #[test]
    fn will_conflict_with_files() {
        assert_eq!(
            MergeNote::WillConflict {
                count: 2,
                files: vec!["src/a b.rs".to_string(), "src/c.rs".to_string()],
            }
            .message_en(),
            "Merge will produce 2 conflict(s): src/a b.rs, src/c.rs. You will resolve them in Conflict Mode."
        );
    }

    #[test]
    fn will_conflict_unknown_files() {
        assert_eq!(
            MergeNote::WillConflict {
                count: 3,
                files: Vec::new(),
            }
            .message_en(),
            "Merge will produce 3 conflict(s): (unknown files). You will resolve them in Conflict Mode."
        );
    }

    #[test]
    fn no_changes() {
        assert_eq!(
            MergeNote::NoChanges {
                target: "feature/y".into()
            }
            .message_en(),
            "Merging 'feature/y' would produce no changes."
        );
    }

    #[test]
    fn title_into_named_branch() {
        assert_eq!(
            MergeTitle::Into {
                target: "feature/x".into(),
                current: Some("main".into()),
            }
            .message_en(),
            "Merge feature/x into main"
        );
    }

    #[test]
    fn title_into_current_branch_unnamed() {
        assert_eq!(
            MergeTitle::Into {
                target: "feature/x".into(),
                current: None,
            }
            .message_en(),
            "Merge feature/x into current branch"
        );
    }

    #[test]
    fn recovery_after_merge() {
        assert_eq!(
            MergeRecovery::AfterMerge.message_en(),
            "If this merge is not wanted after execution, use git reflog to find the previous HEAD.\nFast-forward merges can be undone by moving the branch back; merge commits can be reverted with git revert -m 1 <merge-commit>."
        );
    }
}
