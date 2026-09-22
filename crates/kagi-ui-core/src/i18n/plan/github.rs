//! JA strings for `GithubNote`/`GithubTitle`/`GithubRecovery` (PR merge).

use kagi_domain::plan_note::{GithubNote, GithubRecovery, GithubTitle};

/// Japanese rendering of one GitHub note.
pub fn note_ja(note: &GithubNote) -> String {
    match note {
        GithubNote::HeadUnavailable { number } => format!(
            "#{} の head commit を取得できていません。pull request 一覧を更新してから merge してください。",
            number
        ),
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
            "head branch を remote で削除します。\nbranch `{}`",
            branch
        ),
        GithubNote::ForkKeepsRemoteBranch => {
            "fork 側の remote branch は gh では削除されません。".to_string()
        }
        GithubNote::DeletesLocalBranch { branch, tip } => match tip {
            Some(tip) => format!(
                "GitHub 上で merge が確認できたあと、local branch を削除します。削除するのは、この commit を指したままで、どこにも checkout されていない場合だけです。\nbranch `{}`\ntip `{}`",
                branch, tip
            ),
            None => format!(
                "local branch はこのリポジトリに存在しません。merge 後も local では何も削除しません。\nbranch `{}`",
                branch
            ),
        },
        GithubNote::LocalBranchDeleted { name, tip } => {
            format!("local branch を削除しました: {}@{}", name, tip)
        }
        GithubNote::LocalBranchAbsent { name } => {
            format!("local branch は既に存在しません: {}", name)
        }
        GithubNote::LocalBranchNotDeleted { reason } => {
            format!("merge は完了しました。local branch は削除していません: {}", reason)
        }
        GithubNote::SuggestionRangeGone { path } => format!(
            "レビュー対象だった行が作業ツリーにありません。現在のファイルでレビューを開き直してください。\nfile `{}`",
            path
        ),
        GithubNote::SuggestionStale { path } => format!(
            "suggestion のレビュー後にファイルが変更されています。誤った行を書き換える恐れがあるため拒否します。現在のファイルでレビューを開き直してください。\nfile `{}`",
            path
        ),
        GithubNote::CommentBodyEmpty => {
            "コメント本文が空です。投稿する文章を入力してください。".to_string()
        }
        GithubNote::IssueTitleEmpty => {
            "Issue のタイトルが空です。タイトルか、本文にタイトルとして使える内容を入力してください。"
                .to_string()
        }
        GithubNote::ReviewBodyEmpty { verdict } => format!(
            "GitHub は「{}」のレビューにコメントを必須としています。何を変えてほしいかを書いてから提出してください。",
            verdict
        ),
        GithubNote::FieldEditEmpty { number } => format!(
            "#{} は何も変わりません。追加または削除する reviewer / 担当 / label を選んでください。",
            number
        ),
        GithubNote::SuggestionWorkingTreeOnly => {
            "作業ツリーだけを書き換えます(commit しません)。commit 前に hunk staging で確認してください。".to_string()
        }
    }
}

/// Japanese rendering of one GitHub title.
pub fn title_ja(title: &GithubTitle) -> String {
    match title {
        GithubTitle::CreateIssue => "Issue を作成".into(),
        GithubTitle::CommentIssue { number } => format!("Issue #{number} に返信"),
        GithubTitle::MergePr { number, method } => {
            format!("pull request #{} を merge ({})", number, method)
        }
        GithubTitle::CommentPr { number } => {
            format!("pull request #{} にコメントを投稿", number)
        }
        GithubTitle::ReviewPr { number, verdict } => {
            format!("pull request #{} をレビュー ({})", number, verdict)
        }
        GithubTitle::EditPr { number } => {
            format!("pull request #{} を編集", number)
        }
        GithubTitle::ApplySuggestion { path } => {
            format!("`{}` に suggestion を適用", path)
        }
    }
}

/// Japanese rendering of one GitHub recovery block.
pub fn recovery_ja(recovery: &GithubRecovery) -> String {
    match recovery {
        GithubRecovery::MergePr { number, .. } => format!(
            "merge 後も #{} のページに Revert ボタンが残ります。ローカルでは:\n  git revert -m 1 <merge-sha>\nbranch を削除しても PR ページから復元できます。",
            number
        ),
        GithubRecovery::ApplySuggestion =>
            "作業ツリーのファイルだけを書き換えます(stage も commit もしません)。適用前の内容は oplog(op=\"apply-suggestion\")に blob として記録されます:\n  git cat-file -p <blob-sha>"
                .to_string(),
    }
}
