//! JA strings for `SyncNote`/`SyncTitle`/`SyncRecovery` (sync-to-remote, #536).

use kagi_domain::plan_note::{SyncNote, SyncRecovery, SyncTitle};

/// Japanese rendering of one sync note.
pub fn note_ja(note: &SyncNote) -> String {
    match note {
        SyncNote::BranchMissing { branch } => format!("branch `{}` が存在しません。", branch),
        SyncNote::NoUpstream { branch } => format!(
            "`{}` に upstream がありません。先に Push and create upstream か Set upstream で設定してください。",
            branch
        ),
        SyncNote::UpstreamNotFetched { branch, upstream } => format!(
            "`{}` は `{}` を追跡していますが、その remote-tracking ref がローカルにありません。先に Fetch してください。",
            branch, upstream
        ),
        SyncNote::DetachedHead => {
            "HEAD が detached です。sync to remote には branch が必要です。".to_string()
        }
        SyncNote::OperationInProgress { op } => {
            format!("{} が進行中です。完了か abort してから sync してください。", op.label_ja())
        }
        SyncNote::ConflictedFiles { count } => format!(
            "{} 個のファイルに未解決の conflict があります。解決してから sync してください。",
            count
        ),
        SyncNote::CheckedOutElsewhere { branch, path } => format!(
            "`{}` は {} で checkout 中です。その worktree から sync してください。",
            branch, path
        ),
        SyncNote::AlreadyInSync { branch, upstream } => format!(
            "`{}` は既に `{}` と一致し、ローカルの変更もありません。何もしません。",
            branch, upstream
        ),
        SyncNote::AbandonsCommits { branch, count } => format!(
            "`{}` の {} 個の commit は upstream にありません。現在の先端は refs/kagi/backups/ に保持され、git update-ref で復元できます。",
            branch, count
        ),
        SyncNote::PreservesWork {
            staged,
            unstaged,
            untracked,
        } => format!(
            "ローカルの変更（staged {}、unstaged {}、untracked {}）は stash 形式の commit として refs/kagi/backups/ に保持され（復元: git stash apply --index <ref>）、その後作業ツリーから取り除かれます。",
            staged, unstaged, untracked
        ),
        SyncNote::KeepsIgnored => "ignored ファイルには触れません。remote のファイルが ignored ファイルを上書きする場合、何も動かす前に sync を中止します。".to_string(),
    }
}

/// Japanese rendering of one sync title.
pub fn title_ja(title: &SyncTitle) -> String {
    match title {
        SyncTitle::SyncToRemote {
            branch,
            upstream,
            to,
        } => format!("`{}` を `{}`（{}）に揃える", branch, upstream, to),
    }
}

/// Japanese rendering of one sync recovery block.
pub fn recovery_ja(recovery: &SyncRecovery) -> String {
    match recovery {
        SyncRecovery::SyncToRemote {
            branch,
            from,
            tip_backup,
            work_backup,
        } => {
            let mut s = format!(
                "sync 前の `{branch}` の先端（{}）を復元:\n  git update-ref refs/heads/{branch} {tip_backup}",
                &from[..from.len().min(7)]
            );
            if let Some(work) = work_backup {
                s.push_str(&format!(
                    "\nindex と作業ツリー（staged / unstaged / untracked）を復元:\n  git stash apply --index {work}"
                ));
            }
            s.push_str("\nどちらの ref も、この entry を forget するまで残ります。");
            s
        }
    }
}
