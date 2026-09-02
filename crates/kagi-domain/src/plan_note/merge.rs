//! MergeNote / MergeTitle / MergeRecovery — ADR-0129 appendix §B-6
//! (+ §C title row / §D recovery row).

/// The in-progress `.git/` operation a merge would collide with (issue #299).
/// A pure-domain twin of `git2::RepositoryState`, mapped in the git layer so
/// `kagi-domain` stays git2-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InProgressOp {
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
    /// Any other non-clean state git2 reports.
    Other,
}

impl InProgressOp {
    /// English label used at the start of the "… is already in progress" line.
    pub fn label_en(self) -> &'static str {
        match self {
            InProgressOp::Merge => "A merge",
            InProgressOp::Rebase => "A rebase",
            InProgressOp::CherryPick => "A cherry-pick",
            InProgressOp::Revert => "A revert",
            InProgressOp::Bisect => "A bisect",
            InProgressOp::Other => "Another operation",
        }
    }

    /// Japanese label used at the start of the "… が進行中です" line.
    pub fn label_ja(self) -> &'static str {
        match self {
            InProgressOp::Merge => "merge",
            InProgressOp::Rebase => "rebase",
            InProgressOp::CherryPick => "cherry-pick",
            InProgressOp::Revert => "revert",
            InProgressOp::Bisect => "bisect",
            InProgressOp::Other => "別の操作",
        }
    }
}

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
    /// still confirms and resolves in Conflict Mode. `files` is capped for
    /// display; `count` is the true total, so `message_en` appends
    /// "and N more" when `files.len() < count` (issue #301).
    WillConflict { count: usize, files: Vec<String> },
    /// blocker (no-op family) — merging `target` would produce no changes.
    NoChanges { target: String },
    /// blocker (#299) — the current branch and `target` share no common
    /// ancestor. Real git refuses this without `--allow-unrelated-histories`;
    /// libgit2's `merge_commits` would silently merge against an empty base.
    UnrelatedHistories { target: String },
    /// blocker (#299) — a merge / rebase / cherry-pick / revert is already in
    /// progress, so merging would stack a second operation on top of it.
    OperationInProgress { op: InProgressOp },
    /// blocker (#300) — untracked working-tree files would be overwritten by
    /// the incoming merge. `files` is capped for display; `count` is the true
    /// total. Real git refuses and lists the files by name.
    UntrackedWouldBeOverwritten { count: usize, files: Vec<String> },
    /// blocker (#299) — `source` and `target` share no common ancestor
    /// (off-branch merge variant of [`MergeNote::UnrelatedHistories`]).
    IntoUnrelatedHistories { target: String, source: String },

    // ── Merging into a branch that is not checked out (ADR-0144) ─────────
    /// blocker — `target` is checked out in another worktree, so moving its
    /// ref here would leave that worktree's index and files describing a
    /// commit its HEAD no longer points at.
    IntoCheckedOutElsewhere { target: String, worktree: String },
    /// blocker (no-op family) — `target` already contains `source`.
    IntoAlreadyContains { target: String, source: String },
    /// blocker — the merge conflicts. Resolving needs the files in the working
    /// tree, which means checking `target` out; there is nothing to resolve
    /// against while it is not the current branch.
    IntoWouldConflict {
        target: String,
        source: String,
        count: usize,
    },
    /// warning — `target` is behind `source` with nothing of its own, so its
    /// ref simply moves; no merge commit is written.
    IntoFastForward { target: String, source: String },
    /// warning — states the thing that makes this operation worth having: the
    /// current branch, the index and the files on disk are all left alone.
    IntoWorkingTreeUntouched { current: String },
    /// warning — the destination is a remote-tracking ref with no local branch
    /// yet, so one is created at the remote's tip before the merge.
    IntoCreatesLocalBranch { local: String, remote_ref: String },
    /// warning — a local branch of that name exists but is not at the remote's
    /// tip, so the merge lands on the local one and the remote is not involved.
    IntoLocalDiffersFromRemote { local: String, remote_ref: String },
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
            MergeNote::IntoCheckedOutElsewhere { target, worktree } => format!(
                "Branch '{}' is checked out in the worktree '{}'. Merging into it from here would move its ref out from under that worktree, leaving its files and index describing a different commit. Merge from that worktree instead.",
                target, worktree
            ),
            MergeNote::IntoAlreadyContains { target, source } => format!(
                "Branch '{}' already contains '{}'. Nothing to merge.",
                target, source
            ),
            MergeNote::IntoWouldConflict {
                target,
                source,
                count,
            } => format!(
                "Merging '{}' into '{}' conflicts in {} file(s). Conflicts are resolved in the working tree, so this one needs '{}' checked out first.",
                source, target, count, target
            ),
            MergeNote::IntoFastForward { target, source } => format!(
                "'{}' has no commits of its own, so it fast-forwards to '{}'. Its ref moves; no merge commit is written.",
                target, source
            ),
            MergeNote::IntoCreatesLocalBranch { local, remote_ref } => format!(
                "There is no local '{}' yet, so one is created at '{}' and the merge lands on it. Nothing is pushed; '{}' on the remote is unchanged.",
                local, remote_ref, remote_ref
            ),
            MergeNote::IntoLocalDiffersFromRemote { local, remote_ref } => format!(
                "Local '{}' is not at '{}'. The merge lands on your local branch; the remote ref is not read or written.",
                local, remote_ref
            ),
            MergeNote::IntoWorkingTreeUntouched { current } => format!(
                "Your working tree is not touched: '{}' stays checked out, and no file on disk changes.",
                current
            ),
            MergeNote::WillConflict { count, files } => {
                let files_label = capped_files_en(*count, files);
                format!(
                    "Merge will produce {} conflict(s): {}. You will resolve them in Conflict Mode.",
                    count, files_label
                )
            }
            MergeNote::NoChanges { target } => {
                format!("Merging '{}' would produce no changes.", target)
            }
            MergeNote::UnrelatedHistories { target } => format!(
                "'{}' and the current branch have no common history. git refuses this without --allow-unrelated-histories; merging unrelated trees is almost always a mistake.",
                target
            ),
            MergeNote::OperationInProgress { op } => format!(
                "{} is already in progress. Finish or abort it before merging.",
                op.label_en()
            ),
            MergeNote::UntrackedWouldBeOverwritten { count, files } => format!(
                "{} untracked file(s) would be overwritten by merge: {}. Move or remove them first.",
                count,
                capped_files_en(*count, files)
            ),
            MergeNote::IntoUnrelatedHistories { target, source } => format!(
                "'{}' and '{}' have no common history. git refuses this without --allow-unrelated-histories; merging unrelated trees is almost always a mistake.",
                source, target
            ),
        }
    }
}

/// Render a capped file list: the shown names joined by `", "`, with
/// "and N more" appended when the true `count` exceeds what is shown. Empty
/// list renders "(unknown files)" (legacy behaviour). Issue #301.
fn capped_files_en(count: usize, files: &[String]) -> String {
    if files.is_empty() {
        return "(unknown files)".to_string();
    }
    let shown = files.join(", ");
    let more = count.saturating_sub(files.len());
    if more > 0 {
        format!("{}, and {} more", shown, more)
    } else {
        shown
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
    /// Merging into a branch that is not checked out: the exact command to put
    /// the branch back, since `git reflog` alone shows HEAD's history and this
    /// operation never moved HEAD.
    AfterMergeIntoBranch {
        target: String,
        previous_sha: String,
    },
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
            MergeRecovery::AfterMergeIntoBranch {
                target,
                previous_sha,
            } => format!(
                "HEAD did not move, so git reflog will not show this. To put '{target}' back:\n  git branch -f {target} {previous_sha}\nThe branch's own reflog (git reflog {target}) also records the move."
            ),
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
    fn will_conflict_capped_and_n_more() {
        // 50 shown, count 1000 → "and 950 more" (issue #301 cap).
        let files: Vec<String> = (0..50).map(|i| format!("f{i}.rs")).collect();
        let msg = MergeNote::WillConflict {
            count: 1000,
            files: files.clone(),
        }
        .message_en();
        assert!(msg.contains("1000 conflict(s)"));
        assert!(msg.ends_with("and 950 more. You will resolve them in Conflict Mode."));
        // Bounded: the whole rendered line stays small even for a 1000-conflict merge.
        assert!(
            msg.len() < 1000,
            "rendered note must be bounded: {}",
            msg.len()
        );
    }

    #[test]
    fn unrelated_histories() {
        assert_eq!(
            MergeNote::UnrelatedHistories {
                target: "other".into()
            }
            .message_en(),
            "'other' and the current branch have no common history. git refuses this without --allow-unrelated-histories; merging unrelated trees is almost always a mistake."
        );
    }

    #[test]
    fn operation_in_progress() {
        assert_eq!(
            MergeNote::OperationInProgress {
                op: InProgressOp::Merge
            }
            .message_en(),
            "A merge is already in progress. Finish or abort it before merging."
        );
    }

    #[test]
    fn untracked_would_be_overwritten() {
        assert_eq!(
            MergeNote::UntrackedWouldBeOverwritten {
                count: 1,
                files: vec!["a.txt".into()],
            }
            .message_en(),
            "1 untracked file(s) would be overwritten by merge: a.txt. Move or remove them first."
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
