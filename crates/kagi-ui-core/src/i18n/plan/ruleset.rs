//! JA strings for the GitHub-ruleset plan notes (#346, ADR-0150).

use kagi_domain::plan_note::{RuleField, RulesetNote};

use crate::i18n::Msg;

/// JA template for `Msg::AdviceRulesetPatternViolation`.
pub(crate) const ADVICE_RULESET_PATTERN_VIOLATION: &str = "{}が ruleset の条件を満たしません: {}";

/// JA template for `Msg::AdviceRulesetPatternUncheckable`.
pub(crate) const ADVICE_RULESET_PATTERN_UNCHECKABLE: &str =
    "{}は正規表現 ruleset で制限され、ローカルでは検証できません。push 時に GitHub が判定します。\npattern /{}/";

/// JA template for `Msg::AdviceRulesetFileTooLarge`.
pub(crate) const ADVICE_RULESET_FILE_TOO_LARGE: &str =
    "ruleset の最大ファイルサイズ {} を超えています。push は拒否されます。\nfile `{}` ({})";

/// JA template for `Msg::AdviceRulesetRestrictedExtension`.
pub(crate) const ADVICE_RULESET_RESTRICTED_EXTENSION: &str =
    "ruleset が禁止する拡張子 .{} です。\nfile `{}`";

/// JA template for `Msg::AdviceRulesetRestrictedPath`.
pub(crate) const ADVICE_RULESET_RESTRICTED_PATH: &str =
    "ruleset が禁止するパスパターン {} に一致します。\nfile `{}`";

/// JA template for `Msg::AdviceRulesetPathTooLong`.
pub(crate) const ADVICE_RULESET_PATH_TOO_LONG: &str =
    "パス長 {} 文字が ruleset の上限 {} を超えています。\nfile `{}`";

/// JA text for `Msg::AdviceRulesetSignatureRequired`.
pub(crate) const ADVICE_RULESET_SIGNATURE_REQUIRED: &str =
    "ruleset は署名付き commit を要求していますが、署名が未設定です(commit.gpgsign / user.signingkey)。";

/// JA text for `Msg::AdviceRulesetLinearHistoryRequired`.
pub(crate) const ADVICE_RULESET_LINEAR_HISTORY_REQUIRED: &str =
    "ruleset は linear history を要求しています。merge commit は push 時に拒否されます。";

/// JA text for `Msg::AdviceRulesetNonFastForward`.
pub(crate) const ADVICE_RULESET_NON_FAST_FORWARD: &str =
    "ruleset はこの branch への non-fast-forward 更新を禁止しています。";

/// JA text for `Msg::AdviceRulesetCreationBlocked`.
pub(crate) const ADVICE_RULESET_CREATION_BLOCKED: &str =
    "ruleset はこの branch の作成を禁止しています。";

/// JA text for `Msg::AdviceRulesetConstraintsUnknown`.
pub(crate) const ADVICE_RULESET_CONSTRAINTS_UNKNOWN: &str =
    "branch の ruleset を確定できませんでした。ルール無しとは仮定せず、保守的なフローを維持します。";

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
        RulesetNote::PatternViolation { field, requirement } => super::advice_text(
            Msg::AdviceRulesetPatternViolation,
            &[&field_ja(*field), requirement],
        ),
        RulesetNote::PatternUncheckable { field, pattern } => super::advice_text(
            Msg::AdviceRulesetPatternUncheckable,
            &[&field_ja(*field), pattern],
        ),
        RulesetNote::FileTooLarge { path, size, limit } => {
            super::advice_text(Msg::AdviceRulesetFileTooLarge, &[limit, path, size])
        }
        RulesetNote::RestrictedExtension { path, ext } => {
            super::advice_text(Msg::AdviceRulesetRestrictedExtension, &[ext, path])
        }
        RulesetNote::RestrictedPath { path, pattern } => {
            super::advice_text(Msg::AdviceRulesetRestrictedPath, &[pattern, path])
        }
        RulesetNote::PathTooLong { path, len, limit } => {
            super::advice_text(Msg::AdviceRulesetPathTooLong, &[len, limit, path])
        }
        RulesetNote::SignatureRequired => {
            super::advice_text(Msg::AdviceRulesetSignatureRequired, &[])
        }
        RulesetNote::LinearHistoryRequired => {
            super::advice_text(Msg::AdviceRulesetLinearHistoryRequired, &[])
        }
        RulesetNote::NonFastForward => super::advice_text(Msg::AdviceRulesetNonFastForward, &[]),
        RulesetNote::CreationBlocked => super::advice_text(Msg::AdviceRulesetCreationBlocked, &[]),
        RulesetNote::UpdateBlocked => "ruleset はこの branch の更新を禁止しています。".to_string(),
        RulesetNote::DeletionBlocked => {
            "ruleset はこの branch の削除を禁止しています。".to_string()
        }
        RulesetNote::ConstraintsUnknown => {
            super::advice_text(Msg::AdviceRulesetConstraintsUnknown, &[])
        }
    }
}
