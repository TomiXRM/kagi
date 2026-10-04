//! Typed plan recovery on the oplog wire. The pure domain keeps no serde dependency.
//! Family records mirror the fields used by the EN/JA recovery renderers. A
//! malformed or future payload is absent, never a reason to discard its row.

use kagi_domain::plan_note::*;
use serde::{ser::Error as _, Deserialize, Deserializer, Serialize, Serializer};

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

#[derive(Serialize)]
#[serde(remote = "RecoveryKind", tag = "kind", content = "payload")]
enum KindRecord {
    Discard,
    Branch(#[serde(with = "BranchRecord")] BranchRecovery),
    Stash(#[serde(with = "StashRecord")] StashRecovery),
    History(#[serde(with = "HistoryRecord")] HistoryRecovery),
    Pull(#[serde(with = "PullRecord")] PullRecovery),
    Push(#[serde(with = "PushRecord")] PushRecovery),
    Switch(#[serde(with = "SwitchRecord")] SwitchRecovery),
    Checkout(#[serde(with = "CheckoutRecord")] CheckoutRecovery),
    Merge(#[serde(with = "MergeRecord")] MergeRecovery),
    Worktree(#[serde(with = "WorktreeRecord")] WorktreeRecovery),
    CherryRevert(#[serde(with = "CherryRevertRecord")] CherryRevertRecovery),
    Cleanup(#[serde(with = "CleanupRecord")] CleanupRecovery),
    Conflicts(#[serde(with = "ConflictsRecord")] ConflictsRecovery),
    Commit(#[serde(with = "CommitRecord")] CommitRecovery),
    Tag(#[serde(with = "TagRecord")] TagRecovery),
    RemoteBranch(#[serde(with = "RemoteBranchRecord")] RemoteBranchRecovery),
    Reset(#[serde(with = "ResetRecord")] ResetRecovery),
    ForceLease(#[serde(with = "ForceLeaseRecord")] ForceLeaseRecovery),
    Github(#[serde(with = "GithubRecord")] GithubRecovery),
    Rebase(#[serde(with = "RebaseRecord")] RebaseRecovery),
    Snapshot(#[serde(with = "SnapshotRecord")] SnapshotRecovery),
    Sync(#[serde(with = "SyncRecord")] SyncRecovery),
    Maintenance(#[serde(with = "MaintenanceRecord")] MaintenanceRecovery),
    OplogRestore(#[serde(with = "OplogRestoreRecord")] OplogRestoreRecovery),
}

#[derive(Serialize)]
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
    Ok(decode(value))
}

fn decode(value: serde_json::Value) -> Option<PlanRecovery> {
    let serde_json::Value::Object(mut object) = value else {
        return None;
    };
    let commands: Vec<String> = serde_json::from_value(object.remove("commands")?).ok()?;
    let serde_json::Value::Object(mut kind) = object.remove("kind")? else {
        return None;
    };
    let tag = kind.remove("kind")?;
    let tag = tag.as_str()?;
    let payload = kind.remove("payload");
    macro_rules! decode_kind {
        ($($name:literal => $record:ident => $variant:ident),* $(,)?) => {
            match tag {
                "Discard" if payload.is_none() => Some(RecoveryKind::Discard),
                $($name => $record::deserialize(payload?).ok().map(RecoveryKind::$variant),)*
                _ => None,
            }
        }
    }
    let kind = decode_kind!(
        "Branch" => BranchRecord => Branch,
        "Stash" => StashRecord => Stash,
        "History" => HistoryRecord => History,
        "Pull" => PullRecord => Pull,
        "Push" => PushRecord => Push,
        "Switch" => SwitchRecord => Switch,
        "Checkout" => CheckoutRecord => Checkout,
        "Merge" => MergeRecord => Merge,
        "Worktree" => WorktreeRecord => Worktree,
        "CherryRevert" => CherryRevertRecord => CherryRevert,
        "Cleanup" => CleanupRecord => Cleanup,
        "Conflicts" => ConflictsRecord => Conflicts,
        "Commit" => CommitRecord => Commit,
        "Tag" => TagRecord => Tag,
        "RemoteBranch" => RemoteBranchRecord => RemoteBranch,
        "Reset" => ResetRecord => Reset,
        "ForceLease" => ForceLeaseRecord => ForceLease,
        "Github" => GithubRecord => Github,
        "Rebase" => RebaseRecord => Rebase,
        "Snapshot" => SnapshotRecord => Snapshot,
        "Sync" => SyncRecord => Sync,
        "Maintenance" => MaintenanceRecord => Maintenance,
        "OplogRestore" => OplogRestoreRecord => OplogRestore,
    )?;
    Some(PlanRecovery { kind, commands })
}
