//! JA strings for `ResetNote`/`ResetTitle`/`ResetRecovery` (reset-current-to-HEAD).

use kagi_domain::plan_note::{ResetNote, ResetRecovery, ResetTitle};

/// Japanese rendering of one reset note.
pub fn note_ja(note: &ResetNote) -> String {
    match note {
        ResetNote::DetachedHead => {
            "HEAD が detached です。reset には branch が必要です。".to_string()
        }
        ResetNote::CommitMissing { sha } => {
            format!("commit がこのリポジトリにありません。\ncommit `{}`", sha)
        }
        ResetNote::RefOnlySoftReset => {
            "branch ポインタだけを移動します(`git reset --soft` 相当)。作業ツリーと stage 済みの変更は残り、新しい HEAD への差分として表示されます。".to_string()
        }
        ResetNote::AbandonsCommits { branch, count } => format!(
            "{} 件の commit が到達不能になります(GC まで reflog から復元可能)。\nbranch `{}`",
            count, branch
        ),
        ResetNote::TargetNotAncestor { branch } => format!(
            "対象 commit は祖先ではありません。系譜の巻き戻しではなく、無関係な履歴への付け替えになります。\nbranch `{}`",
            branch
        ),
    }
}

/// Japanese rendering of one reset title.
pub fn title_ja(title: &ResetTitle) -> String {
    match title {
        ResetTitle::ResetCurrentToHead { branch, to } => {
            format!("`{}` を {} に reset", branch, to)
        }
    }
}

/// Japanese rendering of one reset recovery block.
pub fn recovery_ja(recovery: &ResetRecovery) -> String {
    match recovery {
        ResetRecovery::ResetCurrentToHead { branch, from } => format!(
            "元に戻すには branch を以前の先端へ:\n  git update-ref refs/heads/{} {}\nref だけの変更で、作業ツリーと index は変わりません。",
            branch, from
        ),
    }
}
