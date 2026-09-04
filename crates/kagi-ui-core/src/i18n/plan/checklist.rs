//! JA strings for the commit checklist (ADR-0043 rules 4/5/6) plan notes.

use kagi_domain::plan_note::ChecklistNote;

/// Japanese rendering of one checklist finding.
pub fn note_ja(note: &ChecklistNote) -> String {
    match note {
        ChecklistNote::PossibleSecretFileStaged { path } => format!(
            "シークレットの可能性があるファイルが stage されています。commit 前に確認してください。\nfile `{}`",
            path
        ),
        ChecklistNote::LargeBinaryStaged { path, size } => format!(
            "大きなバイナリが stage されています。commit 前に確認してください。\nfile `{}` ({})",
            path, size
        ),
        ChecklistNote::ConflictMarkerFound { path } => format!(
            "conflict marker が残っています。commit 前に解決してください。\nfile `{}`",
            path
        ),
        ChecklistNote::PossibleSecretContentStaged { path } => format!(
            "stage された内容にシークレットの可能性があります。commit 前に確認してください。\nfile `{}`",
            path
        ),
    }
}
