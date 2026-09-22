//! JA strings for the cross-op `CommonNote` templates (ADR-0129 §A).

use kagi_domain::plan_note::{CommonNote, DirtyParts, OpPhrase, PlanOp, UntrackedCtx};

use crate::i18n::{branch_name_error, worktree_path_error, Msg};

/// JA text for `Msg::AdviceSuggestStashPush`.
pub(crate) const ADVICE_SUGGEST_STASH_PUSH: &str = "推奨コマンド: git stash push -u";

/// JA text for `Msg::AdviceDirtyStashFirst` — copied verbatim from the former
/// `Msg::DirtyStashFirst` JA arm. `stash@{0}` is literal git syntax, not a
/// positional placeholder (the note takes no arguments).
pub(crate) const ADVICE_DIRTY_STASH_FIRST: &str =
    "Working tree が dirty です: 確定すると先に変更を stash します(stash@{0} に保存、`git stash pop` で復元)";

pub(crate) const ADVICE_COMMON_BRANCH_INVALID_REF: &str =
    "branch 名 '{}' は有効な git ref 名ではありません(空白・'..' などは使えません)。";

/// JA template for `Msg::AdviceCommonConflictedFiles`.
pub(crate) const ADVICE_COMMON_CONFLICTED_FILES: &str =
    "conflict が {} 件あります。{}の前に解決してください。";

/// JA template for `Msg::AdviceCommonDirtyBlocksOp`.
pub(crate) const ADVICE_COMMON_DIRTY_BLOCKS_OP: &str =
    "作業ツリーに{}があります。{}の前に stash か commit してください。";

/// JA template for `Msg::AdviceCommonDirtyRollbackHint`.
pub(crate) const ADVICE_COMMON_DIRTY_ROLLBACK_HINT: &str =
    "作業ツリーに{}があります。クリーンな復帰点を残すには {} の前に stash か commit してください。";

/// JA template for `Msg::AdviceCommonPartialCloneObjectMissing`.
pub(crate) const ADVICE_COMMON_PARTIAL_CLONE_OBJECT_MISSING: &str =
    "この repository は partial clone で、必要な object がまだ取得されていないため読めません({})。repository で `git fetch` を実行して取得してください。`git` は不足 object を必要に応じて取得しますが、Kagi は取得しません。";

/// JA template for `Msg::AdviceCommonSparseExcludedPath`.
pub(crate) const ADVICE_COMMON_SPARSE_EXCLUDED_PATH: &str =
    "'{}' は sparse-checkout で除外されているため、削除されたのではなく意図的に作業ツリーに存在しません。stage すると、していない削除を記録することになります。git も同じ操作を拒否します。変更するつもりなら、先に sparse-checkout の定義を広げてください。";

/// JA text for `Msg::AdviceCommonMergeConflictWarning`.
pub(crate) const ADVICE_COMMON_MERGE_CONFLICT_WARNING: &str =
    "この merge は conflict を発生させます。conflict marker を残して Conflict Mode に入り、各ファイルを解決します(中止すれば merge 前の状態に戻せます)。";

/// JA template for `Msg::AdviceUntrackedRemain` — one per sentence tail.
/// The single `{}` takes the untracked file count.
pub(crate) fn untracked_advice_ja(ctx: UntrackedCtx) -> &'static str {
    match ctx {
        UntrackedCtx::AfterCheckout => "未追跡ファイル {} 件は checkout 後もそのまま残ります。",
        UntrackedCtx::AfterSwitching => "未追跡ファイル {} 件は切り替え後もそのまま残ります。",
        UntrackedCtx::AfterSwitchingBranches => {
            "未追跡ファイル {} 件は branch 切り替え後もそのまま残ります。"
        }
        UntrackedCtx::AfterCherryPick => "未追跡ファイル {} 件は cherry-pick の影響を受けません。",
        UntrackedCtx::AfterRevert => "未追跡ファイル {} 件は revert の影響を受けません。",
        UntrackedCtx::PullFetchMayTouch => {
            "未追跡ファイル {} 件は、取得した変更が同じパスに触れない限りそのまま残ります。"
        }
        UntrackedCtx::Untouched => "未追跡ファイル {} 件はそのまま残ります。",
    }
}

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
        CommonNote::ConflictedFiles { count, before } => super::advice_text(
            Msg::AdviceCommonConflictedFiles,
            &[count, &phrase_ja(*before)],
        ),
        CommonNote::DirtyBlocksOp { parts, before } => super::advice_text(
            Msg::AdviceCommonDirtyBlocksOp,
            &[&parts_ja(parts), &phrase_ja(*before)],
        ),
        CommonNote::SuggestStashPush => super::advice_text(Msg::AdviceSuggestStashPush, &[]),
        CommonNote::UntrackedRemain { count, ctx } => {
            super::advice_text(Msg::AdviceUntrackedRemain(*ctx), &[count])
        }
        CommonNote::DirtyRollbackHint { parts, op } => super::advice_text(
            Msg::AdviceCommonDirtyRollbackHint,
            &[&parts_ja(parts), &phrase_ja(*op)],
        ),
        CommonNote::PartialCloneObjectMissing { detail } => {
            super::advice_text(Msg::AdviceCommonPartialCloneObjectMissing, &[detail])
        }
        CommonNote::SparseExcludedPath { path } => {
            super::advice_text(Msg::AdviceCommonSparseExcludedPath, &[path])
        }
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
            format!("branch `{}` は存在しません。", name)
        }
        // Error messages stay untranslated (error keying is out of scope).
        CommonNote::GitErrorPassthrough { message } => message.clone(),
        // §E — delegate to the existing keyed-error localizers so there is
        // exactly one source of truth for their JA text.
        CommonNote::BranchNameErrorKeyed(e) => branch_name_error(e),
        CommonNote::WorktreePathErrorKeyed(e) => worktree_path_error(e),
        CommonNote::DirtyStashFirst => super::advice_text(Msg::AdviceDirtyStashFirst, &[]),
        CommonNote::MergeConflictWarning => {
            super::advice_text(Msg::AdviceCommonMergeConflictWarning, &[])
        }
    }
}
