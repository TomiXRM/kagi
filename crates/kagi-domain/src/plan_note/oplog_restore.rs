//! OplogRestoreNote — op-revert / restore-to-point (#334 slice 2b, ADR-0214
//! §5): put branches back where an Operation Log entry recorded them, by ref
//! update only, built on recorded moves alone (never the time-window estimate).

fn short(oid: &Option<String>) -> String {
    match oid {
        Some(oid) => oid.get(..7).unwrap_or(oid).to_string(),
        None => "(none)".to_string(),
    }
}

/// Plan notes for op-revert / restore-to-point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OplogRestoreNote {
    /// blocker — the entry is not in the loaded Operation Log.
    EntryNotLoaded { id: u64 },
    /// blocker — the entry has no recorded ref moves (written before they
    /// were recorded, or by a path that does not record them).
    NotRecorded { id: u64, op: String },
    /// blocker — the entry switched or detached HEAD: undoing that is a
    /// checkout, which touches the working tree.
    HeadMoved { id: u64, op: String },
    /// blocker (op-revert) — a later recorded entry moved the same ref.
    LaterEntryMoved {
        refname: String,
        id: u64,
        op: String,
    },
    /// blocker — nothing would move.
    NothingToRestore,
    /// blocker — the ref is no longer where the record left it (moved
    /// outside Kagi, or by an unrecorded path).
    RefMovedSince {
        refname: String,
        expected: Option<String>,
        current: Option<String>,
    },
    /// blocker — a merge/rebase/cherry-pick/revert is in progress.
    OperationInProgress { op: super::InProgressOp },
    /// warning — a branch to move is checked out in a worktree with changes:
    /// they stay (only the ref moves) and mix with the diff to the new tip.
    CheckedOutDirty { branch: String, path: String },
    /// blocker — a branch to delete is checked out in a worktree.
    DeletesCheckedOutBranch { branch: String, path: String },
    /// blocker — the commit a branch would go back to is no longer in the
    /// object store (pruned after its reflog expired).
    TargetGone { refname: String, oid: String },
    /// warning — one reverse action (`from` = now, `to` = restored; `None` =
    /// the ref is absent on that side).
    Moves {
        refname: String,
        from: Option<String>,
        to: Option<String>,
    },
    /// warning — the branch is checked out: only its ref moves, so that
    /// worktree's index and files stay and its diff against HEAD changes.
    MovesCheckedOutBranch { branch: String, path: String },
    /// warning — what this does not bring back.
    RefsOnly,
}

impl OplogRestoreNote {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            OplogRestoreNote::EntryNotLoaded { id } => {
                format!("Operation #{id} is not in the loaded Operation Log.")
            }
            OplogRestoreNote::NotRecorded { id, op } => format!(
                "Operation #{id} ({op}) has no recorded ref moves, so it cannot be undone exactly. Nothing is guessed from the reflog."
            ),
            OplogRestoreNote::HeadMoved { id, op } => format!(
                "Operation #{id} ({op}) switched or detached HEAD. Undoing it is a checkout, which this does not do."
            ),
            OplogRestoreNote::LaterEntryMoved { refname, id, op } => format!(
                "{refname} was moved again by the later operation #{id} ({op}). Revert that one first, or restore to a point."
            ),
            OplogRestoreNote::NothingToRestore => "No branch would move.".to_string(),
            OplogRestoreNote::RefMovedSince {
                refname,
                expected,
                current,
            } => format!(
                "{refname} is at {}, not at {} where the Operation Log left it. It moved outside the recorded operations.",
                short(current),
                short(expected)
            ),
            OplogRestoreNote::OperationInProgress { op } => format!(
                "{} is in progress. Finish or abort it first.",
                op.label_en()
            ),
            OplogRestoreNote::CheckedOutDirty { branch, path } => format!(
                "'{branch}' is checked out in {path} with uncommitted changes. They stay as they are; after the move they show up together with the difference to the restored tip."
            ),
            OplogRestoreNote::DeletesCheckedOutBranch { branch, path } => format!(
                "'{branch}' would be deleted, but it is checked out in {path}."
            ),
            OplogRestoreNote::TargetGone { refname, oid } => format!(
                "{refname} would go back to {}, which is no longer in the repository.",
                short(&Some(oid.clone()))
            ),
            OplogRestoreNote::Moves { refname, from, to } => match (from, to) {
                (Some(_), Some(_)) => format!("{refname}: {} → {}", short(from), short(to)),
                (None, Some(_)) => format!("{refname}: recreate at {}", short(to)),
                (Some(_), None) => format!("{refname}: delete (was {})", short(from)),
                (None, None) => format!("{refname}: unchanged"),
            },
            OplogRestoreNote::MovesCheckedOutBranch { branch, path } => format!(
                "'{branch}' is checked out in {path}: only the branch moves; that worktree's index and files stay as they are."
            ),
            OplogRestoreNote::RefsOnly => "Only branches move. The working tree, the index, untracked files, stashes, tags and remote branches are not restored. Every moved branch keeps its current tip under refs/kagi/backups/.".to_string(),
        }
    }
}

/// Plan titles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OplogRestoreTitle {
    Revert { id: u64, op: String },
    RestoreTo { id: u64, op: String },
}

impl OplogRestoreTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            OplogRestoreTitle::Revert { id, op } => format!("Revert operation #{id} ({op})"),
            OplogRestoreTitle::RestoreTo { id, op } => {
                format!("Restore branches to after operation #{id} ({op})")
            }
        }
    }
}

/// Recovery kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OplogRestoreRecovery {
    Restore,
}

impl OplogRestoreRecovery {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            OplogRestoreRecovery::Restore => "This is itself recorded with its ref moves: revert it from the Operation Log, or put a branch back with git update-ref <ref> <backup-ref> (the backups are listed in the entry).".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_name_each_reverse_action() {
        let a = Some("a".repeat(40));
        let b = Some("b".repeat(40));
        let line = |from: &Option<String>, to: &Option<String>| {
            OplogRestoreNote::Moves {
                refname: "refs/heads/f".into(),
                from: from.clone(),
                to: to.clone(),
            }
            .message_en()
        };
        assert_eq!(line(&a, &b), "refs/heads/f: aaaaaaa → bbbbbbb");
        assert_eq!(line(&None, &b), "refs/heads/f: recreate at bbbbbbb");
        assert_eq!(line(&a, &None), "refs/heads/f: delete (was aaaaaaa)");
    }
}
