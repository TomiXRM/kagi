//! SyncNote — sync-to-remote (#536, ADR-0215): make a local branch, its index
//! and its working tree match the fetched upstream tip while every local
//! commit and change is retained under `refs/kagi/backups/`.

/// Plan notes for the sync-to-remote op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncNote {
    /// blocker — the branch does not exist locally.
    BranchMissing { branch: String },
    /// blocker — no upstream is configured for the branch.
    NoUpstream { branch: String },
    /// blocker — an upstream is configured but its remote-tracking ref is
    /// absent locally (never fetched, or pruned): nothing to sync *to*.
    UpstreamNotFetched { branch: String, upstream: String },
    /// blocker — HEAD is detached.
    DetachedHead,
    /// blocker — a merge/rebase/cherry-pick/revert is in progress.
    OperationInProgress { op: super::InProgressOp },
    /// blocker — the index has unmerged paths.
    ConflictedFiles { count: usize },
    /// blocker — the branch is checked out in another worktree; its working
    /// tree is not under this operation's control.
    CheckedOutElsewhere { branch: String, path: String },
    /// blocker — the branch is already at the upstream tip and (when it is
    /// HEAD) the working tree is clean: nothing to do, no backups to make.
    AlreadyInSync { branch: String, upstream: String },
    /// warning — `count` local commits are not on the upstream; the branch
    /// tip is retained under `refs/kagi/backups/` before it moves.
    AbandonsCommits { branch: String, count: usize },
    /// warning — staged / unstaged / untracked changes exist; they are
    /// retained as a stash-shaped commit (`git stash apply --index` restores
    /// them) before the working tree is replaced.
    PreservesWork {
        staged: usize,
        unstaged: usize,
        untracked: usize,
    },
    /// warning — ignored files are left where they are; a target path that
    /// collides with an ignored file aborts the sync before any ref moves.
    KeepsIgnored,
}

impl SyncNote {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            SyncNote::BranchMissing { branch } => format!("Branch '{}' does not exist.", branch),
            SyncNote::NoUpstream { branch } => format!(
                "'{}' has no upstream. Set one (Push and create upstream, or Set upstream) first.",
                branch
            ),
            SyncNote::UpstreamNotFetched { branch, upstream } => format!(
                "'{}' tracks '{}', but that remote-tracking ref is not present locally. Fetch first.",
                branch, upstream
            ),
            SyncNote::DetachedHead => {
                "HEAD is detached. Sync to remote requires an attached branch.".to_string()
            }
            SyncNote::OperationInProgress { op } => format!(
                "{} is in progress. Finish or abort it before syncing.",
                op.label_en()
            ),
            SyncNote::ConflictedFiles { count } => format!(
                "{} file(s) have unresolved conflicts. Resolve them before syncing.",
                count
            ),
            SyncNote::CheckedOutElsewhere { branch, path } => format!(
                "'{}' is checked out in {}. Sync it from that worktree.",
                branch, path
            ),
            SyncNote::AlreadyInSync { branch, upstream } => format!(
                "'{}' already matches '{}' and has no local changes. Nothing to do.",
                branch, upstream
            ),
            SyncNote::AbandonsCommits { branch, count } => format!(
                "{} commit(s) on '{}' are not on the upstream. The current tip is kept under refs/kagi/backups/ and can be restored with git update-ref.",
                count, branch
            ),
            SyncNote::PreservesWork {
                staged,
                unstaged,
                untracked,
            } => format!(
                "Local changes ({} staged, {} unstaged, {} untracked) are kept as a stash-shaped commit under refs/kagi/backups/ (restore: git stash apply --index <ref>) and then removed from the working tree.",
                staged, unstaged, untracked
            ),
            SyncNote::KeepsIgnored => "Ignored files are left untouched. If a file from the remote would overwrite an ignored file, the sync stops before anything moves.".to_string(),
        }
    }
}

/// Plan titles for the sync-to-remote op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncTitle {
    /// `Sync '<branch>' to '<upstream>' (<to7>)`.
    SyncToRemote {
        branch: String,
        upstream: String,
        to: String,
    },
}

impl SyncTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            SyncTitle::SyncToRemote {
                branch,
                upstream,
                to,
            } => format!("Sync '{}' to '{}' ({})", branch, upstream, to),
        }
    }
}

/// Recovery kinds for the sync-to-remote op. The backup ref names are fixed
/// at plan time (the attempt id is part of the plan) so the card can show the
/// exact commands that will work after execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncRecovery {
    SyncToRemote {
        branch: String,
        /// Pre-sync tip (full OID).
        from: String,
        /// `refs/kagi/backups/<op>/0` — holds `from`.
        tip_backup: String,
        /// `refs/kagi/backups/<op>/1` — stash-shaped commit of index + working
        /// tree, only when there was anything to keep.
        work_backup: Option<String>,
    },
}

impl SyncRecovery {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            SyncRecovery::SyncToRemote {
                branch,
                from,
                tip_backup,
                work_backup,
            } => {
                let mut s = format!(
                    "Restore the pre-sync tip of '{branch}' ({}) with:\n  git update-ref refs/heads/{branch} {tip_backup}",
                    &from[..from.len().min(7)]
                );
                if let Some(work) = work_backup {
                    s.push_str(&format!(
                        "\nRestore the index and working tree (staged/unstaged/untracked) with:\n  git stash apply --index {work}"
                    ));
                }
                s.push_str("\nBoth refs stay until this entry is forgotten.");
                s
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_lists_both_refs_when_work_was_kept() {
        let r = SyncRecovery::SyncToRemote {
            branch: "main".into(),
            from: "abcdef0123456789".into(),
            tip_backup: "refs/kagi/backups/x/0".into(),
            work_backup: Some("refs/kagi/backups/x/1".into()),
        };
        let text = r.message_en();
        assert!(text.contains("git update-ref refs/heads/main refs/kagi/backups/x/0"));
        assert!(text.contains("git stash apply --index refs/kagi/backups/x/1"));
        assert!(text.contains("(abcdef0)"));
        let bare = SyncRecovery::SyncToRemote {
            branch: "main".into(),
            from: "abcdef0123456789".into(),
            tip_backup: "refs/kagi/backups/x/0".into(),
            work_backup: None,
        };
        assert!(!bare.message_en().contains("stash apply"));
    }

    #[test]
    fn title_and_blockers_name_the_branch_and_upstream() {
        assert_eq!(
            SyncTitle::SyncToRemote {
                branch: "feat".into(),
                upstream: "origin/feat".into(),
                to: "1234567".into()
            }
            .message_en(),
            "Sync 'feat' to 'origin/feat' (1234567)"
        );
        assert!(SyncNote::UpstreamNotFetched {
            branch: "feat".into(),
            upstream: "origin/feat".into()
        }
        .message_en()
        .contains("Fetch first"));
    }
}
