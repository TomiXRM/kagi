//! JA strings for the commit checklist (ADR-0043 rules 4/5/6) plan notes.

use kagi_domain::plan_note::ChecklistNote;

/// Japanese rendering of one checklist finding.
pub fn note_ja(note: &ChecklistNote) -> String {
    match note {
        ChecklistNote::PossibleSecretFileStaged { path } => format!(
            "シークレットの可能性があるファイルが stage されています: {} — commit 前に確認してください。",
            path
        ),
        ChecklistNote::LargeBinaryStaged { path, size } => format!(
            "大きなバイナリファイルが stage されています: {} ({})。commit 前に確認してください。",
            path, size
        ),
        ChecklistNote::ConflictMarkerFound { path } => format!(
            "stage されたファイルにコンフリクトマーカーが残っています: {}。\
             commit 前に merge コンフリクトを解決してください。",
            path
        ),
        ChecklistNote::PossibleSecretContentStaged { path } => format!(
            "stage されたファイルの内容にシークレットの可能性があります: {} — commit 前に確認してください。",
            path
        ),
    }
}
