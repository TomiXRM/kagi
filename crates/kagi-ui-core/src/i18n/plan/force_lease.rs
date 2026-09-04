//! JA strings for `ForceLeaseNote`/`ForceLeaseTitle`/`ForceLeaseRecovery`
//! (force-with-lease push).

use kagi_domain::plan_note::{ForceLeaseNote, ForceLeaseRecovery, ForceLeaseTitle};

/// Japanese rendering of one force-lease note.
pub fn note_ja(note: &ForceLeaseNote) -> String {
    match note {
        ForceLeaseNote::NoUpstream { branch } => format!(
            "upstream が設定されていません。force-with-lease には remote の既知の先端が必要です。\nbranch `{}`",
            branch
        ),
        ForceLeaseNote::NothingToPush { branch } => format!(
            "remote 追跡 ref と一致しています。push する内容がありません。\nbranch `{}`",
            branch
        ),
        ForceLeaseNote::RewritesRemoteHistory { branch } => format!(
            "remote branch の履歴を上書きします。古い履歴を pull した人は rebase 等の調整が必要です。\nbranch `{}`",
            branch
        ),
        ForceLeaseNote::LeaseValue { remote, sha } => format!(
            "lease で保護されます。最後の fetch 以降に `{}` が {} より先へ進んでいれば(誰かが push していれば)push は拒否されます。",
            remote, sha
        ),
    }
}

/// Japanese rendering of one force-lease title.
pub fn title_ja(title: &ForceLeaseTitle) -> String {
    match title {
        ForceLeaseTitle::ForceLeasePush { branch, remote } => {
            format!("`{}` を `{}` へ force-with-lease push", branch, remote)
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
            "remote の以前の先端は `{previous_remote_sha}`。復元するには:\n  git push --force-with-lease={branch}:{new_sha} {remote} {previous_remote_sha}:refs/heads/{branch}\n書き換えた履歴を pull 済みの人は各自調整が必要です。"
        ),
    }
}
