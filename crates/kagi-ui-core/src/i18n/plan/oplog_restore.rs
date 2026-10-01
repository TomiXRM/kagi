//! JA strings for `OplogRestoreNote`/`Title`/`Recovery` (op-revert /
//! restore-to-point, #334 slice 2b).

use kagi_domain::plan_note::{OplogRestoreNote, OplogRestoreRecovery, OplogRestoreTitle};

fn short(oid: &Option<String>) -> String {
    match oid {
        Some(oid) => oid.get(..7).unwrap_or(oid).to_string(),
        None => "(なし)".to_string(),
    }
}

/// Japanese rendering of one note.
pub fn note_ja(note: &OplogRestoreNote) -> String {
    match note {
        OplogRestoreNote::EntryNotLoaded { id } => {
            format!("操作 #{id} はこの repository の読み込まれた操作にありません(別の repository の操作かもしれません)。")
        }
        OplogRestoreNote::NotRecorded { id, op } => format!(
            "操作 #{id}({op})には ref の移動の記録が無いため、正確に戻せません。reflog からの推定は使いません。"
        ),
        OplogRestoreNote::HeadMoved { id, op } => format!(
            "操作 #{id}({op})は HEAD を切り替えた(または detached にした)ため、戻すには checkout が必要です。この操作では行いません。"
        ),
        OplogRestoreNote::LaterEntryMoved { refname, id, op } => format!(
            "{refname} は後の操作 #{id}({op})でも動いています。先にそちらを取り消すか、時点への復元を使ってください。"
        ),
        OplogRestoreNote::NothingToRestore => "動かす branch がありません。".to_string(),
        OplogRestoreNote::RefMovedSince {
            refname,
            expected,
            current,
        } => format!(
            "{refname} は {} にあり、Operation Log の記録({})と違います。記録された操作の外で動いています。",
            short(current),
            short(expected)
        ),
        OplogRestoreNote::OperationInProgress { op } => {
            format!("{} が進行中です。完了か abort してからにしてください。", op.label_ja())
        }
        OplogRestoreNote::CheckedOutDirty { branch, path } => format!(
            "`{branch}` は {path} で checkout 中で、未コミットの変更があります。変更はそのまま残り、移動後は戻した先端との差分と一緒に表示されます。"
        ),
        OplogRestoreNote::DeletesCheckedOutBranch { branch, path } => {
            format!("`{branch}` を削除することになりますが、{path} で checkout 中です。")
        }
        OplogRestoreNote::TargetGone { refname, oid } => format!(
            "{refname} を戻す先の {} はもう repository にありません。",
            short(&Some(oid.clone()))
        ),
        OplogRestoreNote::Moves { refname, from, to } => match (from, to) {
            (Some(_), Some(_)) => format!("{refname}: {} → {}", short(from), short(to)),
            (None, Some(_)) => format!("{refname}: {} で作り直す", short(to)),
            (Some(_), None) => format!("{refname}: 削除する(現在 {})", short(from)),
            (None, None) => format!("{refname}: 変更なし"),
        },
        OplogRestoreNote::MovesCheckedOutBranch { branch, path } => format!(
            "`{branch}` は {path} で checkout 中です。動くのは branch だけで、その worktree の index とファイルはそのままです。"
        ),
        OplogRestoreNote::RefsOnly => "動くのは branch だけです。作業ツリー・index・untracked ファイル・stash・tag・remote branch は戻りません。動かす branch の現在の先端はすべて refs/kagi/backups/ に保持します。".to_string(),
    }
}

/// Japanese rendering of one title.
pub fn title_ja(title: &OplogRestoreTitle) -> String {
    match title {
        OplogRestoreTitle::Revert { id, op } => format!("操作 #{id}({op})を取り消す"),
        OplogRestoreTitle::RestoreTo { id, op } => {
            format!("branch を操作 #{id}({op})の直後に戻す")
        }
    }
}

/// Japanese rendering of one recovery block.
pub fn recovery_ja(recovery: &OplogRestoreRecovery) -> String {
    match recovery {
        OplogRestoreRecovery::Restore => "この操作も ref の移動つきで記録されます。Operation Log から取り消すか、git update-ref <ref> <backup-ref> で branch を戻せます(backup は entry に記載)。".to_string(),
    }
}
