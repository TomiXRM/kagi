//! Display-only operation summaries. Never use these strings in receipts or klog.
use super::{diagnostic, operation_label, state_text};
use crate::i18n::{lang, Lang};
use kagi_domain::{operation::OperationOutcome, plan::PullOutcome};

const REPOSITORY_BUSY_EN: &str = "Another Kagi window is using this repository.";
const REPOSITORY_BUSY_JA: &str = "別の Kagi ウィンドウがこの repository を使用中です。";

pub fn conflict_saved() -> &'static str {
    match lang() {
        Lang::En => "Resolution saved and staged.",
        Lang::Ja => "解決内容を保存し、stage しました。",
    }
}
pub fn completed(op: &str) -> String {
    let label = operation_label(op);
    match lang() {
        Lang::En => format!("{label}: Completed."),
        Lang::Ja => format!("{label}: 完了しました。"),
    }
}

/// Recognize the stable admission cause in previews and after a locale change.
pub fn is_repository_busy_message(message: &str) -> bool {
    [
        crate::i18n::Msg::AdviceConflictsRepositoryBusy.t_for(Lang::En),
        crate::i18n::Msg::AdviceConflictsRepositoryBusy.t_for(Lang::Ja),
        REPOSITORY_BUSY_EN,
        REPOSITORY_BUSY_JA,
    ]
    .into_iter()
    .any(|needle| message.contains(needle))
}

pub fn failed(op: &str, reason: &str) -> String {
    let label = operation_label(op);
    let reason = diagnostic(reason);
    match lang() {
        Lang::En => format!("{label}: Failed: {reason}"),
        Lang::Ja => format!("{label}: 失敗しました: {reason}"),
    }
}

/// Short, display-only failure preview. The unchanged receipt owns the details.
/// Never interpolate diagnostic text here: it may contain credentials or paths.
pub fn failure_preview(op: &str, reason: &str) -> String {
    outcome_preview(op, reason, PreviewOutcome::Failed)
}

/// Partial writes must remain distinguishable from failures with no known effect.
pub fn partial_preview(op: &str, reason: &str) -> String {
    outcome_preview(op, reason, PreviewOutcome::Partial)
}

/// Unconfirmed writes must never invite a retry, even for a known network cause.
pub fn unknown_preview(op: &str, reason: &str) -> String {
    outcome_preview(op, reason, PreviewOutcome::Unknown)
}

#[derive(Clone, Copy)]
enum PreviewOutcome {
    Failed,
    Partial,
    Unknown,
}

fn outcome_preview(op: &str, reason: &str, outcome: PreviewOutcome) -> String {
    use super::text;
    let label = if op.len() <= 80 && super::known_operation(op) {
        operation_label(op)
    } else {
        std::borrow::Cow::Borrowed(text("Operation", "操作"))
    };
    let status = match outcome {
        PreviewOutcome::Failed => "",
        PreviewOutcome::Partial => text(
            "Only partly completed. Check the repository before continuing. ",
            "一部のみ完了しました。続行する前にリポジトリを確認してください。",
        ),
        PreviewOutcome::Unknown => text(
            "The result could not be confirmed. Do not retry yet. ",
            "結果を確認できませんでした。まだ再試行しないでください。",
        ),
    };
    let failure_cause = FailureCause::classify(reason);
    let cause = failure_cause.text();
    let details = text(
        "See the Operation Log for details.",
        "詳しくは操作ログを確認してください。",
    );
    let retry = match (outcome, failure_cause) {
        (PreviewOutcome::Failed, FailureCause::RepositoryBusy) => {
            text(" Try again later.", "後でもう一度お試しください。")
        }
        _ => "",
    };
    format!("{label}: {status}{cause}{retry} {details}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureCause {
    UpstreamChanged,
    Network,
    Authentication,
    IndexLocked,
    RepositoryBusy,
    Untrusted,
    Refused,
    Other,
}

impl FailureCause {
    // This only selects preview wording, never the semantic outcome or retry
    // policy. Unknown diagnostics intentionally receive the safe generic text.
    fn classify(reason: &str) -> Self {
        let contains = |needle: &str| {
            reason
                .as_bytes()
                .windows(needle.len())
                .any(|part| part.eq_ignore_ascii_case(needle.as_bytes()))
        };
        let any = |needles: &[&str]| needles.iter().any(|needle| contains(needle));
        if any(&[
            "non-fast-forward",
            "non fast forward",
            "fetch first",
            "remote contains work",
            "tip of your current branch is behind",
            "new upstream commits",
            "remote has newer commits",
            "リモートに新しいコミット",
            "upstream に新しい commit",
        ]) {
            Self::UpstreamChanged
        } else if any(&[
            "authentication failed",
            "authentication required",
            "requires authentication",
            "authentication was denied",
            "認証が必要",
            "認証に失敗",
            "認証が拒否",
            "could not read username",
            "terminal prompts disabled",
            "permission denied (publickey",
            "permission denied, please try again",
            "invalid username or password",
            "invalid credentials",
            "http 401",
            "error: 401",
            "error: 403",
            "access denied",
        ]) {
            Self::Authentication
        } else if any(&[
            // A lock pathname alone can also occur in permission or remote errors.
            // Require an existing-file diagnostic or an explicit locked-area message.
            "index.lock: file exists",
            "index.lock': file exists",
            "index.lock\": file exists",
            "staging area is locked",
            "ステージ領域がロック",
            "index がロック",
        ]) {
            Self::IndexLocked
        } else if any(&[
            crate::i18n::Msg::AdviceConflictsRepositoryBusy.t_for(Lang::En),
            crate::i18n::Msg::AdviceConflictsRepositoryBusy.t_for(Lang::Ja),
            REPOSITORY_BUSY_EN,
            REPOSITORY_BUSY_JA,
        ]) {
            Self::RepositoryBusy
        } else if any(&[
            "not trusted",
            "untrusted repository",
            "dubious ownership",
            "safe.directory",
            "approve this repository before changing",
            "信頼を確認",
            "変更が許可されていないリポジトリ",
        ]) {
            Self::Untrusted
        } else if any(&[
            "could not resolve host",
            "could not resolve hostname",
            "failed to connect",
            "connection refused",
            "connection timed out",
            "connection reset",
            "network is unreachable",
            "no route to host",
            "remote end hung up",
            "unable to access",
            "does not appear to be a git repository",
            "could not read from remote repository",
            "unreachable",
            "could not be reached",
            "couldn't reach",
            "接続できません",
            "到達できません",
        ]) {
            Self::Network
        } else if any(&[
            "refused",
            "blocked",
            "blocker",
            "rejected",
            "拒否され",
            "ブロックされ",
        ]) {
            Self::Refused
        } else {
            Self::Other
        }
    }

    fn text(self) -> &'static str {
        use super::text;
        match self {
            Self::UpstreamChanged => text(
                "The remote has newer commits. Fetch and review the differences.",
                "リモートに新しいコミットがあります。fetch して差分を確認してください。",
            ),
            Self::Network => text(
                "The remote could not be reached. Check your connection and remote settings.",
                "リモートに接続できませんでした。通信環境とリモート設定を確認してください。",
            ),
            Self::Authentication => text(
                "Authentication was denied. Check your sign-in and repository access.",
                "認証が拒否されました。ログイン情報とリポジトリへのアクセス権を確認してください。",
            ),
            Self::IndexLocked => text(
                "The staging area is locked. Check for another Git operation before continuing.",
                "ステージ領域がロックされています。続行する前に別の Git 操作が実行中でないか確認してください。",
            ),
            Self::RepositoryBusy => text(REPOSITORY_BUSY_EN, REPOSITORY_BUSY_JA),
            Self::Untrusted => text(
                "The repository is not trusted for changes. Review its trust settings.",
                "変更が許可されていないリポジトリです。信頼設定を確認してください。",
            ),
            Self::Refused => text(
                "The operation was blocked. Review the repository and operation warnings.",
                "操作が拒否されました。リポジトリの状態と操作の注意事項を確認してください。",
            ),
            Self::Other => text(
                "The operation could not be completed. Review its details before continuing.",
                "操作を完了できませんでした。続行する前に詳細を確認してください。",
            ),
        }
    }
}

pub fn partial(op: &str, reason: &str) -> String {
    let label = operation_label(op);
    let reason = diagnostic(reason);
    match lang() {
        Lang::En => format!(
            "{label}: Only partly completed. Check the repository before continuing: {reason}"
        ),
        Lang::Ja => format!(
            "{label}: 一部のみ完了しました。続行する前に repository を確認してください: {reason}"
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

pub fn recording_failed(
    repo: impl std::fmt::Display,
    op: &str,
    outcome: crate::i18n::op::RecordingOutcome,
    reason: &str,
) -> String {
    let label = operation_label(op);
    format!(
        "{repo}: {label}: {}",
        crate::i18n::op::oplog_write_failed(outcome, reason)
    )
}

pub fn progress(op: &str) -> String {
    let label = operation_label(op);
    match lang() {
        Lang::En => format!("{label}: In progress…"),
        Lang::Ja => format!("{label}: 実行中…"),
    }
}

pub fn branch_progress(op: &str, branch: &str) -> String {
    let label = operation_label(op);
    match lang() {
        Lang::En => format!("{label}: '{branch}': In progress…"),
        Lang::Ja => format!("{label}: '{branch}' を実行中…"),
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
        Lang::En if deleted == 1 => "Deleted 1 branch.".to_string(),
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
        Lang::En => format!("{label}: Completed on '{branch}'."),
        Lang::Ja => format!("{label}: '{branch}' への操作が完了しました。"),
    }
}

pub fn push_result(outcome: &OperationOutcome) -> String {
    let OperationOutcome::Push(result) = outcome else {
        return unknown("push", &diagnostic("push: unexpected outcome"));
    };
    let noun = if result.pushed == 1 {
        "commit"
    } else {
        "commits"
    };
    match (lang(), result.set_upstream) {
        (Lang::En, false) => format!("Pushed {} {noun}.", result.pushed),
        (Lang::En, true) => format!("Pushed {} {noun} and set upstream.", result.pushed),
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
                (Lang::En, true) => {
                    let noun = if *removed_untracked == 1 { "file was" } else { "files were" };
                    format!("Branch '{branch}' synced to {target}. Local changes were backed up; {removed_untracked} untracked {noun} removed.")
                }
                (Lang::Ja, true) => format!("branch '{branch}' を {target} に同期しました。ローカルの変更はバックアップ済みです。{removed_untracked} 件の untracked ファイルを削除しました。"),
            }
        }
        _ => unknown("branch operation", &diagnostic("unexpected outcome")),
    }
}

/// Returns a complete success headline, not an unrecognized backend output line.
pub fn pull_summary(summary: &str) -> String {
    let (result, restored) = summary
        .split_once("; ")
        .map_or((summary, None), |(a, b)| (a, Some(b)));
    let label = operation_label("pull");
    let mut shown = if matches!(
        result,
        "already up to date" | "Already up to date." | "Already up-to-date."
    ) {
        match lang() {
            Lang::En => format!("{label}: Already up to date."),
            Lang::Ja => format!("{label}: すでに最新です。"),
        }
    } else if let Some(target) = result.strip_prefix("fast-forward to ") {
        match lang() {
            Lang::En => format!("{label}: Fast-forwarded to {target}."),
            Lang::Ja => format!("{label}: {target} に fast-forward しました。"),
        }
    } else if let Some(commit) = result.strip_prefix("merge commit ") {
        match lang() {
            Lang::En => format!("{label}: Created merge commit {commit}."),
            Lang::Ja => format!("{label}: merge commit {commit} を作成しました。"),
        }
    } else {
        return completed("pull");
    };
    if let Some(restored) = restored {
        shown.push(' ');
        shown.push_str(&diagnostic(restored));
    }
    shown
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
        Lang::En if count == 1 => "Removed 1 stale worktree entry.".to_string(),
        Lang::En => format!("Removed {count} stale worktree entries."),
        Lang::Ja => format!("{count} 件の使われなくなった worktree の登録を削除しました。"),
    }
}

pub fn discarded(count: usize, only_path: Option<&str>) -> String {
    match (lang(), only_path) {
        (Lang::En, Some(path)) => format!("Discarded changes in '{path}'."),
        (Lang::Ja, Some(path)) => format!("'{path}' の変更を破棄しました。"),
        (Lang::En, None) if count == 1 => "Discarded changes in 1 file.".to_string(),
        (Lang::En, None) => format!("Discarded changes in {count} files."),
        (Lang::Ja, None) => format!("{count} 件のファイルの変更を破棄しました。"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translated_overrides_keep_their_cause_without_preserving_private_values() {
        use crate::i18n::{set_lang_no_persist, tests::LOCK};
        let _guard = LOCK.lock();
        let previous = lang();
        for language in [Lang::En, Lang::Ja] {
            set_lang_no_persist(language);
            for cause in [
                FailureCause::UpstreamChanged,
                FailureCause::Network,
                FailureCause::Authentication,
                FailureCause::IndexLocked,
                FailureCause::RepositoryBusy,
                FailureCause::Untrusted,
                FailureCause::Refused,
            ] {
                let translated = format!("/private/secret: {}", cause.text());
                assert_eq!(FailureCause::classify(&translated), cause);
                let shown = failure_preview("push", &translated);
                assert!(shown.contains(cause.text()), "{shown}");
                assert!(!shown.contains("/private/secret"), "{shown}");
            }
            let authentication = crate::i18n::Msg::FetchAuthenticationRequired
                .t()
                .replace("{}", "/private/secret");
            assert_eq!(
                FailureCause::classify(&authentication),
                FailureCause::Authentication,
            );
            assert!(!failure_preview("fetch", &authentication).contains("/private/secret"));
        }
        set_lang_no_persist(previous);
    }

    #[test]
    fn repository_busy_previews_explain_when_a_refused_write_can_be_retried() {
        use crate::i18n::{set_lang_no_persist, tests::LOCK};
        let _guard = LOCK.lock();
        let previous = lang();
        for (language, expected, retry) in [
            (
                Lang::En,
                "Another Kagi window is using this repository. Try again later.",
                "Try again later.",
            ),
            (
                Lang::Ja,
                "別の Kagi ウィンドウがこの repository を使用中です。後でもう一度お試しください。",
                "後でもう一度お試しください。",
            ),
        ] {
            set_lang_no_persist(language);
            let reason = crate::i18n::Msg::AdviceConflictsRepositoryBusy.t();
            assert_eq!(FailureCause::classify(reason), FailureCause::RepositoryBusy);
            for stored_language in [Lang::En, Lang::Ja] {
                assert!(is_repository_busy_message(
                    crate::i18n::Msg::AdviceConflictsRepositoryBusy.t_for(stored_language),
                ));
            }
            for displayed in [REPOSITORY_BUSY_EN, REPOSITORY_BUSY_JA] {
                assert!(is_repository_busy_message(displayed));
                assert_eq!(
                    FailureCause::classify(displayed),
                    FailureCause::RepositoryBusy
                );
            }
            assert!(!is_repository_busy_message(""));
            assert!(!is_repository_busy_message("unrelated failure information"));
            for operation in ["pull", "stage-all", "commit"] {
                let preview = failure_preview(operation, reason);
                assert!(is_repository_busy_message(&preview), "{preview}");
                assert!(preview.contains(expected), "{operation}: {language:?}");
                for shown in [
                    partial_preview(operation, reason),
                    unknown_preview(operation, reason),
                ] {
                    assert!(
                        shown.contains(FailureCause::RepositoryBusy.text()),
                        "{shown}"
                    );
                    assert!(!shown.contains(retry), "{shown}");
                }
            }
        }
        set_lang_no_persist(previous);
    }

    #[test]
    fn index_lock_previews_require_competition_not_just_a_lock_path() {
        use crate::i18n::{set_lang_no_persist, tests::LOCK};
        let _guard = LOCK.lock();
        let previous = lang();
        let fixtures = [
            (
                "fatal: Unable to create '/private/repository/.git/index.lock': File exists.",
                FailureCause::IndexLocked,
            ),
            (
                "Unable to create \"/private/repository/.git/index.lock\": File exists",
                FailureCause::IndexLocked,
            ),
            ("index.lock: File exists", FailureCause::IndexLocked),
            ("The staging area is locked.", FailureCause::IndexLocked),
            (
                "ステージ領域がロックされています。",
                FailureCause::IndexLocked,
            ),
            ("index がロックされています。", FailureCause::IndexLocked),
            (
                "fatal: Unable to create '/private/repository/.git/index.lock': Permission denied",
                FailureCause::Other,
            ),
            (
                "/private/repository/.git/index.lock: 許可がありません",
                FailureCause::Other,
            ),
            ("/private/repository/.git/index.lock", FailureCause::Other),
            (
                "File exists: /private/repository/unrelated; /private/repository/.git/index.lock",
                FailureCause::Other,
            ),
            (
                "fatal: unable to access 'https://example.test/index.lock/': Failed to connect",
                FailureCause::Network,
            ),
            (
                "https://example.test/index.lock/ に接続できません",
                FailureCause::Network,
            ),
        ];
        for language in [Lang::En, Lang::Ja] {
            set_lang_no_persist(language);
            for (reason, expected) in fixtures {
                let raw = reason.to_owned();
                assert_eq!(FailureCause::classify(&raw), expected, "{reason}");
                for shown in [
                    failure_preview("stage-hunk", &raw),
                    partial_preview("stage-hunk", &raw),
                    unknown_preview("stage-hunk", &raw),
                ] {
                    assert!(shown.contains(expected.text()), "{shown}");
                    assert!(!shown.contains("/private/"), "{shown}");
                    assert!(!shown.contains("example.test"), "{shown}");
                    if expected != FailureCause::IndexLocked {
                        assert!(!shown.contains(FailureCause::IndexLocked.text()), "{shown}");
                    }
                }
                assert_eq!(raw, reason, "preview must not alter durable details");
            }
        }
        set_lang_no_persist(previous);
    }

    #[test]
    fn failure_previews_classify_causes_without_exposing_diagnostics() {
        use crate::i18n::{set_lang_no_persist, tests::LOCK};
        let _guard = LOCK.lock();
        let previous = lang();
        let fixtures = [
            ("push", "non-fast-forward", FailureCause::UpstreamChanged),
            ("push", "fetch first", FailureCause::UpstreamChanged),
            (
                "push",
                "new upstream commits",
                FailureCause::UpstreamChanged,
            ),
            ("fetch", "Could not resolve host", FailureCause::Network),
            ("push", "Connection refused", FailureCause::Network),
            (
                "fetch",
                "does not appear to be a git repository",
                FailureCause::Network,
            ),
            (
                "fetch",
                "Authentication failed",
                FailureCause::Authentication,
            ),
            (
                "push",
                "Permission denied (publickey)",
                FailureCause::Authentication,
            ),
            (
                "push",
                "unable to access: error: 403",
                FailureCause::Authentication,
            ),
            (
                "stage-hunk",
                "index.lock: File exists",
                FailureCause::IndexLocked,
            ),
            (
                "stage",
                crate::i18n::Msg::AdviceConflictsRepositoryBusy.t_for(Lang::En),
                FailureCause::RepositoryBusy,
            ),
            (
                "stage-all",
                crate::i18n::Msg::AdviceConflictsRepositoryBusy.t_for(Lang::Ja),
                FailureCause::RepositoryBusy,
            ),
            (
                "stash-push",
                "repository is not trusted for writes",
                FailureCause::Untrusted,
            ),
            (
                "commit",
                "detected dubious ownership",
                FailureCause::Untrusted,
            ),
            (
                "push",
                "remote rejected: protected branch",
                FailureCause::Refused,
            ),
            ("stash-pop", "plan has blockers", FailureCause::Refused),
            ("fetch", "unrecognized_hook_failure", FailureCause::Other),
        ];
        for language in [Lang::En, Lang::Ja] {
            set_lang_no_persist(language);
            for (op, cause, expected) in fixtures {
                let raw = format!(
                    "Git: {op} failed (exit 128): {cause}\n\
                     To /private/tmp/private_repository\n\
                     https://private_user:private_token@example.test/repo\n\
                     C:\\private\\repository"
                );
                let unchanged = raw.clone();
                assert_eq!(FailureCause::classify(&raw), expected);
                for shown in [
                    failure_preview(op, &raw),
                    partial_preview(op, &raw),
                    unknown_preview(op, &raw),
                ] {
                    assert!(shown.starts_with(operation_label(op).as_ref()), "{shown}");
                    assert!(shown.contains(expected.text()), "{shown}");
                    assert!(shown.chars().count() <= 400, "{shown}");
                    assert_eq!(shown.lines().count(), 1, "{shown}");
                    for private in [
                        "exit 128",
                        "/private/",
                        "C:\\",
                        "private_user",
                        "private_token",
                        "example.test",
                        "unrecognized_hook_failure",
                        "Git:",
                    ] {
                        assert!(!shown.contains(private), "{shown}");
                    }
                    match language {
                        Lang::En => {
                            assert!(shown.contains("Operation Log"), "{shown}");
                            assert!(!shown
                                .chars()
                                .any(|ch| ('\u{3040}'..='\u{30ff}').contains(&ch)));
                        }
                        Lang::Ja => {
                            assert!(shown.contains("操作ログ"), "{shown}");
                            assert!(shown.contains("確認"), "{shown}");
                            assert!(!shown.contains("Failed"), "{shown}");
                            assert!(!shown.contains("Authentication failed"), "{shown}");
                        }
                    }
                }
                assert_eq!(raw, unchanged, "preview must not alter durable details");
            }
            let raw_operation = "/private/tmp/unknown_operation";
            assert!(!failure_preview(raw_operation, "unknown").contains(raw_operation));
        }
        set_lang_no_persist(previous);
    }

    #[test]
    fn uncertain_previews_keep_their_safety_meaning_for_known_causes() {
        use crate::i18n::{set_lang_no_persist, tests::LOCK};
        let _guard = LOCK.lock();
        let previous = lang();
        for language in [Lang::En, Lang::Ja] {
            set_lang_no_persist(language);
            let reason = "Connection refused: /private/tmp/remote";
            let failed = failure_preview("push", reason);
            let partial = partial_preview("push", reason);
            let unknown = unknown_preview("push", reason);
            assert_ne!(failed, partial);
            assert_ne!(partial, unknown);
            assert_ne!(failed, unknown);
            let (partial_meaning, uncertain_meaning, no_retry) = match language {
                Lang::En => ("partly", "could not be confirmed", "Do not retry"),
                Lang::Ja => ("一部のみ", "確認できません", "再試行しない"),
            };
            assert!(partial.contains(partial_meaning), "{partial}");
            assert!(unknown.contains(uncertain_meaning), "{unknown}");
            assert!(unknown.contains(no_retry), "{unknown}");
            assert!(!failed.contains(partial_meaning), "{failed}");
            assert!(!failed.contains(uncertain_meaning), "{failed}");
        }
        set_lang_no_persist(previous);
    }

    #[test]
    fn action_labels_remain_separate_from_status_and_recording_reason() {
        use crate::i18n::{set_lang_no_persist, tests::LOCK};
        let _guard = LOCK.lock();
        let previous = lang();
        for language in [Lang::En, Lang::Ja] {
            set_lang_no_persist(language);
            for op in ["delete-branch", "conflict-save:merge", "sync-to-remote"] {
                let prefix = format!("{}: ", operation_label(op));
                let reason = "remote: example_reason";
                for shown in [
                    failed(op, reason),
                    partial(op, reason),
                    progress(op),
                    onto(op, "main"),
                ] {
                    assert!(shown.starts_with(&prefix), "{shown}");
                    assert_eq!(diagnostic(&shown), shown, "toast must preserve footer text");
                }
                let notice = recording_failed(
                    "/repos/project",
                    op,
                    crate::i18n::op::RecordingOutcome::Failed,
                    reason,
                );
                assert!(notice.contains(&prefix));
                assert!(notice.contains("/repos/project"));
                assert!(notice.contains(reason));
                assert!(!notice.contains(op), "internal operation name: {notice}");
            }
        }
        set_lang_no_persist(previous);
    }

    #[test]
    fn recording_failure_preserves_each_operation_result_in_both_languages() {
        use crate::i18n::op::RecordingOutcome;
        use crate::i18n::{set_lang_no_persist, tests::LOCK};
        let _guard = LOCK.lock();
        let previous = lang();
        for language in [Lang::En, Lang::Ja] {
            set_lang_no_persist(language);
            for (outcome, en, ja) in [
                (
                    RecordingOutcome::Succeeded,
                    "operation completed",
                    "操作は完了",
                ),
                (RecordingOutcome::Failed, "operation failed", "操作は失敗"),
                (RecordingOutcome::Refused, "not started", "開始されません"),
                (
                    RecordingOutcome::Partial,
                    "partly completed",
                    "一部のみ完了",
                ),
                (
                    RecordingOutcome::Unknown,
                    "could not be confirmed",
                    "結果を確認できません",
                ),
            ] {
                let shown = recording_failed("/repo", "fetch", outcome, "fixture record error");
                let expected = if language == Lang::En { en } else { ja };
                assert!(shown.contains(expected), "{shown}");
                assert!(shown.contains("fixture record error"), "{shown}");
                assert!(shown.contains("/repo"), "{shown}");
                assert!(
                    shown.lines().count() >= 3,
                    "separate result and recording error"
                );
                if matches!(
                    outcome,
                    RecordingOutcome::Failed | RecordingOutcome::Unknown
                ) {
                    assert!(!shown.contains("operation completed"), "{shown}");
                    assert!(!shown.contains("操作は完了"), "{shown}");
                }
            }
        }
        set_lang_no_persist(previous);
    }

    #[test]
    fn recording_failure_warns_about_recovery_only_for_changed_outcomes() {
        use crate::i18n::op::RecordingOutcome;
        use crate::i18n::{set_lang_no_persist, tests::LOCK};
        let _guard = LOCK.lock();
        let previous = lang();
        for language in [Lang::En, Lang::Ja] {
            set_lang_no_persist(language);
            let (undo, restore) = match language {
                Lang::En => ("cannot undo", "restore from"),
                Lang::Ja => ("取り消せません", "復元もできません"),
            };
            for outcome in [
                RecordingOutcome::Succeeded,
                RecordingOutcome::Partial,
                RecordingOutcome::Failed,
                RecordingOutcome::Refused,
                RecordingOutcome::Unknown,
            ] {
                let shown = recording_failed("/repo", "fetch", outcome, "fixture record error");
                let changed = matches!(
                    outcome,
                    RecordingOutcome::Succeeded | RecordingOutcome::Partial
                );
                assert_eq!(shown.contains(undo), changed, "{outcome:?}: {shown}");
                assert_eq!(shown.contains(restore), changed, "{outcome:?}: {shown}");
            }
        }
        set_lang_no_persist(previous);
    }

    #[test]
    fn count_summaries_agree_with_singular_and_plural_counts() {
        use crate::i18n::{set_lang_no_persist, tests::LOCK};
        let _guard = LOCK.lock();
        let previous = lang();
        set_lang_no_persist(Lang::En);
        for count in [0, 1, 2] {
            for set_upstream in [false, true] {
                let outcome = OperationOutcome::Push(kagi_domain::plan::PushOutcome {
                    pushed: count,
                    set_upstream,
                });
                let shown = push_result(&outcome);
                assert!(shown.contains(&count.to_string()));
                assert_eq!(shown.contains("commits"), count != 1);
            }
            assert_eq!(cleanup(count).contains("branches"), count != 1);
            assert_eq!(pruned_worktrees(count).contains("entries"), count != 1);
            assert_eq!(discarded(count, None).contains("files"), count != 1);
        }
        set_lang_no_persist(previous);
    }

    #[test]
    fn branch_display_preserves_user_names() {
        use crate::i18n::{set_lang_no_persist, tests::LOCK};
        let branch = "feature/repo_session-unavailable-日本語";
        let _guard = LOCK.lock();
        let previous = lang();
        for language in [Lang::En, Lang::Ja] {
            set_lang_no_persist(language);
            for shown in [
                branch_progress("branch-pull-ff", branch),
                branch_result(branch, &OperationOutcome::Pull(PullOutcome::UpToDate)),
                pull_summary(&format!("fast-forward to {branch}")),
            ] {
                assert!(shown.contains(branch), "{shown}");
                assert_eq!(
                    diagnostic(&shown),
                    shown,
                    "preserve the target in presentation"
                );
            }
        }
        set_lang_no_persist(previous);
    }

    #[test]
    fn successful_pull_primary_does_not_replay_unrecognized_backend_output() {
        use crate::i18n::{set_lang_no_persist, tests::LOCK};
        let _guard = LOCK.lock();
        let previous = lang();
        for language in [Lang::En, Lang::Ja] {
            set_lang_no_persist(language);
            let first = pull_summary("create mode 100644 /tmp/private-first.txt");
            let second = pull_summary("unrecognized backend stdout /tmp/private-second.txt");
            assert_eq!(first, second, "unrecognized stdout is not primary UI copy");
            assert!(first.starts_with(&format!("{}: ", operation_label("pull"))));
            let unchanged = pull_summary("already up to date");
            for cli_summary in ["Already up to date.", "Already up-to-date."] {
                assert_eq!(pull_summary(cli_summary), unchanged);
            }
            assert_ne!(
                unchanged, first,
                "known no-change is not generic completion"
            );
        }
        set_lang_no_persist(previous);
    }

    #[test]
    fn uncertain_results_are_not_success_summaries() {
        let _guard = crate::i18n::tests::LOCK.lock();
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
