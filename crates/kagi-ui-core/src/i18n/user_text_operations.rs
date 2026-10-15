//! Display-only operation summaries. Never use these strings in receipts or klog.
use super::{diagnostic, operation_label, state_text};
use crate::i18n::{lang, Lang};
use kagi_domain::{operation::OperationOutcome, plan::PullOutcome};

pub fn conflict_saved() -> &'static str {
    match lang() {
        Lang::En => "Resolution saved and staged.",
        Lang::Ja => "解決内容を保存し、stage しました。",
    }
}
pub fn completed(op: &str) -> String {
    let label = operation_label(op);
    match lang() {
        Lang::En => format!("{label}: completed."),
        Lang::Ja => format!("{label}: 完了しました。"),
    }
}

pub fn failed(op: &str, reason: &str) -> String {
    let label = operation_label(op);
    let reason = diagnostic(reason);
    match lang() {
        Lang::En => format!("{label} failed: {reason}"),
        Lang::Ja => format!("{label} に失敗しました: {reason}"),
    }
}

pub fn partial(op: &str, reason: &str) -> String {
    let label = operation_label(op);
    let reason = diagnostic(reason);
    match lang() {
        Lang::En => format!(
            "{label} was only partly completed. Check the repository before continuing: {reason}"
        ),
        Lang::Ja => format!(
            "{label} は一部のみ完了しました。続行する前に repository を確認してください: {reason}"
        ),
    }
}

pub fn unknown(op: &str, reason: &str) -> String {
    let label = operation_label(op);
    let reason = diagnostic(reason);
    match lang() {
        Lang::En => format!("{label}: the result could not be confirmed. Do not retry yet. Check the repository: {reason}"),
        Lang::Ja => format!("{label}: 結果を確認できませんでした。まだ再試行しないでください。repository を確認してください: {reason}"),
    }
}

pub fn recording_failed(repo: impl std::fmt::Display, reason: &str) -> String {
    let reason = diagnostic(reason);
    match lang() {
        Lang::En => format!("{repo}: Could not save the Operation Log: {reason}"),
        Lang::Ja => format!("{repo}: 操作ログを保存できませんでした: {reason}"),
    }
}

pub fn progress(op: &str) -> String {
    let label = operation_label(op);
    match lang() {
        Lang::En => format!("{label} in progress…"),
        Lang::Ja => format!("{label} を実行中…"),
    }
}

pub fn deleted_branch(branch: &str, remote: bool) -> String {
    match (lang(), remote) {
        (Lang::En, false) => {
            format!("Deleted branch '{branch}'. You can restore it from the Operation Log.")
        }
        (Lang::Ja, false) => {
            format!("branch '{branch}' を削除しました。操作ログから復元できます。")
        }
        (Lang::En, true) => format!("Deleted remote branch '{branch}'."),
        (Lang::Ja, true) => format!("remote branch '{branch}' を削除しました。"),
    }
}

pub fn cleanup(deleted: usize) -> String {
    match lang() {
        Lang::En => format!("Deleted {deleted} branches."),
        Lang::Ja => format!("{deleted} 件の branch を削除しました。"),
    }
}

pub fn moved(op: &str, before: &str, after: &str) -> String {
    let label = operation_label(op);
    let before = state_text(before);
    let after = state_text(after);
    match (lang(), op == "amend") {
        (Lang::En, true) => {
            format!("{label}: {before} → {after}. Restore with: git reset --soft {before}")
        }
        (Lang::Ja, true) => {
            format!("{label}: {before} → {after}。復元コマンド: git reset --soft {before}")
        }
        (Lang::En, false) => format!("{label}: {before} → {after}. Recover with git reflog."),
        (Lang::Ja, false) => format!("{label}: {before} → {after}。git reflog で復元できます。"),
    }
}

pub fn reset(target: &str) -> String {
    match lang() {
        Lang::En => format!("Current branch reset to {target}."),
        Lang::Ja => format!("現在の branch を {target} に reset しました。"),
    }
}

pub fn onto(op: &str, branch: &str) -> String {
    let label = operation_label(op);
    match lang() {
        Lang::En => format!("{label} onto '{branch}' completed."),
        Lang::Ja => format!("'{branch}' への {label} が完了しました。"),
    }
}

pub fn push_result(outcome: &OperationOutcome) -> String {
    let OperationOutcome::Push(result) = outcome else {
        return unknown("push", &diagnostic("push: unexpected outcome"));
    };
    match (lang(), result.set_upstream) {
        (Lang::En, false) => format!("Pushed {} commits.", result.pushed),
        (Lang::En, true) => format!("Pushed {} commits and set upstream.", result.pushed),
        (Lang::Ja, false) => format!("{} 件の commit を push しました。", result.pushed),
        (Lang::Ja, true) => format!(
            "{} 件の commit を push し、upstream を設定しました。",
            result.pushed
        ),
    }
}

pub fn branch_result(branch: &str, outcome: &OperationOutcome) -> String {
    match outcome {
        OperationOutcome::Pull(PullOutcome::UpToDate) => match lang() {
            Lang::En => format!("Branch '{branch}' is already up to date."),
            Lang::Ja => format!("branch '{branch}' はすでに最新です。"),
        },
        OperationOutcome::Pull(PullOutcome::FastForward { to }) => match lang() {
            Lang::En => format!("Branch '{branch}' fast-forwarded to {}.", to.short()),
            Lang::Ja => format!(
                "branch '{branch}' を {} に fast-forward しました。",
                to.short()
            ),
        },
        OperationOutcome::Push(_) => format!("{branch}: {}", push_result(outcome)),
        OperationOutcome::SyncToRemote {
            to,
            work_backup,
            removed_untracked,
            ..
        } => {
            let target = &to[..7.min(to.len())];
            match (lang(), work_backup.is_some()) {
                (Lang::En, false) => format!("Branch '{branch}' synced to {target}."),
                (Lang::Ja, false) => format!("branch '{branch}' を {target} に同期しました。"),
                (Lang::En, true) => format!("Branch '{branch}' synced to {target}. Local changes were backed up; {removed_untracked} untracked files were removed."),
                (Lang::Ja, true) => format!("branch '{branch}' を {target} に同期しました。ローカルの変更はバックアップ済みです。{removed_untracked} 件の untracked ファイルを削除しました。"),
            }
        }
        _ => unknown("branch operation", &diagnostic("unexpected outcome")),
    }
}

pub fn pull_summary(summary: &str) -> String {
    let (result, restored) = summary
        .split_once("; ")
        .map_or((summary, None), |(a, b)| (a, Some(b)));
    let shown = if result == "already up to date" {
        match lang() {
            Lang::En => "Already up to date.".to_string(),
            Lang::Ja => "すでに最新です。".to_string(),
        }
    } else if let Some(target) = result.strip_prefix("fast-forward to ") {
        match lang() {
            Lang::En => format!("Fast-forwarded to {target}."),
            Lang::Ja => format!("{target} に fast-forward しました。"),
        }
    } else if let Some(commit) = result.strip_prefix("merge commit ") {
        match lang() {
            Lang::En => format!("Created merge commit {commit}."),
            Lang::Ja => format!("merge commit {commit} を作成しました。"),
        }
    } else {
        return format!("{}: {}", operation_label("pull"), diagnostic(summary));
    };
    match restored {
        Some(restored) => format!("{shown} {}", diagnostic(restored)),
        None => shown,
    }
}

pub fn worktree_lock(name: &str, locked: bool) -> String {
    match (lang(), locked) {
        (Lang::En, true) => format!("Locked worktree '{name}'."),
        (Lang::En, false) => format!("Unlocked worktree '{name}'."),
        (Lang::Ja, true) => format!("worktree '{name}' を lock しました。"),
        (Lang::Ja, false) => format!("worktree '{name}' の lock を解除しました。"),
    }
}

pub fn pruned_worktrees(count: usize) -> String {
    match lang() {
        Lang::En => format!("Removed {count} stale worktree entries."),
        Lang::Ja => format!("{count} 件の使われなくなった worktree の登録を削除しました。"),
    }
}

pub fn discarded(count: usize, only_path: Option<&str>) -> String {
    match (lang(), only_path) {
        (Lang::En, Some(path)) => format!("Discarded changes in '{path}'."),
        (Lang::Ja, Some(path)) => format!("'{path}' の変更を破棄しました。"),
        (Lang::En, None) => format!("Discarded changes in {count} files."),
        (Lang::Ja, None) => format!("{count} 件のファイルの変更を破棄しました。"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_display_preserves_user_names() {
        let branch = "feature/repo_session-unavailable-日本語";
        let shown = branch_result(branch, &OperationOutcome::Pull(PullOutcome::UpToDate));
        assert!(shown.contains(branch));
    }

    #[test]
    fn unknown_git_output_is_not_rewritten() {
        let reason = "remote: user-defined_hook rejected feature/my_branch-123";
        assert!(pull_summary(reason).ends_with(reason));
    }

    #[test]
    fn uncertain_results_are_not_success_summaries() {
        let reason = "remote: custom_hook";
        let success = completed("branch-push");
        let uncertain = unknown("branch-push", reason);
        let incomplete = partial("branch-push", reason);
        assert_ne!(uncertain, success);
        assert_ne!(incomplete, success);
        assert_ne!(uncertain, incomplete);
        assert!(uncertain.contains(reason));
        assert!(incomplete.contains(reason));
        assert!(!uncertain.contains("branch-push"));
        assert!(!incomplete.contains("branch-push"));
    }
}
