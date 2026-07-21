//! JA strings for `ForceLeaseNote`/`ForceLeaseTitle`/`ForceLeaseRecovery`
//! (force-with-lease push).

use kagi_domain::plan_note::{ForceLeaseNote, ForceLeaseRecovery, ForceLeaseTitle};

/// Japanese rendering of one force-lease note.
pub fn note_ja(note: &ForceLeaseNote) -> String {
    match note {
        ForceLeaseNote::NoUpstream { branch } => format!(
            "ブランチ '{}' にはアップストリームが設定されていません。force-with-lease にはリース対象となるリモートの既知の先端が必要です。",
            branch
        ),
        ForceLeaseNote::NothingToPush { branch } => format!(
            "ブランチ '{}' は既にリモート追跡refと一致しています。force-push する内容がありません。",
            branch
        ),
        ForceLeaseNote::RewritesRemoteHistory { branch } => format!(
            "リモートブランチ '{}' の履歴が上書きされます。既に古い履歴をpullした人は調整(古いtipへのrebase等)が必要になります。",
            branch
        ),
        ForceLeaseNote::LeaseValue { remote, sha } => format!(
            "リースにより保護されています: 最後にフェッチした時点から '{}' が {} より先に進んでいた場合(=他の誰かがその間にpushしていた場合)、このpushは拒否されます。",
            remote, sha
        ),
    }
}

/// Japanese rendering of one force-lease title.
pub fn title_ja(title: &ForceLeaseTitle) -> String {
    match title {
        ForceLeaseTitle::ForceLeasePush { branch, remote } => {
            format!("'{}' を '{}' へ force-with-lease push", branch, remote)
        }
    }
}

/// Japanese rendering of one force-lease recovery block.
pub fn recovery_ja(recovery: &ForceLeaseRecovery) -> String {
    match recovery {
        ForceLeaseRecovery::ForceLeasePush {
            branch,
            remote,
            previous_remote_sha,
            new_sha,
        } => format!(
            "リモートの以前の先端は '{previous_remote_sha}' でした。復元するには(この push に対してもリースで保護されます):\n  git push --force-with-lease={branch}:{new_sha} {remote} {previous_remote_sha}:refs/heads/{branch}\n書き換えられた履歴をpull済みの人は、それぞれローカルで調整が必要です。"
        ),
    }
}
