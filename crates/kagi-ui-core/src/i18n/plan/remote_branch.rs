//! JA strings for `RemoteBranchNote`/`RemoteBranchTitle`/`RemoteBranchRecovery`
//! (delete-remote-branch).

use kagi_domain::plan_note::{RemoteBranchNote, RemoteBranchRecovery, RemoteBranchTitle};

/// Japanese rendering of one remote-branch note.
pub fn note_ja(note: &RemoteBranchNote) -> String {
    match note {
        RemoteBranchNote::NotFound { remote, branch } => format!(
            "リモート追跡ブランチ '{}/{}' がローカルに見つかりませんでした。既に削除されているか、まだフェッチされていない可能性があります。",
            remote, branch
        ),
        RemoteBranchNote::LocalBranchUntouched { local_name } => format!(
            "この操作はリモート上のブランチのみを削除します。ローカルブランチ '{}' には影響せず、アップストリーム未設定の状態になります。",
            local_name
        ),
    }
}

/// Japanese rendering of one remote-branch title.
pub fn title_ja(title: &RemoteBranchTitle) -> String {
    match title {
        RemoteBranchTitle::DeleteRemoteBranch { remote, branch } => {
            format!("リモートブランチ '{}/{}' を削除", remote, branch)
        }
    }
}

/// Japanese rendering of one remote-branch recovery block.
pub fn recovery_ja(recovery: &RemoteBranchRecovery) -> String {
    match recovery {
        RemoteBranchRecovery::DeleteRemoteBranch {
            remote,
            branch,
            sha,
        } => format!(
            "コミット '{sha}' がまだ存在する場合(ローカル、またはリモートのreflogがGCされる前なら)、ブランチを復元できます:\n  git push {remote} {sha}:refs/heads/{branch}\nそれ以外の場合、kagiからは元に戻せません。"
        ),
    }
}
