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
        TagNote::NotFound { name } => format!("ローカルにタグ '{}' はありません。", name),
        TagNote::NoRemote => {
            "このリポジトリにはリモートが設定されていないため、タグの push 先がありません。"
                .to_string()
        }
        TagNote::PushRemoteSideEffect { remote, name } => format!(
            "タグ '{}' を '{}' に公開します。kagi の他のタグ操作と違い、これはこのマシンの外に出て他の人からも見えるようになります。",
            name, remote
        ),
        TagNote::PushRejectedIfMoved { name } => format!(
            "リモート側に '{}' が別のコミットを指して既に存在する場合、リモートは push を拒否します(移動はしません)。kagi はタグを force push しません。",
            name
        ),
    }
}

/// Japanese rendering of one tag title.
pub fn title_ja(title: &TagTitle) -> String {
    match title {
        TagTitle::CreateTag { name, at } => format!("タグ '{}' を {} に作成", name, at),
        TagTitle::PushTag { name, remote } => format!("タグ '{}' を '{}' に push", name, remote),
    }
}

/// Japanese rendering of one tag recovery block.
pub fn recovery_ja(recovery: &TagRecovery) -> String {
    match recovery {
        TagRecovery::CreateTag { name } => format!(
            "新しいタグ '{}' は副作用なく削除できます:\n  git tag -d {}\n(タグの作成は HEAD を移動せず、作業ツリーも変更しません。)",
            name, name
        ),
        TagRecovery::PushTag { name, remote } => format!(
            "リモートからタグを削除できます:\n  git push {} --delete {}\nただし他の人が fetch する前に限ります。公開済みで既に pull されたタグは取り消せません。",
            remote, name
        ),
    }
}
