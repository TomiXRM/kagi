//! Typed plan recovery on the oplog wire. The pure domain keeps no serde dependency.
//! Family records mirror the fields used by the EN/JA recovery renderers. A
//! malformed or future payload is absent, never a reason to discard its row.

use kagi_domain::plan_note::*;
use serde::{de::Error as _, ser::Error as _, Deserialize, Deserializer, Serialize, Serializer};

macro_rules! family {
    ($(#[$attr:meta])* $name:ident, $domain:literal, { $($variants:tt)* }) => {
        $(#[$attr])*
        #[derive(Serialize, Deserialize)]
        #[serde(remote = $domain)]
        enum $name { $($variants)* }
    };
}

// serde's remote enum must retain the domain's three *Branch variant names.
family!(#[expect(clippy::enum_variant_names, reason = "remote enum mirrors the domain")] BranchRecord, "BranchRecovery", {
    CreateBranch { name: String }, RenameBranch { old: String, new: String },
    DeleteBranch { name: String, tip: Option<String> },
});
family!(StashRecord, "StashRecovery", {
    Push { message: String }, Apply { index: usize, message: String },
    Pop { index: usize, message: String }, Drop { message: String, oid: Option<String> },
    DropRemote,
});
#[derive(Serialize, Deserialize)]
#[serde(remote = "HistoryMoveDir")]
enum HistoryMoveDirRecord {
    Undo,
    Redo,
}
family!(HistoryRecord, "HistoryRecovery", {
    Undo { sha: String, blocked: bool }, Amend { sha: String, blocked: bool },
    HistoryMove {
        #[serde(with = "HistoryMoveDirRecord")]
        label: HistoryMoveDir,
        branch: String, from_short: String, to_short: String,
        kind_slug: String, from_full: String,
    },
});
family!(PullRecord, "PullRecovery", { Pull, PullAutoStash, PullRemote, PullBranchFf { branch: String }, });
family!(PushRecord, "PushRecovery", { Push, PushBlocked, PushBranch, SetUpstream { branch: String }, });
family!(SwitchRecord, "SwitchRecovery", {
    CheckoutTracking { local: String }, SwitchToLatest { remote: String, branch: String },
});
family!(CheckoutRecord, "CheckoutRecovery", {
    Checkout { previous: String }, CheckoutCommit { previous: String },
});
family!(MergeRecord, "MergeRecovery", {
    AfterMerge, AfterMergeIntoBranch { target: String, previous_sha: String },
});
family!(WorktreeRecord, "WorktreeRecovery", {
    CreateBranchCheckout { name: String, prev: String },
    CreateWorktree { path: String, branch: String }, Unlock { name: String },
    RemoveWorktree { path: String, branch: Option<String> },
    LockWorktree { name: String }, Prune, Repair,
});
family!(CherryRevertRecord, "CherryRevertRecovery", { AfterCherryPick, AfterRevert, });
family!(CleanupRecord, "CleanupRecovery", { CleanupDelete { remote_refs: Vec<String> }, });
family!(ConflictsRecord, "ConflictsRecovery", { Continue { op: String }, Abort { op: String }, Skip { op: String }, });
family!(CommitRecord, "CommitRecovery", { AfterCommit { staged_files: Vec<String> }, });
family!(TagRecord, "TagRecovery", { CreateTag { name: String }, PushTag { name: String, remote: String }, });
family!(RemoteBranchRecord, "RemoteBranchRecovery", {
    DeleteRemoteBranch { remote: String, branch: String, sha: String },
});
family!(ResetRecord, "ResetRecovery", { ResetCurrentToHead { branch: String, from: String }, });
family!(ForceLeaseRecord, "ForceLeaseRecovery", {
    ForceLeasePush { branch: String, remote: String, previous_remote_sha: String, new_sha: String },
});
// The GitHub recovery renderer reads only `number`. The execution-specific
// local cleanup promise lives in the receipt, not in recovery guidance.
family!(GithubRecord, "GithubRecovery", {
    MergePr {
        number: u64,
        #[serde(skip)] base_repo: String,
        #[serde(skip)] delete_branch: Option<String>,
        #[serde(skip)] cross_repository: bool,
        #[serde(skip)] local_branch: Option<Box<kagi_domain::plan::PrMergeLocalBranch>>,
    },
    ApplySuggestion,
});
family!(RebaseRecord, "RebaseRecovery", {
    RebaseCurrentOnto { branch: String, from: String }, ReplayOnto { branch: String, from: String },
});
family!(SnapshotRecord, "SnapshotRecovery", { Restore, });
family!(SyncRecord, "SyncRecovery", {
    SyncToRemote { branch: String, from: String, tip_backup: String, work_backup: Option<String> },
});
family!(MaintenanceRecord, "MaintenanceRecovery", { WriteCommitGraph, EnableFsmonitor, });
family!(OplogRestoreRecord, "OplogRestoreRecovery", { Restore, });

// Serde's remote enum derive cannot deserialize an adjacently tagged tuple
// payload here: its generated missing-field path requires Deserialize on the
// dependency-free domain family. Generate the writer and reader tag from one
// declaration instead, so new RecoveryKind variants cannot drift.
macro_rules! kind_record {
    ($($kind:ident => $record:ident ($path:literal): $domain:ty),+ $(,)?) => {
        #[derive(Serialize)]
        #[serde(remote = "RecoveryKind", tag = "kind", content = "payload")]
        enum KindRecord {
            Discard,
            $($kind(#[serde(with = $path)] $domain),)+
        }

        impl KindRecord {
            fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<RecoveryKind, D::Error> {
                #[derive(Deserialize)]
                #[serde(tag = "kind", content = "payload")]
                enum KindTag {
                    Discard,
                    $($kind(serde_json::Value),)+
                }
                match KindTag::deserialize(deserializer)? {
                    KindTag::Discard => Ok(RecoveryKind::Discard),
                    $(KindTag::$kind(payload) => $record::deserialize(payload)
                        .map(RecoveryKind::$kind)
                        .map_err(D::Error::custom),)+
                }
            }
        }
    };
}

kind_record!(
    Branch => BranchRecord ("BranchRecord"): BranchRecovery,
    Stash => StashRecord ("StashRecord"): StashRecovery,
    History => HistoryRecord ("HistoryRecord"): HistoryRecovery,
    Pull => PullRecord ("PullRecord"): PullRecovery,
    Push => PushRecord ("PushRecord"): PushRecovery,
    Switch => SwitchRecord ("SwitchRecord"): SwitchRecovery,
    Checkout => CheckoutRecord ("CheckoutRecord"): CheckoutRecovery,
    Merge => MergeRecord ("MergeRecord"): MergeRecovery,
    Worktree => WorktreeRecord ("WorktreeRecord"): WorktreeRecovery,
    CherryRevert => CherryRevertRecord ("CherryRevertRecord"): CherryRevertRecovery,
    Cleanup => CleanupRecord ("CleanupRecord"): CleanupRecovery,
    Conflicts => ConflictsRecord ("ConflictsRecord"): ConflictsRecovery,
    Commit => CommitRecord ("CommitRecord"): CommitRecovery,
    Tag => TagRecord ("TagRecord"): TagRecovery,
    RemoteBranch => RemoteBranchRecord ("RemoteBranchRecord"): RemoteBranchRecovery,
    Reset => ResetRecord ("ResetRecord"): ResetRecovery,
    ForceLease => ForceLeaseRecord ("ForceLeaseRecord"): ForceLeaseRecovery,
    Github => GithubRecord ("GithubRecord"): GithubRecovery,
    Rebase => RebaseRecord ("RebaseRecord"): RebaseRecovery,
    Snapshot => SnapshotRecord ("SnapshotRecord"): SnapshotRecovery,
    Sync => SyncRecord ("SyncRecord"): SyncRecovery,
    Maintenance => MaintenanceRecord ("MaintenanceRecord"): MaintenanceRecovery,
    OplogRestore => OplogRestoreRecord ("OplogRestoreRecord"): OplogRestoreRecovery,
);

#[derive(Serialize, Deserialize)]
#[serde(remote = "PlanRecovery")]
struct PlanRecord {
    #[serde(with = "KindRecord")]
    kind: RecoveryKind,
    commands: Vec<String>,
}

pub(super) fn serialize<S: Serializer>(
    value: &Option<&PlanRecovery>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    PlanRecord::serialize(
        value.ok_or_else(|| S::Error::custom("missing plan recovery"))?,
        serializer,
    )
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<PlanRecovery>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(PlanRecord::deserialize(value).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oplog::{entry_to_json, parse_oplog_line, OpLogEntry, OpOutcome};
    use kagi_domain::plan::StateSummary;

    #[test]
    fn every_recovery_family_round_trips_its_typed_payload_and_commands() {
        let kinds = [
            RecoveryKind::Discard,
            RecoveryKind::Branch(BranchRecovery::CreateBranch {
                name: "feature".into(),
            }),
            RecoveryKind::Stash(StashRecovery::DropRemote),
            RecoveryKind::History(HistoryRecovery::Undo {
                sha: "abc".into(),
                blocked: false,
            }),
            RecoveryKind::Pull(PullRecovery::Pull),
            RecoveryKind::Push(PushRecovery::Push),
            RecoveryKind::Switch(SwitchRecovery::CheckoutTracking {
                local: "feature".into(),
            }),
            RecoveryKind::Checkout(CheckoutRecovery::Checkout {
                previous: "main".into(),
            }),
            RecoveryKind::Merge(MergeRecovery::AfterMerge),
            RecoveryKind::Worktree(WorktreeRecovery::Prune),
            RecoveryKind::CherryRevert(CherryRevertRecovery::AfterCherryPick),
            RecoveryKind::Cleanup(CleanupRecovery::CleanupDelete {
                remote_refs: vec!["origin/feature".into()],
            }),
            RecoveryKind::Conflicts(ConflictsRecovery::Continue { op: "merge".into() }),
            RecoveryKind::Commit(CommitRecovery::AfterCommit {
                staged_files: vec!["file.txt".into()],
            }),
            RecoveryKind::Tag(TagRecovery::CreateTag { name: "v1".into() }),
            RecoveryKind::RemoteBranch(RemoteBranchRecovery::DeleteRemoteBranch {
                remote: "origin".into(),
                branch: "feature".into(),
                sha: "abc".into(),
            }),
            RecoveryKind::Reset(ResetRecovery::ResetCurrentToHead {
                branch: "main".into(),
                from: "abc".into(),
            }),
            RecoveryKind::ForceLease(ForceLeaseRecovery::ForceLeasePush {
                branch: "main".into(),
                remote: "origin".into(),
                previous_remote_sha: "abc".into(),
                new_sha: "def".into(),
            }),
            RecoveryKind::Github(GithubRecovery::ApplySuggestion),
            RecoveryKind::Rebase(RebaseRecovery::RebaseCurrentOnto {
                branch: "main".into(),
                from: "abc".into(),
            }),
            RecoveryKind::Snapshot(SnapshotRecovery::Restore),
            RecoveryKind::Sync(SyncRecovery::SyncToRemote {
                branch: "main".into(),
                from: "abc".into(),
                tip_backup: "backup".into(),
                work_backup: Some("work".into()),
            }),
            RecoveryKind::Maintenance(MaintenanceRecovery::WriteCommitGraph),
            RecoveryKind::OplogRestore(OplogRestoreRecovery::Restore),
        ];
        let state = StateSummary {
            head: "main".into(),
            dirty: "clean".into(),
        };
        for kind in kinds {
            let recovery = PlanRecovery {
                kind,
                commands: vec!["git status --short".into()],
            };
            let mut entry = OpLogEntry::new(
                "round-trip",
                "/tmp/recovery-test",
                state.clone(),
                OpOutcome::Success {
                    after: state.clone(),
                },
            );
            entry.recovery_plan = Some(recovery.clone());
            let read = parse_oplog_line(&entry_to_json(&entry)).expect("entry remains readable");
            assert_eq!(read.recovery_plan, Some(recovery));
        }
    }
}
