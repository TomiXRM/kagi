//! JA strings for `RemoteBranchNote`/`RemoteBranchTitle`/`RemoteBranchRecovery`
//! (delete-remote-branch).

use kagi_domain::plan_note::{RemoteBranchNote, RemoteBranchRecovery, RemoteBranchTitle};

use crate::i18n::Msg;

pub(crate) const ADVICE_REMOTE_BRANCH_LOCAL_BRANCH_UNTOUCHED: &str =
    "削除するのは remote 上の branch だけです。local branch は残り、upstream 未設定になります。\nbranch `{}`";

/// Japanese rendering of one remote-branch note.
pub fn note_ja(note: &RemoteBranchNote) -> String {
    match note {
        RemoteBranchNote::NotFound { remote, branch } => format!(
            "remote-tracking branch がローカルに見つかりません。削除済みか未 fetch の可能性があります。\nbranch `{}/{}`",
            remote, branch
        ),
        RemoteBranchNote::LocalBranchUntouched { local_name } => super::advice_text(
            Msg::AdviceRemoteBranchLocalBranchUntouched,
            &[local_name],
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
