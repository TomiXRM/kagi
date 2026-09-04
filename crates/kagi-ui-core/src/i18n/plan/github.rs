//! JA strings for `GithubNote`/`GithubTitle`/`GithubRecovery` (PR merge).

use kagi_domain::plan_note::{GithubNote, GithubRecovery, GithubTitle};

/// Japanese rendering of one GitHub note.
pub fn note_ja(note: &GithubNote) -> String {
    match note {
        GithubNote::NotMergeable { number } => format!(
            "GitHub 上で #{} は merge 可能になっていません。先にコンフリクトの解消(または branch 保護の条件充足)が必要です。",
            number
        ),
        GithubNote::IsDraft { number } => {
            format!("#{} はドラフトです。merge 前に Ready for review にしてください。", number)
        }
        GithubNote::ChecksFailing { number, failed } => format!(
            "#{} には失敗しているチェックが {} 件あります。このまま merge すると CI が拒否したコードが入ります。",
            number, failed
        ),
        GithubNote::ChecksPending { number } => {
            format!("#{} のチェックがまだ完了していません。", number)
        }
        GithubNote::ChangesRequested { number } => {
            format!("#{} にはレビューからの修正依頼が出ています。", number)
        }
        GithubNote::RemoteSideEffect => {
            "merge は GitHub 上で行われます。次に fetch するまでローカルのクローンは変わりません。"
                .to_string()
        }
        GithubNote::DeletesBranch { branch } => format!(
            "head branch '{}' はリモートで削除されます(どこにも checkout されていなければローカルでも削除されます)。",
            branch
        ),
        GithubNote::SuggestionRangeGone { path } => format!(
            "'{}' のレビュー対象だった行は現在のワーキングツリーに存在しません。現在のファイルに対してレビューを開き直してください。",
            path
        ),
        GithubNote::SuggestionStale { path } => format!(
            "'{}' はこの suggestion がレビューされてから変更されています。今適用すると誤った行を書き換える恐れがあるため拒否します。現在のファイルに対してレビューを開き直してください。",
            path
        ),
        GithubNote::SuggestionWorkingTreeOnly => {
            "これはワーキングツリーだけを書き換えます(コミットはされません)。コミット前に hunk staging で確認してください。"
                .to_string()
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
            format!("'{}' に suggestion を適用", path)
        }
    }
}

/// Japanese rendering of one GitHub recovery block.
pub fn recovery_ja(recovery: &GithubRecovery) -> String {
    match recovery {
        GithubRecovery::MergePr { number } => format!(
            "merge 後、#{} のページに 'Revert' ボタンが残ります。ローカルでは以下で merge commit を取り消せます:\n  git revert -m 1 <merge-sha>\nbranch を削除した場合も PR ページから復元できます。",
            number
        ),
        GithubRecovery::ApplySuggestion =>
            "これはワーキングツリーのファイルだけを書き換えます(stage も commit もしません)。適用前のファイル内容は oplog(op=\"apply-suggestion\")に blob として記録されるので、`git cat-file -p <blob-sha>` で復元するか、変更を discard できます。"
                .to_string(),
    }
}
