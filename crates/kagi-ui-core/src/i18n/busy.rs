//! Localized snackbar labels. Unknown internal tags never become display text.
use super::{lang, Lang};

// check-busy-labels validates operation producers against this table.
const LABELS: &[(&str, &str, &str)] = &[
    ("issue-create", "Creating issue…", "Issue を作成中…"),
    ("issue-comment", "Posting reply…", "返信を投稿中…"),
    ("merge-plan", "Planning merge…", "merge を計画中…"),
    ("merge", "Merging…", "merge 中…"),
    ("merge-into", "Merging…", "merge 中…"),
    ("merge-commit", "Committing merge…", "merge を commit 中…"),
    ("pull", "Pulling…", "pull 中…"),
    ("push", "Pushing…", "push 中…"),
    ("fetch", "Fetching…", "fetch 中…"),
    ("commit", "Committing…", "commit 中…"),
    ("amend", "Amending commit…", "commit を amend 中…"),
    ("checkout", "Checking out…", "checkout 中…"),
    ("checkout-commit", "Checking out…", "checkout 中…"),
    ("checkout-tracking", "Checking out…", "checkout 中…"),
    ("switch", "Switching branch…", "branch を切り替え中…"),
    (
        "switch-to-latest",
        "Switching branch…",
        "branch を切り替え中…",
    ),
    ("cherry-pick", "Cherry-picking…", "cherry-pick 中…"),
    ("revert", "Reverting…", "revert 中…"),
    ("discard", "Discarding…", "変更を破棄中…"),
    ("stash", "Stashing…", "stash 中…"),
    ("stash-push", "Stashing…", "stash 中…"),
    ("stash-apply", "Applying stash…", "stash を適用中…"),
    ("stash-pop", "Applying stash…", "stash を適用中…"),
    ("stash-drop", "Dropping stash…", "stash を削除中…"),
    (
        "remote-stash-drop",
        "Dropping remote stash…",
        "remote の stash を削除中…",
    ),
    (
        "create-worktree",
        "Creating worktree…",
        "worktree を作成中…",
    ),
    (
        "open-worktree",
        "Opening worktree…",
        "worktree を開いています…",
    ),
    (
        "remove-worktree",
        "Removing worktree…",
        "worktree を削除中…",
    ),
    ("create-branch", "Creating branch…", "branch を作成中…"),
    ("delete-branch", "Deleting branch…", "branch を削除中…"),
    (
        "delete-branch-plan",
        "Planning branch deletion…",
        "branch 削除を計画中…",
    ),
    (
        "delete-remote-branch",
        "Deleting remote branch…",
        "remote branch を削除中…",
    ),
    ("rename-branch", "Renaming branch…", "branch 名を変更中…"),
    ("set-upstream", "Setting upstream…", "upstream を設定中…"),
    (
        "branch-cleanup",
        "Cleaning up branches…",
        "branch を整理中…",
    ),
    ("branch-pull-ff", "Pulling branch…", "branch を pull 中…"),
    ("branch-push", "Pushing branch…", "branch を push 中…"),
    (
        "branch-push-set-upstream",
        "Pushing branch…",
        "branch を push 中…",
    ),
    ("rebase", "Rebasing…", "rebase 中…"),
    ("replay-onto", "Replaying commits…", "commit を replay 中…"),
    ("op-revert", "Reverting operation…", "操作を取り消し中…"),
    (
        "restore-to-point",
        "Restoring branches…",
        "branch を復元中…",
    ),
    (
        "sync-to-remote",
        "Syncing to remote…",
        "remote に揃えています…",
    ),
    ("reset-current", "Resetting changes…", "変更を reset 中…"),
    ("reset", "Resetting changes…", "変更を reset 中…"),
    ("undo", "Undoing commit…", "commit を取り消し中…"),
    (
        "force-with-lease-push",
        "Pushing with lease…",
        "lease を確認して push 中…",
    ),
    (
        "pr-merge",
        "Merging pull request…",
        "pull request を merge 中…",
    ),
    ("create-tag", "Creating tag…", "tag を作成中…"),
    ("push-tag", "Pushing tag…", "tag を push 中…"),
    ("snapshot", "Creating savepoint…", "復元点を作成中…"),
    (
        "restore-snapshot",
        "Restoring savepoint…",
        "復元点から復元中…",
    ),
    ("apply-suggestion", "Applying suggestion…", "提案を適用中…"),
    (
        "write-commit-graph",
        "Writing commit-graph…",
        "commit-graph を書き込み中…",
    ),
    (
        "enable-fsmonitor",
        "Enabling filesystem monitor…",
        "ファイルシステムモニターを有効化中…",
    ),
    ("editor-save", "Saving file…", "ファイルを保存中…"),
    ("stage", "Staging…", "stage 中…"),
    ("unstage", "Unstaging…", "stage を解除中…"),
    ("stage-all", "Staging all files…", "全ファイルを stage 中…"),
    (
        "unstage-all",
        "Unstaging all files…",
        "全ファイルの stage を解除中…",
    ),
    (
        "conflict-save",
        "Saving resolution…",
        "競合の解決内容を保存中…",
    ),
    (
        "conflict-dir-file:keep-directory",
        "Resolving conflict…",
        "競合を解決中…",
    ),
    (
        "conflict-dir-file:keep-file",
        "Resolving conflict…",
        "競合を解決中…",
    ),
    (
        "conflict-continue",
        "Continuing operation…",
        "操作を続行中…",
    ),
    ("conflict-skip", "Skipping commit…", "commit をスキップ中…"),
    ("conflict-abort", "Aborting operation…", "操作を中止中…"),
];

pub fn busy_label(op: &str) -> &'static str {
    label_for(op, lang())
}

fn label_for(op: &str, language: Lang) -> &'static str {
    match LABELS.iter().find(|(name, _, _)| *name == op) {
        Some((_, en, ja)) => match language {
            Lang::En => en,
            Lang::Ja => ja,
        },
        None => match language {
            Lang::En => "Processing…",
            Lang::Ja => "処理中…",
        },
    }
}

use crate::slow_read::SlowRead;

/// #355: `(reason, subject)` of a slow read, EN and JA. The reason is also the
/// snackbar label when no write is running.
fn slow_read_parts(read: SlowRead, language: Lang) -> (&'static str, &'static str) {
    match (read, language) {
        (SlowRead::AheadBehind, Lang::En) => (
            "Counting ahead/behind…",
            "counting commits against each upstream",
        ),
        (SlowRead::AheadBehind, Lang::Ja) => (
            "ahead/behind を計算中…",
            "各ブランチと upstream の差分の計算",
        ),
        (SlowRead::Worktrees, Lang::En) => (
            "Reading worktree status…",
            "checking every worktree for changes",
        ),
        (SlowRead::Worktrees, Lang::Ja) => {
            ("worktree の状態を読み込み中…", "各 worktree の変更の確認")
        }
        (SlowRead::WorktreeSize, Lang::En) => (
            "Measuring worktree size…",
            "walking every file in a worktree",
        ),
        (SlowRead::WorktreeSize, Lang::Ja) => (
            "worktree の容量を計測中…",
            "各 worktree 内の全ファイルの走査",
        ),
        (SlowRead::Analyze, Lang::En) => (
            "Analyzing hotspots…",
            "reading the commit history and touched files",
        ),
        (SlowRead::Analyze, Lang::Ja) => {
            ("hotspot を解析中…", "commit 履歴と変更ファイルの読み込み")
        }
        (SlowRead::Diff, Lang::En) => ("Loading diff…", "reading a large diff"),
        (SlowRead::Diff, Lang::Ja) => ("diff を読み込み中…", "大きい diff の読み込み"),
    }
}

/// Snackbar label for a slow read (shown when no write owns the snackbar).
pub fn slow_read_label(read: SlowRead) -> &'static str {
    slow_read_parts(read, lang()).0
}

/// The one-line explanation added to the busy snackbar once a read is slow.
pub fn slow_read_advice(read: SlowRead) -> String {
    slow_read_advice_for(read, lang())
}

fn slow_read_advice_for(read: SlowRead, language: Lang) -> String {
    let (reason, subject) = slow_read_parts(read, language);
    let reason = reason.trim_end_matches('…');
    match language {
        Lang::En => {
            format!("Taking a while: {reason} ({subject} takes time in large repositories)")
        }
        Lang::Ja => format!(
            "時間がかかっています: {reason}（大きいリポジトリでは{subject}に時間がかかります）"
        ),
    }
}

/// The snackbar's Skip button: stop the read and show its result as unknown.
pub fn slow_read_skip() -> &'static str {
    match lang() {
        Lang::En => "Skip",
        Lang::Ja => "スキップ",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_operations_have_both_languages() {
        for (op, en, ja) in [
            ("fetch", "Fetching…", "fetch 中…"),
            ("commit", "Committing…", "commit 中…"),
            ("stash-apply", "Applying stash…", "stash を適用中…"),
            ("discard", "Discarding…", "変更を破棄中…"),
            ("delete-branch", "Deleting branch…", "branch を削除中…"),
            ("editor-save", "Saving file…", "ファイルを保存中…"),
        ] {
            assert_eq!(label_for(op, Lang::En), en);
            assert_eq!(label_for(op, Lang::Ja), ja);
        }
        let mut names = std::collections::HashSet::new();
        for (op, en, ja) in LABELS {
            assert!(names.insert(op), "duplicate label: {op}");
            assert_ne!(en, ja);
            assert!(!en.is_empty() && !ja.is_empty());
        }
    }

    #[test]
    fn unknown_tags_never_leak_internal_identifiers() {
        for op in ["app-writer", "private-job-123", "", "内部-id"] {
            assert_eq!(label_for(op, Lang::En), "Processing…");
            assert_eq!(label_for(op, Lang::Ja), "処理中…");
        }
    }

    /// #355: every slow read explains itself in both languages with the
    /// "taking a while: <reason> (<subject> in large repositories)" shape.
    #[test]
    fn slow_read_advice_names_reason_and_subject() {
        assert_eq!(
            slow_read_advice_for(SlowRead::AheadBehind, Lang::En),
            "Taking a while: Counting ahead/behind (counting commits against each upstream takes time in large repositories)"
        );
        assert_eq!(
            slow_read_advice_for(SlowRead::AheadBehind, Lang::Ja),
            "時間がかかっています: ahead/behind を計算中（大きいリポジトリでは各ブランチと upstream の差分の計算に時間がかかります）"
        );
        for read in [
            SlowRead::AheadBehind,
            SlowRead::Worktrees,
            SlowRead::WorktreeSize,
            SlowRead::Analyze,
            SlowRead::Diff,
        ] {
            let (en, ja) = (
                slow_read_parts(read, Lang::En),
                slow_read_parts(read, Lang::Ja),
            );
            assert_ne!(en, ja, "{read:?} is not translated");
            assert!(slow_read_advice_for(read, Lang::En).starts_with("Taking a while: "));
            assert!(slow_read_advice_for(read, Lang::Ja).starts_with("時間がかかっています: "));
        }
    }
}
