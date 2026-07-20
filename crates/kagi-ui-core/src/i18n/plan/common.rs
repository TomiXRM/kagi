//! JA strings for the cross-op `CommonNote` templates (ADR-0129 §A).

use kagi_domain::plan_note::{CommonNote, DirtyParts, OpPhrase, PlanOp, UntrackedCtx};

/// JA rendering of the op phrase embedded in the common sentences.
fn phrase_ja(p: OpPhrase) -> &'static str {
    match p {
        OpPhrase::UndoingACommit => "コミットの取り消し",
        OpPhrase::Amending => "amend",
        OpPhrase::Undo => "undo",
        OpPhrase::Redo => "redo",
        OpPhrase::Checkout => "チェックアウト",
        OpPhrase::Switching => "ブランチ切り替え",
        OpPhrase::CherryPicking => "cherry-pick",
        OpPhrase::Reverting => "revert",
        OpPhrase::Pulling => "pull",
        OpPhrase::Merging => "merge",
        OpPhrase::SwitchingBranches => "ブランチ切り替え",
        OpPhrase::Stashing => "stash",
        OpPhrase::ApplyingAStash => "stash の適用",
        OpPhrase::CheckingOutTheNewBranch => "新しいブランチのチェックアウト",
    }
}

/// JA rendering of the op name in the HEAD-state sentences.
fn op_ja(op: PlanOp) -> &'static str {
    match op {
        PlanOp::Undo => "コミットの取り消し",
        PlanOp::Amend => "amend",
        PlanOp::CherryPick => "cherry-pick",
        PlanOp::Revert => "revert",
        PlanOp::Pull => "pull",
        PlanOp::Push => "push",
        PlanOp::Merge => "merge",
    }
}

/// `「ステージ済み 2 件、変更 1 件」` — the dirty-parts fragment in JA.
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

/// Japanese rendering of one cross-op note.
pub fn note_ja(note: &CommonNote) -> String {
    match note {
        CommonNote::ConflictedFiles { count, before } => format!(
            "リポジトリに {} 件のコンフリクトファイルがあります。{}の前にコンフリクトを解決してください。",
            count,
            phrase_ja(*before)
        ),
        CommonNote::DirtyBlocksOp { parts, before } => format!(
            "作業ツリーに{}があります — {}の前に stash するかコミットしてください。",
            parts_ja(parts),
            phrase_ja(*before)
        ),
        CommonNote::SuggestStashPush => "推奨コマンド: git stash push -u".to_string(),
        CommonNote::UntrackedRemain { count, ctx } => match ctx {
            UntrackedCtx::AfterCheckout => format!(
                "未追跡ファイル {} 件はチェックアウト後もそのまま残ります。",
                count
            ),
            UntrackedCtx::AfterSwitching => format!(
                "未追跡ファイル {} 件は切り替え後もそのまま残ります。",
                count
            ),
            UntrackedCtx::AfterSwitchingBranches => format!(
                "未追跡ファイル {} 件はブランチ切り替え後もそのまま残ります。",
                count
            ),
            UntrackedCtx::AfterCherryPick => format!(
                "未追跡ファイル {} 件は cherry-pick の影響を受けません。",
                count
            ),
            UntrackedCtx::AfterRevert => format!(
                "未追跡ファイル {} 件は revert の影響を受けません。",
                count
            ),
            UntrackedCtx::PullFetchMayTouch => format!(
                "未追跡ファイル {} 件は、取得した変更が同じパスに触れない限りそのまま残ります。",
                count
            ),
            UntrackedCtx::Untouched => {
                format!("未追跡ファイル {} 件はそのまま残ります。", count)
            }
        },
        CommonNote::DirtyRollbackHint { parts, op } => format!(
            "作業ツリーに{}があります。クリーンな復帰点を残したい場合は {} の前に stash かコミットをしてください。",
            parts_ja(parts),
            phrase_ja(*op)
        ),
        CommonNote::HeadDetached { op } => format!(
            "HEAD が detached 状態です。{} はブランチ上でのみ実行できます。",
            op_ja(*op)
        ),
        CommonNote::HeadUnborn { op } => {
            let tail = match op {
                PlanOp::Undo => "取り消すコミットがありません。",
                PlanOp::Amend => "amend するコミットがありません。",
                PlanOp::CherryPick => "空のブランチには cherry-pick できません。",
                PlanOp::Revert => "空のブランチでは revert できません。",
                PlanOp::Pull => "空のブランチには pull できません。",
                PlanOp::Push => "空のブランチは push できません。",
                PlanOp::Merge => "空のブランチには merge できません。",
            };
            format!("HEAD が unborn(コミットが存在しません)です。{}", tail)
        }
        CommonNote::BranchMissing { name, .. } => {
            format!("ブランチ '{}' は存在しません。", name)
        }
        // Error messages stay untranslated (error keying is out of scope).
        CommonNote::GitErrorPassthrough { message } => message.clone(),
    }
}
