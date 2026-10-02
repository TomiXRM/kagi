//! JA strings for `CloneNote` / `CloneTitle` (cloning a GitHub repository,
//! #923).

use kagi_domain::plan_note::{CloneNote, CloneTitle};

/// Japanese rendering of one clone note.
pub fn note_ja(note: &CloneNote) -> String {
    match note {
        CloneNote::SourceInvalid { source } => {
            format!("`{source}` は Kagi が clone できる GitHub のリポジトリではありません。")
        }
        CloneNote::SourceWithoutHost { source } => format!(
            "`{source}` には host がありません(host/owner/repo)。どのサーバーから clone するか Kagi が確定できません。"
        ),
        CloneNote::DestinationNotAbsolute { path } => {
            format!("clone 先 `{path}` が絶対パスではありません。")
        }
        CloneNote::DestinationNotUtf8 { path } => format!(
            "clone 先 `{path}` に UTF-8 でない文字が含まれています。UTF-8 の名前の clone 先を選んでください。"
        ),
        CloneNote::DestinationNotEmpty { path } => format!(
            "`{path}` は既に存在し、空のフォルダーではありません。別の clone 先を選んでください。Kagi は上書きしません。"
        ),
        CloneNote::ParentMissing { path } => format!("フォルダー `{path}` がありません。"),
        CloneNote::DestinationUnreadable { path, error } => {
            format!("`{path}` を確認できませんでした: {error}")
        }
        CloneNote::PlanMismatch { source, path } => format!(
            "`{source}` を `{path}` へ clone する内容は確認した計画と違います。もう一度確認してください。"
        ),
        CloneNote::ForkAddsUpstream => {
            "このリポジトリは fork です。gh は fork 元を指す `upstream` remote も追加します。"
                .to_string()
        }
    }
}

/// Japanese rendering of one clone title.
pub fn title_ja(title: &CloneTitle) -> String {
    match title {
        CloneTitle::Clone { source } => format!("`{source}` を clone"),
    }
}
