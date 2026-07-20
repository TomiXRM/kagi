//! JA strings for `SwitchNote` (ADR-0129 appendix §B-2 — switch op family).

use kagi_domain::plan_note::{SwitchNote, SwitchRecovery, SwitchTitle};

/// Japanese rendering of one switch note.
pub fn note_ja(note: &SwitchNote) -> String {
    match note {
        SwitchNote::LocalNameEmpty => "ローカルブランチ名が空です。".to_string(),
        SwitchNote::LocalExists { name } => {
            format!("ローカルブランチ '{}' はすでに存在します。", name)
        }
        SwitchNote::NameEmpty => "ブランチ名が空です。".to_string(),
        SwitchNote::NoUpstreamToSwitch => {
            "切り替え先の upstream/リモートブランチがありません。".to_string()
        }
        SwitchNote::WillCreateTracking { name, remote } => format!(
            "ローカルブランチ '{}' は存在しないため、{} を追跡する形で新規作成されます。",
            name, remote
        ),
        SwitchNote::FfLocalKnowledge { behind } => format!(
            "{} コミット分 fast-forward します(ローカル情報に基づく判定・fetch 後に再確認されます)。",
            behind
        ),
        SwitchNote::AheadSwitchOnly {
            name,
            ahead,
            remote,
        } => format!(
            "'{}' は {} に対して {} コミット進んでいます。切り替えのみ行い、更新はしません。",
            name, remote, ahead
        ),
        SwitchNote::DivergedSwitchOnly {
            name,
            remote,
            ahead,
            behind,
        } => format!(
            "'{}' は {} から分岐しています({} コミット進み、{} コミット遅れ)。切り替えのみ行います \
             — 統合するには merge か rebase をしてください。",
            name, remote, ahead, behind
        ),
    }
}

/// Japanese rendering of one switch title.
pub fn title_ja(title: &SwitchTitle) -> String {
    match title {
        SwitchTitle::CheckoutTracking { remote, local } => {
            format!(
                "{} をローカルブランチ {} としてチェックアウト",
                remote, local
            )
        }
        SwitchTitle::SwitchToLatest { branch, remote } => {
            format!("{} の最新版に切り替え(fetch: {})", branch, remote)
        }
    }
}

/// Japanese rendering of one switch recovery block.
pub fn recovery_ja(recovery: &SwitchRecovery) -> String {
    match recovery {
        SwitchRecovery::CheckoutTracking { local } => format!(
            "チェックアウトは成功したがこのブランチが不要な場合は、元のブランチに戻ってから削除してください:\n  git checkout -\n  git branch -d {}",
            local
        ),
        SwitchRecovery::SwitchToLatest { remote, branch } => format!(
            "{} を fetch したうえで {} に切り替え、安全な場合のみ fast-forward します。\
             分岐済み・進んでいるブランチは切り替えのみ行われ、移動はしません。\
             元に戻るには: git checkout -",
            remote, branch
        ),
    }
}
