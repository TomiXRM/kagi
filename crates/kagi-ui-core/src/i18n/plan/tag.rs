//! JA strings for `TagNote`/`TagTitle`/`TagRecovery` (create-tag-here).

use kagi_domain::plan_note::tag::TagNameError;
use kagi_domain::plan_note::{TagNote, TagRecovery, TagTitle};

/// Japanese rendering of one tag-name validation error.
fn name_error_ja(err: &TagNameError) -> String {
    match err {
        TagNameError::Empty => "tag 名を入力してください。".to_string(),
        TagNameError::InvalidRef(name) => format!("'{}' は有効な tag 名ではありません。", name),
        TagNameError::LeadingDash(name) => format!(
            "tag 名 '{}' は '-' で始まっており、コマンドラインでフラグと誤認される可能性があります。",
            name
        ),
        TagNameError::Exists(name) => format!("tag '{}' は既に存在します。", name),
    }
}

/// Japanese rendering of one tag note.
pub fn note_ja(note: &TagNote) -> String {
    match note {
        TagNote::NameError(e) => name_error_ja(e),
        TagNote::CommitMissing { sha } => {
            format!("commit '{}' はこのリポジトリに存在しません。", sha)
        }
        TagNote::NotFound { name } => format!("ローカルに tag '{}' はありません。", name),
        TagNote::NoRemote => {
            "このリポジトリにはリモートが設定されていないため、tag の push 先がありません。"
                .to_string()
        }
        TagNote::PushRemoteSideEffect { remote, name } => format!(
            "tag '{}' を '{}' に公開します。kagi の他の tag 操作と違い、これはこのマシンの外に出て他の人からも見えるようになります。",
            name, remote
        ),
        TagNote::PushRejectedIfMoved { name } => format!(
            "リモート側に '{}' が別の commit を指して既に存在する場合、リモートは push を拒否します(移動はしません)。kagi は tag を force push しません。",
            name
        ),
    }
}

/// Japanese rendering of one tag title.
pub fn title_ja(title: &TagTitle) -> String {
    match title {
        TagTitle::CreateTag { name, at } => format!("tag '{}' を {} に作成", name, at),
        TagTitle::PushTag { name, remote } => format!("tag '{}' を '{}' に push", name, remote),
    }
}

/// Japanese rendering of one tag recovery block.
pub fn recovery_ja(recovery: &TagRecovery) -> String {
    match recovery {
        TagRecovery::CreateTag { name } => format!(
            "新しい tag '{}' は副作用なく削除できます:\n  git tag -d {}\n(tag の作成は HEAD を移動せず、作業ツリーも変更しません。)",
            name, name
        ),
        TagRecovery::PushTag { name, remote } => format!(
            "リモートから tag を削除できます:\n  git push {} --delete {}\nただし他の人が fetch する前に限ります。公開済みで既に pull された tag は取り消せません。",
            remote, name
        ),
    }
}
