//! JA strings for `CherryRevertNote` / `CherryRevertTitle` / `CherryRevertRecovery`
//! (ADR-0129 appendix §B-3 / §C / §D).

use kagi_domain::plan_note::{
    CherryRevertNote, CherryRevertRecovery, CherryRevertTitle, DirtyParts, PlanOp,
};

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
                "commit {} は merge commit です({} 個の親)。merge commit の cherry-pick には \
                 mainline の明示的な指定が必要ですが、MVP では未対応です。",
                sha, parents
            ),
            PlanOp::Revert => format!(
                "commit {} は merge commit です({} 個の親)。merge commit の revert には \
                 mainline の明示的な指定が必要ですが、MVP では未対応です。",
                sha, parents
            ),
            _ => unreachable!(
                "CherryRevertNote::MergeCommitNeedsMainline only uses CherryPick/Revert"
            ),
        },
        CherryRevertNote::NothingToCherryPickHead { sha } => format!(
            "commit {} は現在の HEAD commit です。cherry-pick する対象がありません。",
            sha
        ),
        CherryRevertNote::WouldConflict { count, files, op } => {
            let joined = files.join(", ");
            match op {
                PlanOp::CherryPick => format!(
                    "cherry-pick すると {} 件のコンフリクトが発生します: {}。cherry-pick の前に \
                     差分を解決してください。",
                    count, joined
                ),
                PlanOp::Revert => format!(
                    "revert すると {} 件のコンフリクトが発生します: {}。revert の前に差分を \
                     解決してください。",
                    count, joined
                ),
                _ => unreachable!("CherryRevertNote::WouldConflict only uses CherryPick/Revert"),
            }
        }
        CherryRevertNote::NoChanges { sha, op } => match op {
            PlanOp::CherryPick => format!(
                "{} を cherry-pick しても変更は発生しません — すでに適用済みのようです。",
                sha
            ),
            PlanOp::Revert => format!("{} を revert しても変更は発生しません。", sha),
            _ => unreachable!("CherryRevertNote::NoChanges only uses CherryPick/Revert"),
        },
        CherryRevertNote::NotInCurrentBranch { sha } => format!(
            "commit {} は現在の branch に含まれていません。revert は現在の branch 上の commit\
             のみを対象とします。",
            sha
        ),
        CherryRevertNote::DirtyMayRefuse { parts } => format!(
            "作業ツリーに{}があります。対象ファイルが revert と重複する場合、安全な checkout が\
             拒否されることがあります。",
            parts_ja(parts)
        ),
    }
}

/// Japanese rendering of one cherry_revert title.
pub fn title_ja(title: &CherryRevertTitle) -> String {
    match title {
        CherryRevertTitle::CherryPick {
            sha,
            summary: Some(summary),
            branch,
        } => format!("{} へ {} '{}' を cherry-pick", branch, sha, summary),
        CherryRevertTitle::CherryPick {
            sha,
            summary: None,
            branch,
        } => format!("{} へ {} を cherry-pick", branch, sha),
        CherryRevertTitle::Revert {
            sha,
            summary,
            branch,
        } => format!("{} で {} '{}' を revert", branch, sha, summary),
    }
}

/// Japanese rendering of one cherry_revert recovery block.
pub fn recovery_ja(recovery: &CherryRevertRecovery) -> String {
    match recovery {
        CherryRevertRecovery::AfterCherryPick => {
            "実行後に cherry-pick を取り消したい場合は次を使用してください:\n  git revert \
             <new-commit-sha>\n以前の HEAD の sha は reflog に記録されています:\n  git reflog"
                .to_string()
        }
        CherryRevertRecovery::AfterRevert => {
            "実行後にこの revert を取り消したい場合は、新しく作成された revert commit をさらに \
             revert してください:\n  git revert <new-revert-commit-sha>\n以前の HEAD の sha は \
             reflog に記録されています:\n  git reflog"
                .to_string()
        }
    }
}
