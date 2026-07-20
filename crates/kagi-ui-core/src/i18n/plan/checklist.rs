//! JA strings for the commit checklist (ADR-0043 rules 4/5/6) plan notes.

use kagi_domain::plan_note::ChecklistNote;

/// Japanese rendering of one checklist finding.
pub fn note_ja(note: &ChecklistNote) -> String {
    match note {
        ChecklistNote::PossibleSecretFileStaged { path } => format!(
            "シークレットの可能性があるファイルがステージされています: {} — コミット前に確認してください。",
            path
        ),
        ChecklistNote::LargeBinaryStaged { path, size } => format!(
            "大きなバイナリファイルがステージされています: {} ({})。コミット前に確認してください。",
            path, size
        ),
        ChecklistNote::ConflictMarkerFound { path } => format!(
            "ステージされたファイルにコンフリクトマーカーが残っています: {}。\
             コミット前にマージコンフリクトを解決してください。",
            path
        ),
        ChecklistNote::PossibleSecretContentStaged { path } => format!(
            "ステージされたファイルの内容にシークレットの可能性があります: {} — コミット前に確認してください。",
            path
        ),
    }
}
