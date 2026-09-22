//! JA strings for `CherryRevertNote` / `CherryRevertTitle` / `CherryRevertRecovery`
//! (ADR-0129 appendix §B-3 / §C / §D).

use kagi_domain::plan_note::{
    CherryRevertNote, CherryRevertRecovery, CherryRevertTitle, DirtyParts, PlanOp,
};

use crate::i18n::Msg;

pub(crate) const ADVICE_CHERRY_REVERT_WOULD_CONFLICT_CHERRY_PICK: &str =
    "cherry-pick すると {} 件 conflict します。先に解決してください。\nfiles {}";
pub(crate) const ADVICE_CHERRY_REVERT_WOULD_CONFLICT_REVERT: &str =
    "revert すると {} 件 conflict します。先に解決してください。\nfiles {}";
pub(crate) const ADVICE_CHERRY_REVERT_DIRTY_MAY_REFUSE: &str =
    "作業ツリーに{}があります。対象ファイルが重複すると安全な checkout が拒否されることがあります。";

/// `「stage 済み 2 件、変更 1 件」` — the dirty-parts fragment in JA (mirrors
/// `i18n::plan::common::parts_ja`; kept local since that helper is private to
/// its own module).
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

/// Japanese rendering of one cherry_revert note.
pub fn note_ja(note: &CherryRevertNote) -> String {
    match note {
        CherryRevertNote::MergeCommitNeedsMainline { sha, parents, op } => match op {
            PlanOp::CherryPick => format!(
                "merge commit です(親 {} 個)。merge commit の cherry-pick は未対応です。\ncommit `{}`",
                parents, sha
            ),
            PlanOp::Revert => format!(
                "merge commit です(親 {} 個)。merge commit の revert は未対応です。\ncommit `{}`",
                parents, sha
            ),
            _ => unreachable!(
                "CherryRevertNote::MergeCommitNeedsMainline only uses CherryPick/Revert"
            ),
        },
        CherryRevertNote::NothingToCherryPickHead { sha } => format!(
            "現在の HEAD commit です。cherry-pick する対象がありません。\ncommit `{}`",
            sha
        ),
        CherryRevertNote::WouldConflict { count, files, op } => {
            let joined = files.join(", ");
            match op {
                PlanOp::CherryPick => super::advice_text(
                    Msg::AdviceCherryRevertWouldConflictCherryPick,
                    &[count, &joined],
                ),
                PlanOp::Revert => super::advice_text(
                    Msg::AdviceCherryRevertWouldConflictRevert,
                    &[count, &joined],
                ),
                _ => unreachable!("CherryRevertNote::WouldConflict only uses CherryPick/Revert"),
            }
        }
        CherryRevertNote::NoChanges { sha, op } => match op {
            PlanOp::CherryPick => format!(
                "cherry-pick しても変更はありません。すでに適用済みのようです。\ncommit `{}`",
                sha
            ),
            PlanOp::Revert => format!("revert しても変更はありません。\ncommit `{}`", sha),
            _ => unreachable!("CherryRevertNote::NoChanges only uses CherryPick/Revert"),
        },
        CherryRevertNote::NotInCurrentBranch { sha } => format!(
            "現在の branch に含まれていません。revert は現在の branch 上の commit のみが対象です。\ncommit `{}`",
            sha
        ),
        CherryRevertNote::DirtyMayRefuse { parts } => {
            let parts_label = parts_ja(parts);
            super::advice_text(
                Msg::AdviceCherryRevertDirtyMayRefuse,
                &[&parts_label],
            )
        }
    }
}

/// Japanese rendering of one cherry_revert title.
pub fn title_ja(title: &CherryRevertTitle) -> String {
    match title {
        CherryRevertTitle::CherryPick {
            sha,
            summary: Some(summary),
            branch,
        } => format!(
            "cherry-pick: `{}` \"{}\" → branch `{}`",
            sha, summary, branch
        ),
        CherryRevertTitle::CherryPick {
            sha,
            summary: None,
            branch,
        } => format!("cherry-pick: `{}` → branch `{}`", sha, branch),
        CherryRevertTitle::Revert {
            sha,
            summary,
            branch,
        } => format!("revert: `{}` \"{}\"(branch `{}`)", sha, summary, branch),
    }
}

/// Japanese rendering of one cherry_revert recovery block.
pub fn recovery_ja(recovery: &CherryRevertRecovery) -> String {
    match recovery {
        CherryRevertRecovery::AfterCherryPick => {
            "取り消す:\n  git revert <new-commit-sha>\n以前の HEAD は reflog に記録されています:\n  git reflog"
                .to_string()
        }
        CherryRevertRecovery::AfterRevert => {
            "取り消すには、作成された revert commit をさらに revert:\n  git revert <new-revert-commit-sha>\n以前の HEAD は reflog に記録されています:\n  git reflog"
                .to_string()
        }
    }
}
