//! JA strings for `GithubNote`/`GithubTitle`/`GithubRecovery` (PR merge).

use kagi_domain::plan_note::{GithubNote, GithubRecovery, GithubTitle};

/// Japanese rendering of one GitHub note.
pub fn note_ja(note: &GithubNote) -> String {
    match note {
        GithubNote::NotMergeable { number } => format!(
            "#{} は merge できません。conflict 解消か branch 保護条件の充足が必要です。",
            number
        ),
        GithubNote::IsDraft { number } => {
            format!("#{} は draft です。merge 前に Ready for review にしてください。", number)
        }
        GithubNote::ChecksFailing { number, failed } => format!(
            "#{} で {} 件のチェックが失敗しています。merge すると CI 不合格のコードが入ります。",
            number, failed
        ),
        GithubNote::ChecksPending { number } => {
            format!("#{} のチェックがまだ完了していません。", number)
        }
        GithubNote::ChangesRequested { number } => {
            format!("#{} にレビューの修正依頼があります。", number)
        }
        GithubNote::RemoteSideEffect => {
            "merge は GitHub 上で実行されます。次の fetch までローカルは変わりません。".to_string()
        }
        GithubNote::DeletesBranch { branch } => format!(
            "head branch をリモートで削除します。どこにも checkout されていなければローカルも削除します。\nbranch `{}`",
            branch
        ),
        GithubNote::SuggestionRangeGone { path } => format!(
            "レビュー対象だった行が作業ツリーにありません。現在のファイルでレビューを開き直してください。\nfile `{}`",
            path
        ),
        GithubNote::SuggestionStale { path } => format!(
            "suggestion のレビュー後にファイルが変更されています。誤った行を書き換える恐れがあるため拒否します。現在のファイルでレビューを開き直してください。\nfile `{}`",
            path
        ),
        GithubNote::SuggestionWorkingTreeOnly => {
            "作業ツリーだけを書き換えます(commit しません)。commit 前に hunk staging で確認してください。".to_string()
        }
    }
}

/// Japanese rendering of one GitHub title.
pub fn title_ja(title: &GithubTitle) -> String {
    match title {
        GithubTitle::MergePr { number, method } => {
            format!("pull request #{} を merge ({})", number, method)
        }
        GithubTitle::ApplySuggestion { path } => {
            format!("`{}` に suggestion を適用", path)
        }
    }
}

/// Japanese rendering of one GitHub recovery block.
pub fn recovery_ja(recovery: &GithubRecovery) -> String {
    match recovery {
        GithubRecovery::MergePr { number } => format!(
            "merge 後も #{} のページに Revert ボタンが残ります。ローカルでは:\n  git revert -m 1 <merge-sha>\nbranch を削除しても PR ページから復元できます。",
            number
        ),
        GithubRecovery::ApplySuggestion =>
            "作業ツリーのファイルだけを書き換えます(stage も commit もしません)。適用前の内容は oplog(op=\"apply-suggestion\")に blob として記録されます:\n  git cat-file -p <blob-sha>"
                .to_string(),
    }
}
