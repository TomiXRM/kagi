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

/// A mechanical explanation of the running operation's kind, never an
/// inferred bottleneck. Unknown kinds have an honest generic description.
pub fn slow_write_advice(op: &str, seconds: u64) -> String {
    slow_write_advice_for(op, seconds, lang())
}

/// Every op kind with a classified slow-write reason, taken from what the
/// operation actually writes. A kind not listed gets the generic reason: the
/// explanation never guesses (#995 review). `branch-cleanup` stays generic
/// because a batch may delete remote branches as well as local ones.
const SLOW_WRITE_KINDS: &[(&str, super::Msg)] = {
    use super::Msg::*;
    &[
        // Waits on a remote or on GitHub.
        ("fetch", SlowWriteNetwork),
        ("pull", SlowWriteNetwork),
        ("push", SlowWriteNetwork),
        ("branch-pull-ff", SlowWriteNetwork),
        ("branch-push", SlowWriteNetwork),
        ("branch-push-set-upstream", SlowWriteNetwork),
        ("force-with-lease-push", SlowWriteNetwork),
        ("push-tag", SlowWriteNetwork),
        ("sync-to-remote", SlowWriteNetwork),
        ("delete-remote-branch", SlowWriteNetwork),
        ("remote-stash-drop", SlowWriteNetwork),
        ("pr-merge", SlowWriteNetwork),
        ("pr-comment", SlowWriteNetwork),
        ("pr-review", SlowWriteNetwork),
        ("pr-edit", SlowWriteNetwork),
        ("issue-create", SlowWriteNetwork),
        ("issue-comment", SlowWriteNetwork),
        // Replays commits.
        ("rebase", SlowWriteRebase),
        ("replay-onto", SlowWriteRebase),
        ("cherry-pick", SlowWriteRebase),
        ("revert", SlowWriteRebase),
        // Updates the worktree to another commit.
        ("checkout", SlowWriteCheckout),
        ("checkout-commit", SlowWriteCheckout),
        ("checkout-tracking", SlowWriteCheckout),
        ("switch", SlowWriteCheckout),
        ("switch-to-latest", SlowWriteCheckout),
        // Ref or branch-config only: no commit object, no index, no worktree.
        ("reset-current", SlowWriteRefs),
        ("reset", SlowWriteRefs),
        ("undo", SlowWriteRefs),
        ("redo", SlowWriteRefs),
        ("op-revert", SlowWriteRefs),
        ("restore-to-point", SlowWriteRefs),
        ("create-branch", SlowWriteRefs),
        ("delete-branch", SlowWriteRefs),
        ("rename-branch", SlowWriteRefs),
        ("set-upstream", SlowWriteRefs),
        ("create-tag", SlowWriteRefs),
        // Combines histories.
        ("merge", SlowWriteMerge),
        ("merge-into", SlowWriteMerge),
        ("merge-commit", SlowWriteMerge),
        // Writes a commit object.
        ("commit", SlowWriteCommit),
        ("amend", SlowWriteCommit),
        // Stash entries.
        ("stash", SlowWriteStash),
        ("stash-push", SlowWriteStash),
        ("stash-apply", SlowWriteStash),
        ("stash-pop", SlowWriteStash),
        ("stash-drop", SlowWriteStash),
        ("snapshot", SlowWriteStash),
        ("restore-snapshot", SlowWriteStash),
        // Local files in a worktree.
        ("create-worktree", SlowWriteWorktree),
        ("open-worktree", SlowWriteWorktree),
        ("remove-worktree", SlowWriteWorktree),
        ("discard", SlowWriteWorktree),
        ("editor-save", SlowWriteWorktree),
        ("stage", SlowWriteWorktree),
        ("unstage", SlowWriteWorktree),
        ("stage-all", SlowWriteWorktree),
        ("unstage-all", SlowWriteWorktree),
        ("apply-suggestion", SlowWriteWorktree),
        // Conflict resolution.
        ("conflict-save", SlowWriteConflict),
        ("conflict-continue", SlowWriteConflict),
        ("conflict-skip", SlowWriteConflict),
        ("conflict-abort", SlowWriteConflict),
        ("conflict-dir-file:keep-directory", SlowWriteConflict),
        ("conflict-dir-file:keep-file", SlowWriteConflict),
        // Repository maintenance data.
        ("write-commit-graph", SlowWriteLocal),
        ("enable-fsmonitor", SlowWriteLocal),
    ]
};

fn slow_write_reason(op: &str) -> super::Msg {
    SLOW_WRITE_KINDS
        .iter()
        .find(|(kind, _)| *kind == op)
        .map_or(super::Msg::SlowWriteGeneric, |(_, reason)| *reason)
}

fn slow_write_advice_for(op: &str, seconds: u64, language: Lang) -> String {
    // The reason and the elapsed seconds only: no lead-in sentence.
    format!("{} · {seconds} s", slow_write_reason(op).t_for(language))
}

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

/// #355: the reason of a slow read, EN and JA. It is also the snackbar label
/// when no write is running.
fn slow_read_reason(read: SlowRead, language: Lang) -> &'static str {
    match (read, language) {
        (SlowRead::AheadBehind, Lang::En) => "Counting ahead/behind…",
        (SlowRead::AheadBehind, Lang::Ja) => "ahead/behind を計算中…",
        (SlowRead::Worktrees, Lang::En) => "Reading worktree status…",
        (SlowRead::Worktrees, Lang::Ja) => "worktree の状態を読み込み中…",
        (SlowRead::WorktreeSize, Lang::En) => "Measuring worktree size…",
        (SlowRead::WorktreeSize, Lang::Ja) => "worktree の容量を計測中…",
        (SlowRead::Analyze, Lang::En) => "Analyzing hotspots…",
        (SlowRead::Analyze, Lang::Ja) => "hotspot を解析中…",
        (SlowRead::Diff, Lang::En) => "Loading diff…",
        (SlowRead::Diff, Lang::Ja) => "diff を読み込み中…",
    }
}

/// Snackbar label for a slow read (shown when no write owns the snackbar).
pub fn slow_read_label(read: SlowRead) -> &'static str {
    slow_read_reason(read, lang())
}

/// The one-line explanation added to the busy snackbar once a read is slow.
pub fn slow_read_advice(read: SlowRead) -> String {
    slow_read_advice_for(read, lang())
}

fn slow_read_advice_for(read: SlowRead, language: Lang) -> String {
    // The reason only: no lead-in and no explanatory sentence.
    slow_read_reason(read, language)
        .trim_end_matches('…')
        .to_string()
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

    /// #355: a slow read names its reason in both languages, and nothing
    /// else — no lead-in, no explanatory sentence.
    #[test]
    fn slow_read_advice_is_the_reason_only() {
        assert_eq!(
            slow_read_advice_for(SlowRead::AheadBehind, Lang::En),
            "Counting ahead/behind"
        );
        assert_eq!(
            slow_read_advice_for(SlowRead::AheadBehind, Lang::Ja),
            "ahead/behind を計算中"
        );
        for read in [
            SlowRead::AheadBehind,
            SlowRead::Worktrees,
            SlowRead::WorktreeSize,
            SlowRead::Analyze,
            SlowRead::Diff,
        ] {
            assert_ne!(
                slow_read_reason(read, Lang::En),
                slow_read_reason(read, Lang::Ja),
                "{read:?} is not translated"
            );
            for language in [Lang::En, Lang::Ja] {
                let advice = slow_read_advice_for(read, language);
                assert_eq!(
                    advice,
                    slow_read_reason(read, language).trim_end_matches('…')
                );
            }
        }
    }

    /// #995 review: every GitHub write that holds the lease waits on the
    /// network, so none of them falls back to the generic reason.
    #[test]
    fn github_writes_wait_on_the_network() {
        let network = format!(
            "{} · 3 s",
            super::super::Msg::SlowWriteNetwork.t_for(Lang::En)
        );
        for op in [
            "pr-merge",
            "pr-comment",
            "pr-review",
            "pr-edit",
            "issue-create",
            "issue-comment",
        ] {
            assert_eq!(slow_write_advice_for(op, 3, Lang::En), network, "{op}");
        }
    }

    /// #995 review: the whole classification, pinned. Every listed op kind
    /// maps to the reason for what it actually writes; anything unlisted is
    /// generic, never a guess. Changing a row means changing this table.
    #[test]
    fn every_slow_write_kind_is_classified_by_what_it_writes() {
        use super::super::Msg::*;
        let expected: &[(&str, super::super::Msg)] = &[
            ("fetch", SlowWriteNetwork),
            ("pull", SlowWriteNetwork),
            ("push", SlowWriteNetwork),
            ("branch-pull-ff", SlowWriteNetwork),
            ("branch-push", SlowWriteNetwork),
            ("branch-push-set-upstream", SlowWriteNetwork),
            ("force-with-lease-push", SlowWriteNetwork),
            ("push-tag", SlowWriteNetwork),
            ("sync-to-remote", SlowWriteNetwork),
            ("delete-remote-branch", SlowWriteNetwork),
            ("remote-stash-drop", SlowWriteNetwork),
            ("pr-merge", SlowWriteNetwork),
            ("pr-comment", SlowWriteNetwork),
            ("pr-review", SlowWriteNetwork),
            ("pr-edit", SlowWriteNetwork),
            ("issue-create", SlowWriteNetwork),
            ("issue-comment", SlowWriteNetwork),
            ("rebase", SlowWriteRebase),
            ("replay-onto", SlowWriteRebase),
            ("cherry-pick", SlowWriteRebase),
            ("revert", SlowWriteRebase),
            ("checkout", SlowWriteCheckout),
            ("checkout-commit", SlowWriteCheckout),
            ("checkout-tracking", SlowWriteCheckout),
            ("switch", SlowWriteCheckout),
            ("switch-to-latest", SlowWriteCheckout),
            ("reset-current", SlowWriteRefs),
            ("reset", SlowWriteRefs),
            ("undo", SlowWriteRefs),
            ("redo", SlowWriteRefs),
            ("op-revert", SlowWriteRefs),
            ("restore-to-point", SlowWriteRefs),
            ("create-branch", SlowWriteRefs),
            ("delete-branch", SlowWriteRefs),
            ("rename-branch", SlowWriteRefs),
            ("set-upstream", SlowWriteRefs),
            ("create-tag", SlowWriteRefs),
            ("merge", SlowWriteMerge),
            ("merge-into", SlowWriteMerge),
            ("merge-commit", SlowWriteMerge),
            ("commit", SlowWriteCommit),
            ("amend", SlowWriteCommit),
            ("stash", SlowWriteStash),
            ("stash-push", SlowWriteStash),
            ("stash-apply", SlowWriteStash),
            ("stash-pop", SlowWriteStash),
            ("stash-drop", SlowWriteStash),
            ("snapshot", SlowWriteStash),
            ("restore-snapshot", SlowWriteStash),
            ("create-worktree", SlowWriteWorktree),
            ("open-worktree", SlowWriteWorktree),
            ("remove-worktree", SlowWriteWorktree),
            ("discard", SlowWriteWorktree),
            ("editor-save", SlowWriteWorktree),
            ("stage", SlowWriteWorktree),
            ("unstage", SlowWriteWorktree),
            ("stage-all", SlowWriteWorktree),
            ("unstage-all", SlowWriteWorktree),
            ("apply-suggestion", SlowWriteWorktree),
            ("conflict-save", SlowWriteConflict),
            ("conflict-continue", SlowWriteConflict),
            ("conflict-skip", SlowWriteConflict),
            ("conflict-abort", SlowWriteConflict),
            ("conflict-dir-file:keep-directory", SlowWriteConflict),
            ("conflict-dir-file:keep-file", SlowWriteConflict),
            ("write-commit-graph", SlowWriteLocal),
            ("enable-fsmonitor", SlowWriteLocal),
        ];
        assert_eq!(
            SLOW_WRITE_KINDS, expected,
            "the classification table changed"
        );
        for (kind, reason) in expected {
            assert_eq!(slow_write_reason(kind), *reason, "{kind}");
        }
        let mut seen = std::collections::HashSet::new();
        for (kind, _) in SLOW_WRITE_KINDS {
            assert!(seen.insert(kind), "duplicate kind: {kind}");
        }
        // Unlisted kinds — including branch-cleanup, which may also delete
        // remote branches — are generic.
        for kind in ["branch-cleanup", "merge-plan", "unexpected-kind", ""] {
            assert_eq!(slow_write_reason(kind), SlowWriteGeneric, "{kind}");
        }
        assert_eq!(
            slow_write_advice_for("delete-branch", 3, Lang::En),
            "refs: updating branches · 3 s"
        );
        assert_eq!(
            slow_write_advice_for("set-upstream", 3, Lang::Ja),
            "refs: branch を更新中 · 3 s"
        );
    }

    #[test]
    fn slow_write_advice_names_known_kind_and_elapsed_without_guessing() {
        assert_eq!(
            slow_write_advice_for("pull", 2, Lang::En),
            "network: waiting for the remote · 2 s"
        );
        assert_eq!(
            slow_write_advice_for("pull", 4, Lang::Ja),
            "network: remote の応答を待っています · 4 s"
        );
        assert_eq!(
            slow_write_advice_for("checkout", 4, Lang::En),
            "checkout: updating the worktree · 4 s"
        );
        assert_eq!(
            slow_write_advice_for("unexpected-kind", 9, Lang::En),
            "operation in progress · 9 s"
        );
    }
}
