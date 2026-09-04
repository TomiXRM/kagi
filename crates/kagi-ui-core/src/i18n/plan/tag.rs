//! JA strings for `TagNote`/`TagTitle`/`TagRecovery` (create-tag-here).

use kagi_domain::plan_note::tag::TagNameError;
use kagi_domain::plan_note::{TagNote, TagRecovery, TagTitle};

/// Japanese rendering of one tag-name validation error.
fn name_error_ja(err: &TagNameError) -> String {
    match err {
        TagNameError::Empty => "tag 名を入力してください。".to_string(),
        TagNameError::InvalidRef(name) => {
            format!("有効な tag 名ではありません。\ntag `{}`", name)
        }
        TagNameError::LeadingDash(name) => format!(
            "tag 名が '-' で始まっており、フラグと誤認される可能性があります。\ntag `{}`",
            name
        ),
        TagNameError::Exists(name) => format!("tag はすでに存在します。\ntag `{}`", name),
    }
}

/// Japanese rendering of one tag note.
pub fn note_ja(note: &TagNote) -> String {
    match note {
        TagNote::NameError(e) => name_error_ja(e),
        TagNote::CommitMissing { sha } => {
            format!("commit がこのリポジトリにありません。\ncommit `{}`", sha)
        }
        TagNote::NotFound { name } => format!("ローカルに tag がありません。\ntag `{}`", name),
        TagNote::NoRemote => "remote が未設定で、tag の push 先がありません。".to_string(),
        TagNote::PushRemoteSideEffect { remote, name } => format!(
            "tag を {} に公開します。他の tag 操作と違い、このマシンの外に出て他の人から見えます。\ntag `{}`",
            remote, name
        ),
        TagNote::PushRejectedIfMoved { name } => format!(
            "remote 側で別の commit を指して既に存在する場合、push は拒否されます（移動しません）。kagi は tag を force push しません。\ntag `{}`",
            name
        ),
    }
}

/// Japanese rendering of one tag title.
pub fn title_ja(title: &TagTitle) -> String {
    match title {
        TagTitle::CreateTag { name, at } => format!("tag `{}` を {} に作成", name, at),
        TagTitle::PushTag { name, remote } => format!("tag `{}` を `{}` に push", name, remote),
    }
}

/// Japanese rendering of one tag recovery block.
pub fn recovery_ja(recovery: &TagRecovery) -> String {
    match recovery {
        TagRecovery::CreateTag { name } => format!(
            "tag `{}` は副作用なく削除できます:\n  git tag -d {}\n作成は HEAD も作業ツリーも変更しません。",
            name, name
        ),
        TagRecovery::PushTag { name, remote } => format!(
            "remote から tag を削除できます:\n  git push {} --delete {}\nただし他の人が fetch する前に限ります。pull 済みの tag は取り消せません。",
            remote, name
        ),
    }
}
