//! JA strings for the cross-op `CommonNote` templates (ADR-0129 §A).

use kagi_domain::plan_note::{CommonNote, DirtyParts, OpPhrase, PlanOp, UntrackedCtx};

use crate::i18n::{branch_name_error, worktree_path_error};

/// JA rendering of the op phrase embedded in the common sentences.
fn phrase_ja(p: OpPhrase) -> &'static str {
    match p {
        OpPhrase::UndoingACommit => "commit の取り消し",
        OpPhrase::Amending => "amend",
        OpPhrase::Undo => "undo",
        OpPhrase::Redo => "redo",
        OpPhrase::Checkout => "checkout",
        OpPhrase::Switching => "branch 切り替え",
        OpPhrase::CherryPicking => "cherry-pick",
        OpPhrase::Reverting => "revert",
        OpPhrase::Pulling => "pull",
        OpPhrase::Merging => "merge",
        OpPhrase::SwitchingBranches => "branch 切り替え",
        OpPhrase::Stashing => "stash",
        OpPhrase::ApplyingAStash => "stash の適用",
        OpPhrase::CheckingOutTheNewBranch => "新しい branch の checkout",
    }
}

/// JA rendering of the op name in the HEAD-state sentences.
fn op_ja(op: PlanOp) -> &'static str {
    match op {
        PlanOp::Undo => "commit の取り消し",
        PlanOp::Amend => "amend",
        PlanOp::CherryPick => "cherry-pick",
        PlanOp::Revert => "revert",
        PlanOp::Pull => "pull",
        PlanOp::Push => "push",
        PlanOp::Merge => "merge",
    }
}

/// `「stage 済み 2 件、変更 1 件」` — the dirty-parts fragment in JA.
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

/// Japanese rendering of one cross-op note.
pub fn note_ja(note: &CommonNote) -> String {
    match note {
        CommonNote::ConflictedFiles { count, before } => format!(
            "リポジトリに {} 件のコンフリクトファイルがあります。{}の前にコンフリクトを解決してください。",
            count,
            phrase_ja(*before)
        ),
        CommonNote::DirtyBlocksOp { parts, before } => format!(
            "作業ツリーに{}があります — {}の前に stash するか commit してください。",
            parts_ja(parts),
            phrase_ja(*before)
        ),
        CommonNote::SuggestStashPush => "推奨コマンド: git stash push -u".to_string(),
        CommonNote::UntrackedRemain { count, ctx } => match ctx {
            UntrackedCtx::AfterCheckout => format!(
                "未追跡ファイル {} 件は checkout 後もそのまま残ります。",
                count
            ),
            UntrackedCtx::AfterSwitching => format!(
                "未追跡ファイル {} 件は切り替え後もそのまま残ります。",
                count
            ),
            UntrackedCtx::AfterSwitchingBranches => format!(
                "未追跡ファイル {} 件は branch 切り替え後もそのまま残ります。",
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
            "作業ツリーに{}があります。クリーンな復帰点を残したい場合は {} の前に stash か commit をしてください。",
            parts_ja(parts),
            phrase_ja(*op)
        ),
        CommonNote::HeadDetached { op } => format!(
            "HEAD が detached 状態です。{} は branch 上でのみ実行できます。",
            op_ja(*op)
        ),
        CommonNote::HeadUnborn { op } => {
            let tail = match op {
                PlanOp::Undo => "取り消す commit がありません。",
                PlanOp::Amend => "amend する commit がありません。",
                PlanOp::CherryPick => "空の branch には cherry-pick できません。",
                PlanOp::Revert => "空の branch では revert できません。",
                PlanOp::Pull => "空の branch には pull できません。",
                PlanOp::Push => "空の branch は push できません。",
                PlanOp::Merge => "空の branch には merge できません。",
            };
            format!("HEAD が unborn(commit が存在しません)です。{}", tail)
        }
        CommonNote::BranchMissing { name, .. } => {
            format!("branch '{}' は存在しません。", name)
        }
        // Error messages stay untranslated (error keying is out of scope).
        CommonNote::GitErrorPassthrough { message } => message.clone(),
        // §E — delegate to the existing keyed-error localizers so there is
        // exactly one source of truth for their JA text.
        CommonNote::BranchNameErrorKeyed(e) => branch_name_error(e),
        CommonNote::WorktreePathErrorKeyed(e) => worktree_path_error(e),
        // §F-6 — copied verbatim from the former `Msg::DirtyStashFirst` /
        // `Msg::MergeConflictWarning` JA arms (ADR-0129 Phase 3: same text,
        // now typed).
        CommonNote::DirtyStashFirst => {
            "Working tree が dirty です: 確定すると先に変更を stash します\
             (stash@{0} に保存、`git stash pop` で復元)"
                .to_string()
        }
        CommonNote::MergeConflictWarning => {
            "この merge は conflict を発生させます。conflict marker を残して Conflict Mode に入り、各ファイルを解決します(中止すれば merge 前の状態に戻せます)。"
                .to_string()
        }
    }
}
