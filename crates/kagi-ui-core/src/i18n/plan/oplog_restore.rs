//! JA strings for `OplogRestoreNote`/`Title`/`Recovery` (op-revert /
//! restore-to-point, #334 slice 2b).

use kagi_domain::plan_note::{HeadAt, OplogRestoreNote, OplogRestoreRecovery, OplogRestoreTitle};

fn short(oid: &Option<String>) -> String {
    match oid {
        Some(oid) => oid.get(..7).unwrap_or(oid).to_string(),
        None => "(なし)".to_string(),
    }
}

/// `main` / `abc1234 の detached HEAD` / `(不明)`.
fn head_ja(head: &HeadAt) -> String {
    match head {
        HeadAt::Branch(b) => format!("`{b}`"),
        HeadAt::Detached(oid) => format!("{} の detached HEAD", oid.get(..7).unwrap_or(oid)),
        HeadAt::Unknown => "(不明)".to_string(),
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
        OplogRestoreNote::HeadMoved {
            id,
            op,
            from,
            to,
            worktree,
        } => {
            let place = worktree
                .as_ref()
                .map(|p| format!("{p} の worktree で"))
                .unwrap_or_default();
            format!(
                "操作 #{id}({op})で{place} HEAD が {} から {} に切り替わっているので、この範囲は restore できません。restore は branch だけを動かし、HEAD は動かしません(作業ツリーが変わるため)。{place} {} を自分で checkout してから、#{id} 以降の時点へ restore してください。それより複雑な場合(複数回の切り替え、その操作が作成・削除した branch など)は手で戻してください。",
                head_ja(from),
                head_ja(to),
                head_ja(from)
            )
        }
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
        OplogRestoreNote::OperationInProgress { op, path } => format!(
            "{path} で {} が進行中です。完了か abort してからにしてください。",
            op.label_ja()
        ),
        OplogRestoreNote::HistoryGap { after, next } => format!(
            "Operation Log が操作 #{after} の後で途切れています(次の記録は #{next})。間の操作が消えているか読めないため、この範囲は正確に戻せません。"
        ),
        OplogRestoreNote::UnknownRepository { id, op, path } => format!(
            "操作 #{id}({op})を実行した {path} はもう開けないため、この repository の branch を動かした可能性があります。この範囲は正確に戻せません。"
        ),
        OplogRestoreNote::RefChangedOutsideRecord { refname } => format!(
            "{refname} はその時点の後に、記録された操作の外で作られたか動いています。復元してもそのまま残ります。"
        ),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// #886: the HEAD blocker names the entry, both sides of the switch, and
    /// what to check out by hand before restoring to that entry or later —
    /// in both languages.
    #[test]
    fn head_moved_names_the_switch_and_the_manual_step() {
        let note = OplogRestoreNote::HeadMoved {
            id: 42,
            op: "checkout-commit".into(),
            from: HeadAt::Branch("feature".into()),
            to: HeadAt::Detached("0123456789abcdef".into()),
            worktree: None,
        };
        let en = note.message_en();
        let ja = note_ja(&note);
        for text in [&en, &ja] {
            assert!(text.contains("#42"), "{text}");
            assert!(text.contains("checkout-commit"), "{text}");
            assert!(text.contains("feature"), "{text}");
            assert!(text.contains("0123456"), "{text}");
            assert!(!text.contains("0123456789abcdef"), "short oid: {text}");
        }
        // The manual step names the side to go back to, and the restore
        // target is the entry itself.
        assert!(en.contains("Check out 'feature' yourself"), "{en}");
        assert!(en.contains("restore to #42 or a later point"), "{en}");
        assert!(ja.contains("`feature` を自分で checkout"), "{ja}");
        assert!(ja.contains("#42 以降の時点へ restore"), "{ja}");
        // #912 review: one guidance only; anything more involved is by hand.
        assert!(en.contains("has to be done by hand"), "{en}");
        assert!(ja.contains("手で戻してください"), "{ja}");
    }
}
