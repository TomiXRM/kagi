//! JA strings for `PushNote` / `PushTitle` / `PushRecovery` (ADR-0129
//! appendix §B-5, `plan_push` / `plan_push_branch` / `plan_set_upstream`).
//!
//! The EN side carries two punctuation-twin templates (`PushPunct`) that
//! exist only to keep the byte-identical legacy strings apart — Japanese has
//! no such ambiguity, so both puncts render the same JA sentence here.

use kagi_domain::plan_note::{PushNote, PushRecovery, PushTitle};

/// Japanese rendering of one push note.
pub fn note_ja(note: &PushNote) -> String {
    match note {
        PushNote::NoForceUsed { .. } => {
            "fast-forward できない push は失敗します。force は使いません。".to_string()
        }
        PushNote::NoUpstreamNoRemotes { branch } => format!(
            "upstream が未設定で、remote もありません。remote を追加してください。\nbranch `{}`\n  git remote add origin <url>",
            branch
        ),
        PushNote::NoUpstreamWithErr { branch, err } => {
            format!("upstream が未設定です: {}\nbranch `{}`", err, branch)
        }
        PushNote::AlreadyUpToDate { branch, .. } => format!(
            "upstream に対してすでに最新です。push する内容はありません。\nbranch `{}`",
            branch
        ),
        PushNote::UpstreamFormatInvalid => {
            "upstream は origin/main のような remote branch 名で指定してください。".to_string()
        }
        PushNote::UpstreamNotPresentLocally { upstream } => format!(
            "この remote-tracking branch はローカルにありませんが、設定はできます。\nbranch `{}`",
            upstream
        ),
    }
}

/// Japanese rendering of one push title.
pub fn title_ja(title: &PushTitle) -> String {
    match title {
        PushTitle::Push {
            branch,
            remote,
            set_upstream: true,
        } => format!("`{}` を `{}` へ push(upstream 設定)", branch, remote),
        PushTitle::Push { branch, remote, .. } => format!("`{}` を `{}` へ push", branch, remote),
        PushTitle::PushBlocked => "push(ブロック中)".to_string(),
        PushTitle::PushBranch {
            branch,
            remote,
            set_upstream: true,
        } => format!(
            "`{}` を `{}/{}` へ push(upstream 設定)",
            branch, remote, branch
        ),
        PushTitle::PushBranch { branch, remote, .. } => {
            format!("`{}` を `{}` へ push", branch, remote)
        }
        PushTitle::SetUpstream { branch, upstream } => {
            format!("`{}` の upstream を `{}` に設定", branch, upstream)
        }
    }
}

/// Japanese rendering of one push recovery block.
pub fn recovery_ja(recovery: &PushRecovery) -> String {
    match recovery {
        PushRecovery::Push => "push は remote へ commit を送るだけで、ローカルは変更しません。\n\
             拒否された場合(non-fast-forward)は先に pull してから再度プラン:\n  \
             git pull\n  git push\nHEAD の移動は reflog に記録されます:\n  git reflog"
            .to_string(),
        PushRecovery::PushBlocked => {
            "push には branch が必要です。\n  git checkout <branch>".to_string()
        }
        PushRecovery::PushBranch => {
            "push は remote へ commit を送るだけで、作業ツリーは変更しません。\
             拒否された場合は先に fetch か pull してから再度プランしてください。"
                .to_string()
        }
        PushRecovery::SetUpstream { branch } => format!(
            "変更するのは git config の branch.{}.remote と branch.{}.merge だけです。\
             元に戻すには以前の upstream を再設定してください。",
            branch, branch
        ),
    }
}
