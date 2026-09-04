//! JA strings for `SwitchNote` (ADR-0129 appendix §B-2 — switch op family).

use kagi_domain::plan_note::{SwitchNote, SwitchRecovery, SwitchTitle};

/// Japanese rendering of one switch note.
pub fn note_ja(note: &SwitchNote) -> String {
    match note {
        SwitchNote::LocalNameEmpty => "local branch 名が空です。".to_string(),
        SwitchNote::LocalExists { name } => {
            format!("local branch はすでに存在します。\nbranch `{}`", name)
        }
        SwitchNote::NameEmpty => "branch 名が空です。".to_string(),
        SwitchNote::NoUpstreamToSwitch => {
            "切り替え先の upstream/remote branch がありません。".to_string()
        }
        SwitchNote::WillCreateTracking { name, remote } => format!(
            "local branch がないため、{} を追跡して新規作成します。\nbranch `{}`",
            remote, name
        ),
        SwitchNote::FfLocalKnowledge { behind } => format!(
            "{} commit 分 fast-forward します(ローカル情報での判定、fetch 後に再確認)。",
            behind
        ),
        SwitchNote::AheadSwitchOnly {
            name,
            ahead,
            remote,
        } => format!(
            "{} に対して {} commit 進んでいます。切り替えのみ行い、更新はしません。\nbranch `{}`",
            remote, ahead, name
        ),
        SwitchNote::DivergedSwitchOnly {
            name,
            remote,
            ahead,
            behind,
        } => format!(
            "{} から分岐しています({} commit 進み、{} commit 遅れ)。切り替えのみ行います。統合するには merge か rebase してください。\nbranch `{}`",
            remote, ahead, behind, name
        ),
    }
}

/// Japanese rendering of one switch title.
pub fn title_ja(title: &SwitchTitle) -> String {
    match title {
        SwitchTitle::CheckoutTracking { remote, local } => {
            format!("`{}` を local branch `{}` として checkout", remote, local)
        }
        SwitchTitle::SwitchToLatest { branch, remote } => {
            format!("`{}` の最新に切り替え(fetch: {})", branch, remote)
        }
    }
}

/// Japanese rendering of one switch recovery block.
pub fn recovery_ja(recovery: &SwitchRecovery) -> String {
    match recovery {
        SwitchRecovery::CheckoutTracking { local } => format!(
            "この branch が不要なら、元に戻してから削除:\n  git checkout -\n  git branch -d {}",
            local
        ),
        SwitchRecovery::SwitchToLatest { remote, branch } => format!(
            "{} を fetch して {} に切り替え、安全な場合のみ fast-forward します。分岐・先行している場合は切り替えのみです。\n元に戻す:\n  git checkout -",
            remote, branch
        ),
    }
}
