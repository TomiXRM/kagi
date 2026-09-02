//! RemoteBranchNote — delete-remote-branch (branch-menu "Advanced / Dangerous"
//! group). A distinct category from `BranchNote` (which covers local
//! create/rename/delete): this op never touches a local ref, HEAD, or the
//! working tree — it only pushes a delete refspec to the remote.

/// Plan notes for the remote-branch op family (delete).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteBranchNote {
    /// blocker (`plan_delete_remote_branch`) — the remote-tracking ref does
    /// not exist locally (nothing to delete, or already deleted/never fetched).
    NotFound { remote: String, branch: String },
    /// warning (`plan_delete_remote_branch`, unconditional) — only the remote
    /// ref is removed; any local branch tracking it is untouched (mirrors
    /// `BranchNote::DeleteKeepsRemote`'s inverse).
    LocalBranchUntouched { local_name: String },
}

impl RemoteBranchNote {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            RemoteBranchNote::NotFound { remote, branch } => format!(
                "Remote-tracking branch '{}/{}' was not found locally. It may already be deleted, or has never been fetched.",
                remote, branch
            ),
            RemoteBranchNote::LocalBranchUntouched { local_name } => format!(
                "This only deletes the branch on the remote. Your local branch '{}' is untouched and will show as having no upstream.",
                local_name
            ),
        }
    }
}

/// Plan titles for the remote-branch op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteBranchTitle {
    /// `plan_delete_remote_branch` — `Delete remote branch '<remote>/<branch>'`.
    DeleteRemoteBranch { remote: String, branch: String },
}

impl RemoteBranchTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            RemoteBranchTitle::DeleteRemoteBranch { remote, branch } => {
                format!("Delete remote branch '{}/{}'", remote, branch)
            }
        }
    }
}

/// Recovery kinds for the remote-branch op family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteBranchRecovery {
    /// `plan_delete_remote_branch` — recreate the ref by pushing the last-known
    /// tip commit back, if still available in the object store.
    DeleteRemoteBranch {
        remote: String,
        branch: String,
        sha: String,
    },
}

impl RemoteBranchRecovery {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            RemoteBranchRecovery::DeleteRemoteBranch {
                remote,
                branch,
                sha,
            } => format!(
                "If commit '{sha}' still exists (locally or on the remote's reflog before GC), you can recreate the branch:\n  git push {remote} {sha}:refs/heads/{branch}\nOtherwise this cannot be undone from kagi."
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_found() {
        assert_eq!(
            RemoteBranchNote::NotFound {
                remote: "origin".into(),
                branch: "feat/x".into()
            }
            .message_en(),
            "Remote-tracking branch 'origin/feat/x' was not found locally. It may already be deleted, or has never been fetched."
        );
    }

    #[test]
    fn local_branch_untouched() {
        assert_eq!(
            RemoteBranchNote::LocalBranchUntouched {
                local_name: "feat/x".into()
            }
            .message_en(),
            "This only deletes the branch on the remote. Your local branch 'feat/x' is untouched and will show as having no upstream."
        );
    }

    #[test]
    fn delete_remote_branch_title() {
        assert_eq!(
            RemoteBranchTitle::DeleteRemoteBranch {
                remote: "origin".into(),
                branch: "feat/x".into()
            }
            .message_en(),
            "Delete remote branch 'origin/feat/x'"
        );
    }

    #[test]
    fn delete_remote_branch_recovery() {
        assert_eq!(
            RemoteBranchRecovery::DeleteRemoteBranch {
                remote: "origin".into(),
                branch: "feat/x".into(),
                sha: "a1b2c3d4".into()
            }
            .message_en(),
            "If commit 'a1b2c3d4' still exists (locally or on the remote's reflog before GC), you can recreate the branch:\n  git push origin a1b2c3d4:refs/heads/feat/x\nOtherwise this cannot be undone from kagi."
        );
    }
}
