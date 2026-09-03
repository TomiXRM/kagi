//! SnapshotNote — working-tree savepoints (`refs/kagi/snapshots/`, ADR-0154).
//!
//! A snapshot is a real commit of the working tree + index written to the ODB
//! under `refs/kagi/snapshots/<id>` (outside `refs/heads`/`refs/remotes`, so it
//! never appears in the branch list and is never a push/fetch target). Creating
//! one only ADDS a ref — it is non-destructive and needs no plan. Only the
//! *restore* rewrites the working tree, so restore is the sole planned op here.

/// Plan notes for the restore-snapshot op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotNote {
    /// blocker (`plan_restore_snapshot`) — the snapshot ref no longer exists.
    SnapshotMissing { id: String },
    /// warning (unconditional) — a savepoint snapshot of the CURRENT working
    /// tree is taken before the restore, so the restore itself is reversible.
    SavepointFirst,
    /// warning (unconditional) — restore makes the working tree match the
    /// snapshot: tracked files are overwritten and recorded files re-created.
    RewritesWorkingTree,
}

impl SnapshotNote {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            SnapshotNote::SnapshotMissing { id } => {
                format!("Snapshot '{}' does not exist in this repository.", id)
            }
            SnapshotNote::SavepointFirst => {
                "A snapshot of your current working tree is taken first, so this restore can \
                 itself be undone by restoring that savepoint."
                    .to_string()
            }
            SnapshotNote::RewritesWorkingTree => {
                "This makes your working tree match the snapshot: tracked files are overwritten \
                 and the files it recorded are re-created. Your current state is saved as a \
                 savepoint first, so this is recoverable."
                    .to_string()
            }
        }
    }
}

/// Plan titles for the restore-snapshot op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotTitle {
    /// `plan_restore_snapshot` — `Restore snapshot <id>`.
    Restore { id: String },
}

impl SnapshotTitle {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            SnapshotTitle::Restore { id } => format!("Restore snapshot {}", id),
        }
    }
}

/// Recovery kinds for the restore-snapshot op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotRecovery {
    /// `plan_restore_snapshot` — the pre-restore savepoint is the way back.
    /// Fieldless: the savepoint's id is only known at execute time, and it is
    /// always the newest snapshot after the op runs.
    Restore,
}

impl SnapshotRecovery {
    /// Sole English renderer.
    pub fn message_en(&self) -> String {
        match self {
            SnapshotRecovery::Restore => {
                "To undo, restore the savepoint taken just before this operation (it is the \
                 newest snapshot afterwards). No `reset --hard` or `git clean` is used."
                    .to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_missing() {
        assert_eq!(
            SnapshotNote::SnapshotMissing { id: "42".into() }.message_en(),
            "Snapshot '42' does not exist in this repository."
        );
    }

    #[test]
    fn savepoint_first_promises_reversibility() {
        assert_eq!(
            SnapshotNote::SavepointFirst.message_en(),
            "A snapshot of your current working tree is taken first, so this restore can \
             itself be undone by restoring that savepoint."
        );
    }

    #[test]
    fn rewrites_working_tree_mentions_savepoint() {
        assert_eq!(
            SnapshotNote::RewritesWorkingTree.message_en(),
            "This makes your working tree match the snapshot: tracked files are overwritten \
             and the files it recorded are re-created. Your current state is saved as a \
             savepoint first, so this is recoverable."
        );
    }

    #[test]
    fn restore_title() {
        assert_eq!(
            SnapshotTitle::Restore { id: "7".into() }.message_en(),
            "Restore snapshot 7"
        );
    }

    #[test]
    fn restore_recovery_forbids_destructive_verbs() {
        let msg = SnapshotRecovery::Restore.message_en();
        assert!(msg.contains("savepoint"));
        assert!(msg.contains("No `reset --hard` or `git clean`"));
    }
}
