//! JA strings for `RemoteBranchNote`/`RemoteBranchTitle`/`RemoteBranchRecovery`
//! (delete-remote-branch).

use kagi_domain::plan_note::{RemoteBranchNote, RemoteBranchRecovery, RemoteBranchTitle};

/// Japanese rendering of one remote-branch note.
pub fn note_ja(note: &RemoteBranchNote) -> String {
    match note {
        RemoteBranchNote::NotFound { remote, branch } => format!(
            "remote-tracking branch がローカルに見つかりません。削除済みか未 fetch の可能性があります。\nbranch `{}/{}`",
            remote, branch
        ),
        RemoteBranchNote::LocalBranchUntouched { local_name } => format!(
            "削除するのは remote 上の branch だけです。local branch は残り、upstream 未設定になります。\nbranch `{}`",
            local_name
        ),
    }
}

/// Japanese rendering of one remote-branch title.
pub fn title_ja(title: &RemoteBranchTitle) -> String {
    match title {
        RemoteBranchTitle::DeleteRemoteBranch { remote, branch } => {
            format!("remote branch `{}/{}` を削除", remote, branch)
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
            "commit がまだ残っていれば branch を復元できます:\n  git push {remote} {sha}:refs/heads/{branch}\nそれ以外は kagi からは元に戻せません。\ncommit `{sha}`"
        ),
    }
}
