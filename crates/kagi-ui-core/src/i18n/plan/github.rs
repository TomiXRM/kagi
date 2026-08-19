//! JA strings for `GithubNote`/`GithubTitle`/`GithubRecovery` (PR merge).

use kagi_domain::plan_note::{GithubNote, GithubRecovery, GithubTitle};

/// Japanese rendering of one GitHub note.
pub fn note_ja(note: &GithubNote) -> String {
    match note {
        GithubNote::NotMergeable { number } => format!(
            "GitHub 上で #{} はマージ可能になっていません。先にコンフリクトの解消(またはブランチ保護の条件充足)が必要です。",
            number
        ),
        GithubNote::IsDraft { number } => {
            format!("#{} はドラフトです。マージ前に Ready for review にしてください。", number)
        }
        GithubNote::ChecksFailing { number, failed } => format!(
            "#{} には失敗しているチェックが {} 件あります。このままマージすると CI が拒否したコードが入ります。",
            number, failed
        ),
        GithubNote::ChecksPending { number } => {
            format!("#{} のチェックがまだ完了していません。", number)
        }
        GithubNote::ChangesRequested { number } => {
            format!("#{} にはレビューからの修正依頼が出ています。", number)
        }
        GithubNote::RemoteSideEffect => {
            "マージは GitHub 上で行われます。次に fetch するまでローカルのクローンは変わりません。"
                .to_string()
        }
        GithubNote::DeletesBranch { branch } => format!(
            "head ブランチ '{}' はリモートで削除されます(どこにも checkout されていなければローカルでも削除されます)。",
            branch
        ),
    }
}

/// Japanese rendering of one GitHub title.
pub fn title_ja(title: &GithubTitle) -> String {
    match title {
        GithubTitle::MergePr { number, method } => {
            format!("プルリクエスト #{} をマージ ({})", number, method)
        }
    }
}

/// Japanese rendering of one GitHub recovery block.
pub fn recovery_ja(recovery: &GithubRecovery) -> String {
    match recovery {
        GithubRecovery::MergePr { number } => format!(
            "マージ後、#{} のページに 'Revert' ボタンが残ります。ローカルでは以下でマージコミットを取り消せます:\n  git revert -m 1 <merge-sha>\nブランチを削除した場合も PR ページから復元できます。",
            number
        ),
    }
}
