//! JA strings for the GitHub-ruleset plan notes (#346, ADR-0150).

use kagi_domain::plan_note::{RuleField, RulesetNote};

/// Japanese subject noun for a pattern-rule field.
fn field_ja(field: RuleField) -> &'static str {
    match field {
        RuleField::CommitMessage => "commit メッセージ",
        RuleField::AuthorEmail => "commit の author email",
        RuleField::CommitterEmail => "committer email",
        RuleField::BranchName => "branch 名",
    }
}

/// Japanese rendering of one ruleset finding.
pub fn note_ja(note: &RulesetNote) -> String {
    match note {
        RulesetNote::PatternViolation { field, requirement } => format!(
            "{}が ruleset の条件を満たしません: {}",
            field_ja(*field),
            requirement
        ),
        RulesetNote::PatternUncheckable { field, pattern } => format!(
            "{}は正規表現 ruleset で制限され、ローカルでは検証できません。push 時に GitHub が判定します。\npattern /{}/",
            field_ja(*field),
            pattern
        ),
        RulesetNote::FileTooLarge { path, size, limit } => format!(
            "ruleset の最大ファイルサイズ {} を超えています。push は拒否されます。\nfile `{}` ({})",
            limit, path, size
        ),
        RulesetNote::RestrictedExtension { path, ext } => {
            format!("ruleset が禁止する拡張子 .{} です。\nfile `{}`", ext, path)
        }
        RulesetNote::RestrictedPath { path, pattern } => format!(
            "ruleset が禁止するパスパターン {} に一致します。\nfile `{}`",
            pattern, path
        ),
        RulesetNote::PathTooLong { path, len, limit } => format!(
            "パス長 {} 文字が ruleset の上限 {} を超えています。\nfile `{}`",
            len, limit, path
        ),
        RulesetNote::SignatureRequired => "ruleset は署名付き commit を要求していますが、\
             署名が未設定です（commit.gpgsign / user.signingkey）。"
            .to_string(),
        RulesetNote::LinearHistoryRequired => "ruleset は linear history を要求しています。\
             merge commit は push 時に拒否されます。"
            .to_string(),
        RulesetNote::NonFastForward => {
            "ruleset はこの branch への non-fast-forward 更新を禁止しています。".to_string()
        }
        RulesetNote::CreationBlocked => {
            "ruleset はこの branch の作成を禁止しています。".to_string()
        }
        RulesetNote::UpdateBlocked => {
            "ruleset はこの branch の更新を禁止しています。".to_string()
        }
        RulesetNote::DeletionBlocked => {
            "ruleset はこの branch の削除を禁止しています。".to_string()
        }
        RulesetNote::ConstraintsUnknown => "branch の ruleset を確定できませんでした。\
             ルール無しとは仮定せず、保守的なフローを維持します。"
            .to_string(),
    }
}
