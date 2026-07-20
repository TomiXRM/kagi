//! JA strings for `StashNote` / `StashTitle` / `StashRecovery`
//! (ADR-0129 appendix §B-7 / §C / §D).

use kagi_domain::plan_note::stash::StashDirtyOp;
use kagi_domain::plan_note::{DirtyParts, StashNote, StashRecovery, StashTitle};

/// `「ステージ済み 2 件、変更 1 件」` — the dirty-parts fragment in JA
/// (mirrors `plan/common.rs::parts_ja`; stash has its own module so it stays
/// local rather than reaching into a sibling category file).
fn parts_ja(parts: &DirtyParts) -> String {
    let mut out: Vec<String> = Vec::new();
    if parts.staged > 0 {
        out.push(format!("ステージ済み {} 件", parts.staged));
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
            "作業ツリーはすでにクリーンです\
             (ステージ済み・変更・未追跡ファイルのいずれもありません)。stash する対象がありません。"
                .to_string()
        }
        StashNote::UntrackedIncluded { count } => format!(
            "未追跡ファイル {} 件が stash に含まれます(`git stash push -u` と同等)。",
            count
        ),
        StashNote::UntrackedExcluded { count } => format!(
            "未追跡ファイル {} 件は stash に含まれません(include_untracked=false)。\
             作業ツリーにそのまま残ります。",
            count
        ),
        // JA には単複の別がないため、count は複数形サフィックスを持たず自然に読める。
        StashNote::IndexOutOfRange { index, count } => format!(
            "stash index {} は範囲外です(存在する stash entry は {} 件のみです)。",
            index, count
        ),
        StashNote::DirtyBlocksApply { parts, op } => {
            let op_word = match op {
                StashDirtyOp::Apply => "apply",
                StashDirtyOp::Pop => "pop",
            };
            format!(
                "作業ツリーに{}があります — 意図しない merge コンフリクトを防ぐため、\
                 stash {} はクリーンな作業ツリーでのみ実行できます。",
                parts_ja(parts),
                op_word
            )
        }
        StashNote::PopWouldConflict { count, files } => {
            let files_label = if files.is_empty() {
                "(不明なファイル)".to_string()
            } else {
                files.join(", ")
            };
            format!(
                "stash pop を実行すると {} 件のコンフリクトが発生します: {}。\
                 stash entry を失わないよう pop はブロックされました。\
                 代わりに 'Stash Apply' を使用してください: stash を削除せずに適用できるため、\
                 安全にコンフリクトを解決できます。",
                count, files_label
            )
        }
        StashNote::RemoteDropIrreversible => {
            "リモートホスト上の stash entry を完全に削除します。Kagi から元に戻すことはできません。"
                .to_string()
        }
    }
}

/// Japanese rendering of one stash title.
pub fn title_ja(title: &StashTitle) -> String {
    match title {
        StashTitle::Push { next_count } => {
            format!("Stash push — ローカルの変更を保存({})", next_count)
        }
        StashTitle::Apply { index } => format!("Stash apply — stash@{{{}}} を復元", index),
        StashTitle::Pop { index } => format!("Stash pop — stash@{{{}}} を適用して削除", index),
        StashTitle::Drop { index } => format!("Stash drop — stash@{{{}}} を削除", index),
        StashTitle::DropRemote { label } => format!("{} を削除", label),
    }
}

/// Japanese rendering of one stash recovery block.
pub fn recovery_ja(recovery: &StashRecovery) -> String {
    match recovery {
        StashRecovery::Push { message } => format!(
            "stash entry を確認するには:  git stash list\n\
             stash entry を削除せずに復元するには:  git stash apply stash@{{0}}\n\
             使用される stash message: \"{}\"",
            message
        ),
        StashRecovery::Apply { index, message } => format!(
            "stash entry stash@{{{}}} は apply では削除されません — 一覧に残り続けます。\n\
             apply でコンフリクトが発生した場合は手動で解決してください。stash は安全に保持されています。\n\
             残っている stash entry を確認するには:  git stash list\n\
             stash message: \"{}\"",
            index, message
        ),
        StashRecovery::Pop { index, message } => format!(
            "警告: pop = apply + drop です。apply が成功すると、stash@{{{}}} は完全に削除されます。\n\
             stash entry \"{}\" は消費されます。\n\
             stash を削除せずに復元するには 'Stash Apply' を使用してください。\n\
             残っている stash entry を確認するには:  git stash list",
            index, message
        ),
        StashRecovery::Drop { message, oid } => match oid {
            Some(oid) => format!(
                "drop は stash entry のみを削除します — 作業ツリーには触れません。\n\
                 削除された stash commit {} は gc されるまで stash reflog から到達可能です。\
                 復元するには:\n  git stash store -m \"{}\" {}\n\
                 残っている stash entry を確認するには:  git stash list",
                oid, message, oid
            ),
            None => "drop は stash entry のみを削除します — 作業ツリーには触れません。".to_string(),
        },
        StashRecovery::DropRemote => "削除された stash commit は gc されるまでリモートの stash reflog \
             から到達可能な場合がありますが、Kagi はリモートの復元を管理しません。"
            .to_string(),
    }
}
