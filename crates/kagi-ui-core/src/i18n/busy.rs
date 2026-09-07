//! Localized snackbar labels. Unknown internal tags never become display text.
use super::{lang, Lang};

// check-busy-labels validates operation producers against this table.
const LABELS: &[(&str, &str, &str)] = &[
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
}
