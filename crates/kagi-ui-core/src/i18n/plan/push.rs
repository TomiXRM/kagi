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
            "fast-forward できない push は失敗します(force は使用しません)。".to_string()
        }
        PushNote::NoUpstreamNoRemotes { branch } => format!(
            "ブランチ '{}' に upstream が設定されておらず、remote も存在しません。\
             `git remote add origin <url>` で remote を追加してください。",
            branch
        ),
        PushNote::NoUpstreamWithErr { branch, err } => {
            format!(
                "ブランチ '{}' に upstream が設定されていません: {}。",
                branch, err
            )
        }
        PushNote::AlreadyUpToDate { branch, .. } => format!(
            "ブランチ '{}' は upstream に対してすでに最新です — push する内容がありません。",
            branch
        ),
        PushNote::UpstreamFormatInvalid => {
            "upstream は origin/main のような remote branch 名で指定してください。".to_string()
        }
        PushNote::UpstreamNotPresentLocally { upstream } => format!(
            "remote-tracking branch '{}' はローカルに存在しませんが、設定自体は行えます。",
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
        } => format!("'{}' を '{}' へ push(upstream 設定)", branch, remote),
        PushTitle::Push { branch, remote, .. } => format!("'{}' を '{}' へ push", branch, remote),
        PushTitle::PushBlocked => "Push(ブロック中)".to_string(),
        PushTitle::PushBranch {
            branch,
            remote,
            set_upstream: true,
        } => format!(
            "'{}' を '{}/{}' へ push(upstream 設定)",
            branch, remote, branch
        ),
        PushTitle::PushBranch { branch, remote, .. } => {
            format!("'{}' を '{}' へ push", branch, remote)
        }
        PushTitle::SetUpstream { branch, upstream } => {
            format!("'{}' の upstream を '{}' に設定", branch, upstream)
        }
    }
}

/// Japanese rendering of one push recovery block.
pub fn recovery_ja(recovery: &PushRecovery) -> String {
    match recovery {
        PushRecovery::Push => {
            "push はリモートへコミットを送るだけで、ローカルリポジトリは変更されません。\n\
             push が拒否された場合(non-fast-forward)は、先に pull してから再度プランしてください:\n  \
             git pull\n  git push\n\
             reflog にはすべての HEAD 移動が記録されます:\n  git reflog"
                .to_string()
        }
        PushRecovery::PushBlocked => {
            "push にはブランチが必要です。`git checkout <branch>` で HEAD をブランチに紐付けてください。"
                .to_string()
        }
        PushRecovery::PushBranch => {
            "push はリモートへコミットを送るだけで、作業ツリーは変更しません。\
             push が拒否された場合は、先に fetch または pull してから再度プランしてください。"
                .to_string()
        }
        PushRecovery::SetUpstream { branch } => format!(
            "これは git config の branch.{}.remote と branch.{}.merge のみを変更します。\
             元に戻すには、以前の upstream を再度設定してください。",
            branch, branch
        ),
    }
}
