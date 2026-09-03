//! JA strings for the GitHub-ruleset plan notes (#346, ADR-0150).

use kagi_domain::plan_note::{RuleField, RulesetNote};

/// Japanese subject noun for a pattern-rule field.
fn field_ja(field: RuleField) -> &'static str {
    match field {
        RuleField::CommitMessage => "commit メッセージ",
        RuleField::AuthorEmail => "commit の author email",
        RuleField::CommitterEmail => "committer email",
        RuleField::BranchName => "ブランチ名",
    }
}

/// Japanese rendering of one ruleset finding.
pub fn note_ja(note: &RulesetNote) -> String {
    match note {
        RulesetNote::PatternViolation { field, requirement } => format!(
            "{}が ruleset の条件を満たしていません: {}",
            field_ja(*field),
            requirement
        ),
        RulesetNote::PatternUncheckable { field, pattern } => format!(
            "{}は正規表現 ruleset (/{}/) で制限されており、ローカルでは検証できません。push 時に GitHub が判定します。",
            field_ja(*field),
            pattern
        ),
        RulesetNote::FileTooLarge { path, size, limit } => format!(
            "{} ({}) が ruleset の最大ファイルサイズ ({}) を超えています。push は拒否されます。",
            path, size, limit
        ),
        RulesetNote::RestrictedExtension { path, ext } => {
            format!("{} は禁止された拡張子 (.{}) です。ruleset が許可していません。", path, ext)
        }
        RulesetNote::RestrictedPath { path, pattern } => format!(
            "{} は禁止されたパスパターン ({}) に一致します。ruleset が許可していません。",
            path, pattern
        ),
        RulesetNote::PathTooLong { path, len, limit } => format!(
            "{} のパス長は {} 文字で、ruleset の上限 {} を超えています。",
            path, len, limit
        ),
        RulesetNote::SignatureRequired => "ruleset は署名付き commit を要求していますが、\
             commit 署名が設定されていません (commit.gpgsign / user.signingkey)。"
            .to_string(),
        RulesetNote::LinearHistoryRequired => "ruleset は linear history を要求しています。\
             merge コミットは push 時に拒否されます。"
            .to_string(),
        RulesetNote::NonFastForward => {
            "ruleset はこのブランチへの non-fast-forward 更新を禁止しています。".to_string()
        }
        RulesetNote::CreationBlocked => {
            "ruleset はこのブランチの作成を禁止しています。".to_string()
        }
        RulesetNote::UpdateBlocked => {
            "ruleset はこのブランチの更新を禁止しています。".to_string()
        }
        RulesetNote::DeletionBlocked => {
            "ruleset はこのブランチの削除を禁止しています。".to_string()
        }
        RulesetNote::ConstraintsUnknown => "ブランチの ruleset を確定できませんでした。\
             ルールが無いとは仮定せず、従来の保守的なフローを維持します。"
            .to_string(),
    }
}
