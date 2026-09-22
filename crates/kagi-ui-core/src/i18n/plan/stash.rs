//! JA strings for `StashNote` / `StashTitle` / `StashRecovery`
//! (ADR-0129 appendix §B-7 / §C / §D).

use kagi_domain::plan_note::stash::StashDirtyOp;
use kagi_domain::plan_note::{DirtyParts, StashNote, StashRecovery, StashTitle};

use crate::i18n::Msg;

/// JA template for `Msg::AdviceUntrackedExcluded`. The single `{}` takes the
/// untracked file count.
pub(crate) const ADVICE_UNTRACKED_EXCLUDED: &str =
    "未追跡ファイル {} 件は stash に含めません。作業ツリーに残ります。";

pub(crate) const ADVICE_STASH_UNTRACKED_INCLUDED: &str =
    "未追跡ファイル {} 件も stash に含めます(git stash push -u 相当)。";
pub(crate) const ADVICE_STASH_DIRTY_BLOCKS_APPLY: &str =
    "作業ツリーに{}があります。意図しない conflict を防ぐため、stash {} はクリーンな作業ツリーでのみ実行できます。";
pub(crate) const ADVICE_STASH_CONFLICT_UNKNOWN_FILES: &str = "(不明なファイル)";
pub(crate) const ADVICE_STASH_POP_WOULD_CONFLICT: &str =
    "stash pop すると {} 件が conflict します。stash entry は保持されるので、解決後に手動で drop してください。\nfiles {}";
pub(crate) const ADVICE_STASH_APPLY_WOULD_CONFLICT: &str =
    "stash apply すると {} 件が conflict します。stash entry は残るので、作業ツリーで解決してください。\nfiles {}";
pub(crate) const ADVICE_STASH_APPLY_PREDICTION_UNAVAILABLE: &str =
    "クリーンに適用できるか検証できませんでした({})。apply は entry を削除しないので、そのまま試せます。conflict した場合は解決してください。stash は残ります。";
pub(crate) const ADVICE_STASH_POP_PREDICTION_UNAVAILABLE: &str =
    "クリーンに適用できるか検証できませんでした({})。pop は entry を削除するためブロックしました。削除せず適用する Stash Apply を使ってください。";
pub(crate) const ADVICE_STASH_REMOTE_DROP_IRREVERSIBLE: &str =
    "remote 上の stash entry を完全に削除します。kagi からは元に戻せません。";
pub(crate) const ADVICE_STASH_TARGET_CHANGED: &str =
    "stash@{{}} は承認した entry {} ではなくなりました。別の stash がその位置にあります。何も変更していません。再 plan してください。";
pub(crate) const ADVICE_STASH_LIST_CHANGED: &str =
    "plan 以降に stash 一覧が変化しました(順序または entry が異なります)。何も変更していません。再 plan してください。";

/// `「stage 済み 2 件、変更 1 件」` — the dirty-parts fragment in JA
/// (mirrors `plan/common.rs::parts_ja`; stash has its own module so it stays
/// local rather than reaching into a sibling category file).
fn parts_ja(parts: &DirtyParts) -> String {
    let mut out: Vec<String> = Vec::new();
    if parts.staged > 0 {
        out.push(format!("stage 済み {} 件", parts.staged));
    }
    if parts.modified > 0 {
        out.push(format!("変更 {} 件", parts.modified));
    }
    out.join("、")
}

/// Japanese rendering of one stash note.
pub fn note_ja(note: &StashNote) -> String {
    match note {
        StashNote::NothingToStash => {
            "作業ツリーはすでにクリーンです。stash する対象がありません。".to_string()
        }
        StashNote::UntrackedIncluded { count } => {
            super::advice_text(Msg::AdviceStashUntrackedIncluded, &[count])
        }
        StashNote::UntrackedExcluded { count } => {
            super::advice_text(Msg::AdviceUntrackedExcluded, &[count])
        }
        // JA には単複の別がないため、count は複数形サフィックスを持たず自然に読める。
        StashNote::IndexOutOfRange { index, count } => {
            format!(
                "stash index {} は範囲外です(entry は {} 件)。",
                index, count
            )
        }
        StashNote::DirtyBlocksApply { parts, op } => {
            let op_word = match op {
                StashDirtyOp::Apply => "apply",
                StashDirtyOp::Pop => "pop",
            };
            super::advice_text(
                Msg::AdviceStashDirtyBlocksApply,
                &[&parts_ja(parts), &op_word],
            )
        }
        StashNote::PopWouldConflict { count, files } => {
            let files_label = if files.is_empty() {
                super::advice_text(Msg::AdviceStashConflictUnknownFiles, &[])
            } else {
                files.join(", ")
            };
            super::advice_text(Msg::AdviceStashPopWouldConflict, &[count, &files_label])
        }
        StashNote::ApplyWouldConflict { count, files } => {
            let files_label = if files.is_empty() {
                super::advice_text(Msg::AdviceStashConflictUnknownFiles, &[])
            } else {
                files.join(", ")
            };
            super::advice_text(Msg::AdviceStashApplyWouldConflict, &[count, &files_label])
        }
        StashNote::ApplyPredictionUnavailable { reason } => {
            super::advice_text(Msg::AdviceStashApplyPredictionUnavailable, &[reason])
        }
        StashNote::PopPredictionUnavailable { reason } => {
            super::advice_text(Msg::AdviceStashPopPredictionUnavailable, &[reason])
        }
        StashNote::RemoteDropIrreversible => {
            super::advice_text(Msg::AdviceStashRemoteDropIrreversible, &[])
        }
        StashNote::TargetChanged { index, expected } => {
            super::advice_text(Msg::AdviceStashTargetChanged, &[index, expected])
        }
        StashNote::ListChanged => super::advice_text(Msg::AdviceStashListChanged, &[]),
    }
}

/// Japanese rendering of one stash title.
pub fn title_ja(title: &StashTitle) -> String {
    match title {
        StashTitle::Push { next_count } => {
            format!("Stash push: ローカルの変更を保存({})", next_count)
        }
        StashTitle::Apply { index } => format!("Stash apply: stash@{{{}}} を復元", index),
        StashTitle::Pop { index } => format!("Stash pop: stash@{{{}}} を適用して削除", index),
        StashTitle::Drop { index } => format!("Stash drop: stash@{{{}}} を削除", index),
        StashTitle::DropRemote { label } => format!("{} を削除", label),
    }
}

/// Japanese rendering of one stash recovery block.
pub fn recovery_ja(recovery: &StashRecovery) -> String {
    match recovery {
        StashRecovery::Push { message } => format!(
            "一覧を確認:\n  git stash list\n削除せず復元:\n  git stash apply stash@{{0}}\nstash message: \"{}\"",
            message
        ),
        StashRecovery::Apply { index, message } => format!(
            "apply では stash@{{{}}} は削除されず、一覧に残ります。conflict したら手動で解決してください。stash は保持されます。\n一覧を確認:\n  git stash list\nstash message: \"{}\"",
            index, message
        ),
        StashRecovery::Pop { index, message } => format!(
            "注意: pop = apply + drop。apply 成功で stash@{{{}}} は完全に削除されます。\nentry \"{}\" は消費されます。削除せず復元するには Stash Apply を使ってください。\n一覧を確認:\n  git stash list",
            index, message
        ),
        StashRecovery::Drop { message, oid } => match oid {
            Some(oid) => format!(
                "drop は stash entry だけを削除し、作業ツリーには触れません。削除した stash commit {} は gc まで stash reflog から到達可能です。復元:\n  git stash store -m \"{}\" {}\n一覧を確認:\n  git stash list",
                oid, message, oid
            ),
            None => "drop は stash entry だけを削除し、作業ツリーには触れません。".to_string(),
        },
        StashRecovery::DropRemote => "削除した stash commit は gc まで remote の stash reflog \
             から到達可能な場合がありますが、kagi は remote の復元を管理しません。"
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_identity_notes_are_localized() {
        assert!(note_ja(&StashNote::TargetChanged {
            index: 1,
            expected: "0123456789abcdef".into(),
        })
        .contains("何も変更していません"));
        assert!(note_ja(&StashNote::ListChanged).contains("再 plan してください"));
    }
}
