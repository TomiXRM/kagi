//! JA strings for the commit checklist (ADR-0043 rules 4/5/6) plan notes.

use kagi_domain::plan_note::ChecklistNote;

use crate::i18n::Msg;

/// JA template for `Msg::AdviceChecklistPossibleSecretFileStaged`.
pub(crate) const ADVICE_CHECKLIST_POSSIBLE_SECRET_FILE_STAGED: &str =
    "シークレットの可能性があるファイルが stage されています。commit 前に確認してください。\nfile `{}`";

/// JA template for `Msg::AdviceChecklistLargeBinaryStaged`.
pub(crate) const ADVICE_CHECKLIST_LARGE_BINARY_STAGED: &str =
    "大きなバイナリが stage されています。commit 前に確認してください。\nfile `{}` ({})";

/// JA template for `Msg::AdviceChecklistConflictMarkerFound`.
pub(crate) const ADVICE_CHECKLIST_CONFLICT_MARKER_FOUND: &str =
    "conflict marker が残っています。commit 前に解決してください。\nfile `{}`";

/// JA template for `Msg::AdviceChecklistPossibleSecretContentStaged`.
pub(crate) const ADVICE_CHECKLIST_POSSIBLE_SECRET_CONTENT_STAGED: &str =
    "stage された内容にシークレットの可能性があります。commit 前に確認してください。\nfile `{}`";

/// Japanese rendering of one checklist finding.
pub fn note_ja(note: &ChecklistNote) -> String {
    match note {
        ChecklistNote::PossibleSecretFileStaged { path } => {
            super::advice_text(Msg::AdviceChecklistPossibleSecretFileStaged, &[path])
        }
        ChecklistNote::LargeBinaryStaged { path, size } => {
            super::advice_text(Msg::AdviceChecklistLargeBinaryStaged, &[path, size])
        }
        ChecklistNote::ConflictMarkerFound { path } => {
            super::advice_text(Msg::AdviceChecklistConflictMarkerFound, &[path])
        }
        ChecklistNote::PossibleSecretContentStaged { path } => {
            super::advice_text(Msg::AdviceChecklistPossibleSecretContentStaged, &[path])
        }
    }
}
