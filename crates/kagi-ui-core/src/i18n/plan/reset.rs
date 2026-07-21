//! JA strings for `ResetNote`/`ResetTitle`/`ResetRecovery` (reset-current-to-HEAD).

use kagi_domain::plan_note::{ResetNote, ResetRecovery, ResetTitle};

/// Japanese rendering of one reset note.
pub fn note_ja(note: &ResetNote) -> String {
    match note {
        ResetNote::DetachedHead => {
            "HEAD が detached 状態です。Reset current-to-HEAD にはアタッチされたブランチが必要です。".to_string()
        }
        ResetNote::CommitMissing { sha } => {
            format!("コミット '{}' はこのリポジトリに存在しません。", sha)
        }
        ResetNote::RefOnlySoftReset => {
            "この操作はブランチポインタのみを移動します(`git reset --soft` と同様): 作業ツリーとステージ済みの変更はそのまま残るため、ファイルが失われるのではなく、新しい HEAD に対する大きな差分として表示されます。".to_string()
        }
        ResetNote::AbandonsCommits { branch, count } => format!(
            "{} 個のコミットが '{}' から到達できなくなります(GC されるまでは reflog から復元可能です)。",
            count, branch
        ),
        ResetNote::TargetNotAncestor { branch } => format!(
            "対象のコミットは '{}' の祖先ではありません。この操作は同じ系譜を巻き戻すのではなく、ブランチを無関係な履歴に付け替えます。",
            branch
        ),
    }
}

/// Japanese rendering of one reset title.
pub fn title_ja(title: &ResetTitle) -> String {
    match title {
        ResetTitle::ResetCurrentToHead { branch, to } => {
            format!("'{}' を {} にリセット", branch, to)
        }
    }
}

/// Japanese rendering of one reset recovery block.
pub fn recovery_ja(recovery: &ResetRecovery) -> String {
    match recovery {
        ResetRecovery::ResetCurrentToHead { branch, from } => format!(
            "元に戻すには、ブランチを以前の先端に戻してください:\n  git update-ref refs/heads/{} {}\n(ref のみの変更のため、いずれの場合も作業ツリーとインデックスは変更されません。)",
            branch, from
        ),
    }
}
