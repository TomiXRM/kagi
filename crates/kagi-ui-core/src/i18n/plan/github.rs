//! JA strings for `GithubNote`/`GithubTitle`/`GithubRecovery` (PR merge).

use kagi_domain::plan_note::{GithubNote, GithubRecovery, GithubTitle, PrMergeLocalReason};

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
        GithubNote::KeepsLocalBranch { branch, reason } => format!(
            "local branch は削除しません: {}\nbranch `{}`",
            reason_ja(reason),
            branch
        ),
        GithubNote::LocalBranchDeleted { name, tip } => {
            format!("local branch を削除しました: {}@{}", name, tip)
        }
        GithubNote::LocalBranchAbsent { name } => {
            format!("local branch は既に存在しません: {}", name)
        }
        GithubNote::LocalBranchKept { name, reason } => {
            format!("local branch を残しました: {} ({})", name, reason_ja(reason))
        }
        GithubNote::LocalBranchNotDeleted { reason } => {
            format!(
                "merge は完了しました。local branch は削除していません: {}",
                reason_ja(reason)
            )
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

/// Japanese rendering of one kept-local-branch reason.
///
/// [`PrMergeLocalReason::Plan`] recurses through the **whole** plan-note
/// dispatch: the delete-branch family's refusal is already translated, and
/// the bug this replaces was exactly a flattened English blocker showing up
/// inside a Japanese notice (#705 review P3).
pub fn reason_ja(reason: &PrMergeLocalReason) -> String {
    match reason {
        PrMergeLocalReason::Plan(note) => super::note_ja_any(note),
        PrMergeLocalReason::NotAtPrHead { tip, head } => format!(
            "local branch は {} を指しており、merge された PR head {} と一致しません",
            tip, head
        ),
        PrMergeLocalReason::Queued => {
            "GitHub が merge を queue に入れたため、まだ merge されていません".to_string()
        }
        PrMergeLocalReason::Changed => "承認後に branch が動きました".to_string(),
        PrMergeLocalReason::HeadChanged => "承認後に HEAD が変わりました".to_string(),
        PrMergeLocalReason::IdentityChanged => "承認時の repository ではありません".to_string(),
        PrMergeLocalReason::DeletionUnauthorized { detail } => format!(
            "gh がエラーを返したため、local branch の削除は transport に許可されていません: {}",
            detail
        ),
        PrMergeLocalReason::Detail(detail) => detail.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{plan::plan_note_text, set_lang_no_persist, Lang};
    use kagi_domain::plan_note::{branch::BranchNote, PlanNote};

    /// The reason a merge kept the local branch is carried, not printed: a
    /// blocker raised by the delete-branch family reaches the Japanese notice
    /// in **Japanese**. Flattening it to English was the bug (#705 review P3).
    #[test]
    fn a_kept_branch_reason_is_localized_all_the_way_down() {
        let _g = crate::i18n::tests::LOCK.lock();
        let blocker = PlanNote::Branch(BranchNote::DeleteBranchCheckedOut {
            name: "feat/x".into(),
            path: "/w/other".into(),
        });
        let note = PlanNote::Github(GithubNote::LocalBranchNotDeleted {
            reason: PrMergeLocalReason::Plan(Box::new(blocker.clone())),
        });
        set_lang_no_persist(Lang::Ja);
        let ja = plan_note_text(&note);
        assert!(
            ja.contains("checkout 中です"),
            "the blocker must read in Japanese: {ja}"
        );
        assert!(
            !ja.contains(&blocker.message_en()),
            "no English inside the Japanese notice: {ja}"
        );
        set_lang_no_persist(Lang::En);
        assert_eq!(plan_note_text(&note), note.message_en());
    }
}
