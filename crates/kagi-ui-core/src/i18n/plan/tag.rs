//! JA strings for `TagNote`/`TagTitle`/`TagRecovery` (create-tag-here).

use kagi_domain::plan_note::tag::TagNameError;
use kagi_domain::plan_note::{TagNote, TagRecovery, TagTitle};

/// Japanese rendering of one tag-name validation error.
fn name_error_ja(err: &TagNameError) -> String {
    match err {
        TagNameError::Empty => "タグ名を入力してください。".to_string(),
        TagNameError::InvalidRef(name) => format!("'{}' は有効なタグ名ではありません。", name),
        TagNameError::LeadingDash(name) => format!(
            "タグ名 '{}' は '-' で始まっており、コマンドラインでフラグと誤認される可能性があります。",
            name
        ),
        TagNameError::Exists(name) => format!("タグ '{}' は既に存在します。", name),
    }
}

/// Japanese rendering of one tag note.
pub fn note_ja(note: &TagNote) -> String {
    match note {
        TagNote::NameError(e) => name_error_ja(e),
        TagNote::CommitMissing { sha } => {
            format!("コミット '{}' はこのリポジトリに存在しません。", sha)
        }
    }
}

/// Japanese rendering of one tag title.
pub fn title_ja(title: &TagTitle) -> String {
    match title {
        TagTitle::CreateTag { name, at } => format!("タグ '{}' を {} に作成", name, at),
    }
}

/// Japanese rendering of one tag recovery block.
pub fn recovery_ja(recovery: &TagRecovery) -> String {
    match recovery {
        TagRecovery::CreateTag { name } => format!(
            "新しいタグ '{}' は副作用なく削除できます:\n  git tag -d {}\n(タグの作成は HEAD を移動せず、作業ツリーも変更しません。)",
            name, name
        ),
    }
}
