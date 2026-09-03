//! JA strings for `SnapshotNote`/`SnapshotTitle`/`SnapshotRecovery`
//! (working-tree savepoints — restore, ADR-0154).

use kagi_domain::plan_note::{SnapshotNote, SnapshotRecovery, SnapshotTitle};

/// Japanese rendering of one snapshot note.
pub fn note_ja(note: &SnapshotNote) -> String {
    match note {
        SnapshotNote::SnapshotMissing { id } => {
            format!("スナップショット '{}' はこのリポジトリに存在しません。", id)
        }
        SnapshotNote::SavepointFirst => {
            "先に現在の作業ツリーのスナップショットを取得するため、この復元自体も後から取り消せます。".to_string()
        }
        SnapshotNote::RewritesWorkingTree => {
            "作業ツリーをスナップショットの状態に合わせます: tracked ファイルは上書きされ、記録済みのファイルは再作成されます。直前に現在の状態を savepoint として保存するため復元可能です。".to_string()
        }
    }
}

/// Japanese rendering of one snapshot title.
pub fn title_ja(title: &SnapshotTitle) -> String {
    match title {
        SnapshotTitle::Restore { id } => format!("スナップショット {} を復元", id),
    }
}

/// Japanese rendering of one snapshot recovery block.
pub fn recovery_ja(recovery: &SnapshotRecovery) -> String {
    match recovery {
        SnapshotRecovery::Restore => {
            "元に戻すには、この操作の直前に取得した savepoint(実行後の最新スナップショット)を復元してください。`reset --hard` や `git clean` は使用しません。".to_string()
        }
    }
}
