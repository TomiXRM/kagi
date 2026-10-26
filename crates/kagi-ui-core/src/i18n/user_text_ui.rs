//! Known UI diagnostics kept separate from raw Git output and user content.
use super::{diagnostic as present, operation_label, text};
use crate::i18n::{lang, op, Lang, Msg};

pub(super) fn diagnostic(value: &str) -> Option<String> {
    for language in [Lang::En, Lang::Ja] {
        let message = if value == op::pull_identity_changed_contract(language) {
            Some(Msg::PullAutoStashIdentityChanged)
        } else if value == op::pull_files_changed_contract(language) {
            Some(Msg::PullAutoStashPlanStale)
        } else if value == op::pull_restore_changed_contract(language) {
            Some(Msg::PullAutoStashRestoreChanged)
        } else if value == op::remote_completion_pending_contract(language) {
            Some(Msg::RemoteOpAwaitingCompletion)
        } else if value == op::terminal_lock_changed_contract(language) {
            Some(Msg::TerminalAutoLockStaleOwner)
        } else {
            None
        };
        if let Some(message) = message {
            return Some(message.t().to_string());
        }
    }
    if value == "NeedsReconcile" {
        return Some(Msg::AppReconcileRequired.t().into());
    }
    if value == "Busy" {
        return Some(Msg::OpInProgress.t().into());
    }
    if value == "StaleApproval" {
        return Some(
            text(
                "The operation changed. Review it again before continuing.",
                "操作の内容が変わりました。続行する前にもう一度確認してください。",
            )
            .into(),
        );
    }
    if (value.starts_with("Identity(\"") && value.ends_with("\")"))
        || value.starts_with("repository write lock: ")
    {
        // The receipt retains the full cause for Details/Copy; its summary
        // must not expose a Debug wrapper, raw I/O cause or absolute path.
        return Some(
            text(
                "Could not start the operation. Review its details in Operation Log.",
                "操作を開始できませんでした。操作ログで詳細を確認してください。",
            )
            .into(),
        );
    }
    let fixed = match value {
        "worktree identity changed; reopen the repository" => Some((
            "The repository changed. Reopen it before continuing.",
            "repository が変更されました。開き直してから続けてください。",
        )),
        "Planning merge…" => Some(("Preparing merge preview…", "merge のプレビューを準備中…")),
        "Force-with-lease pushing…" => Some(("Pushing with --force-with-lease…", "--force-with-lease で push 中…")),
        "pull: unexpected outcome" | "stash pop: unexpected outcome" => Some((
            "Could not confirm the result. Check your files and stashes before retrying.",
            "結果を確認できませんでした。再試行する前にファイルと stash を確認してください。",
        )),
        "create-pr: remote host not recognized (github.com / gitlab.com / bitbucket.org only)" => Some((
            "Creating pull requests is supported only for github.com, gitlab.com, and bitbucket.org.",
            "pull request を作成できるのは github.com、gitlab.com、bitbucket.org のみです。",
        )),
        "replay: no current branch" => Some((
            "Switch to a branch before replaying commits.",
            "commit を replay する前に branch に切り替えてください。",
        )),
        "session unavailable" | "repository session unavailable" | "repo session unavailable" => Some((
            "Repository unavailable. Reopen it and try again.",
            "repository を利用できません。開き直してからやり直してください。",
        )),
        "stale conflict action was ignored" => Some((
            "The conflict changed. Review it before trying again.",
            "conflict の状態が変わりました。内容を確認してからやり直してください。",
        )),
        "merge commit: unexpected outcome" => Some((
            "Could not confirm the merge commit. Check the repository before retrying.",
            "merge commit を確認できませんでした。再試行する前に repository を確認してください。",
        )),
        "push: unexpected outcome" => Some((
            "Could not confirm the push. Check the remote before retrying.",
            "push を確認できませんでした。再試行する前に remote を確認してください。",
        )),
        "unexpected merge outcome" | "unexpected outcome" => Some((
            "Could not confirm the result. Check the branch before retrying.",
            "結果を確認できませんでした。再試行する前に branch を確認してください。",
        )),
        "save task unwound; recording failed — inspect the operation notice" => Some((
            "The save may not have completed, and its result could not be recorded. Check the operation notice.",
            "保存が完了していない可能性があり、結果も記録できませんでした。操作の通知を確認してください。",
        )),
        "save task unwound; outcome unknown — inspect the operation notice" => Some((
            "The save may not have completed. Check the operation notice.",
            "保存が完了していない可能性があります。操作の通知を確認してください。",
        )),
        "corrupt pr-viewed file could not be moved aside; not overwritten" => Some((
            "Saved viewed marks are damaged and could not be updated. Your existing data was left unchanged.",
            "保存済みの確認状態が壊れているため、更新できませんでした。既存のデータは変更していません。",
        )),
        "mmdc failed" => Some((
            "Could not display this diagram; showing its source instead.",
            "図を表示できないため、代わりにソースを表示しています。",
        )),
        "[kagi] refreshed (external change)" => Some(("Refreshed", "更新しました")),
        "[kagi] refresh skipped (snapshot failed)" => Some((
            "Could not refresh. Try Refresh again.",
            "更新できませんでした。もう一度「更新」を実行してください。",
        )),
        _ => None,
    };
    if let Some((en, ja)) = fixed {
        return Some(text(en, ja).into());
    }
    if value.ends_with(": repo session unavailable") {
        let op = value.strip_suffix(": repo session unavailable")?;
        if super::known_operation(op) {
            return diagnostic("repo session unavailable");
        }
    }
    if value == "Your changes are stored in the stash entry named `kagi: auto-stash before pull`, but Kagi could not verify its identity. Pull was not started; inspect the stash before continuing."
        || value == "変更内容は `kagi: auto-stash before pull` という名前の stash に保存されていますが、Kagi はその識別情報を確認できませんでした。Pull は開始していません。続行前に stash を確認してください。" {
        return Some(super::super::auto_stash_identity_unverified().into());
    }
    if value.starts_with("External merge tool launch is not wired yet (") && value.ends_with(").") {
        return Some(
            text(
                "External merge tools are not available.",
                "外部 merge ツールは利用できません。",
            )
            .into(),
        );
    }
    if value.starts_with("kagi:auto:") {
        return Some(
            text(
                "Locked while a Kagi terminal was open.",
                "Kagi のターミナル使用中に lock されました。",
            )
            .into(),
        );
    }
    if let Some(target) = value.strip_prefix("no storage name for ") {
        return Some(match lang() {
            Lang::En => format!("Could not save viewed marks for {target}."),
            Lang::Ja => format!("{target} の確認済み状態を保存できませんでした。"),
        });
    }
    if let Some(reason) =
        value.strip_suffix("; process termination is unconfirmed — do not retry this operation")
    {
        return Some(format!(
            "{}: {}",
            text(
                "The operation may still be running. Do not retry it yet.",
                "操作がまだ実行中の可能性があります。まだ再試行しないでください。",
            ),
            present(reason),
        ));
    }
    if let Some(reason) = value.strip_prefix("Restore plan ref rows could not be decoded: ") {
        return Some(format!(
            "{}: {}",
            text(
                "Could not read the restoration details",
                "復元の詳細を読み取れませんでした",
            ),
            present(reason),
        ));
    }
    if let Some(reason) = value.strip_prefix("Repository changed since planning: ") {
        let reason = present(reason);
        return Some(match lang() {
            Lang::En => {
                format!("The repository changed. Review the changes before discarding: {reason}")
            }
            Lang::Ja => format!(
                "repository が変更されました。破棄する前に変更内容を確認してください: {reason}"
            ),
        });
    }
    for prefix in ["openpty: ", "take_writer: ", "try_clone_reader: "] {
        if let Some(reason) = value.strip_prefix(prefix) {
            let reason = present(reason);
            return Some(match lang() {
                Lang::En => format!("Could not open the terminal: {reason}"),
                Lang::Ja => format!("ターミナルを開けませんでした: {reason}"),
            });
        }
    }
    if let Some(rest) = value.strip_prefix("spawn '") {
        if let Some((shell, reason)) = rest.split_once("': ") {
            let reason = present(reason);
            return Some(match lang() {
                Lang::En => format!("Could not start '{shell}': {reason}"),
                Lang::Ja => format!("「{shell}」を起動できませんでした: {reason}"),
            });
        }
    }
    for (prefix, en, ja) in [
        (
            "snapshot error: ",
            "Could not read the repository",
            "repository を読み込めませんでした",
        ),
        (
            "repo open error: ",
            "Could not open the repository",
            "repository を開けませんでした",
        ),
        (
            "Refresh failed: ",
            "Could not refresh",
            "更新できませんでした",
        ),
        (
            "Load more failed: ",
            "Could not load more commits",
            "追加の commit を読み込めませんでした",
        ),
    ] {
        if let Some(reason) = value.strip_prefix(prefix) {
            return Some(format!("{}: {}", text(en, ja), present(reason)));
        }
    }
    for suffix in [
        " plan failed unexpectedly",
        ": operation failed unexpectedly",
    ] {
        if let Some(op) = value.strip_suffix(suffix) {
            let label = operation_label(op);
            if super::known_operation(op) {
                return Some(match lang() {
                    Lang::En => format!("{label}: Could not prepare. Please try again."),
                    Lang::Ja => format!("{label}: 準備できませんでした。もう一度お試しください。"),
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::tests::LOCK;

    #[test]
    fn restore_decode_diagnostic_preserves_the_presented_reason() {
        let _guard = LOCK.lock();
        for reason in [
            "invalid ref row at byte 42",
            "git error: index.write() failed: denied",
        ] {
            let shown = diagnostic(&format!(
                "Restore plan ref rows could not be decoded: {reason}"
            ))
            .unwrap();
            assert!(shown.ends_with(present(reason).as_ref()), "{shown}");
            assert!(!shown.contains("Reopen the Operation Log"), "{shown}");
        }
    }

    #[test]
    fn unconfirmed_termination_diagnostic_preserves_the_presented_reason() {
        let _guard = LOCK.lock();
        let reason = "git update-ref: child 42 did not terminate";
        let shown = diagnostic(&format!(
            "{reason}; process termination is unconfirmed — do not retry this operation"
        ))
        .unwrap();
        assert!(shown.ends_with(present(reason).as_ref()), "{shown}");
        assert!(shown.contains(text("Do not retry it yet.", "まだ再試行しないでください。",)));
    }

    #[test]
    fn every_admission_error_has_a_localized_summary() {
        let _guard = LOCK.lock();
        let previous = lang();
        for language in [Lang::En, Lang::Ja] {
            crate::i18n::set_lang_no_persist(language);
            let cases = [
                ("Busy", Msg::OpInProgress.t()),
                ("NeedsReconcile", Msg::AppReconcileRequired.t()),
                (
                    "StaleApproval",
                    text(
                        "The operation changed. Review it again before continuing.",
                        "操作の内容が変わりました。続行する前にもう一度確認してください。",
                    ),
                ),
                (
                    r#"Identity("repository write lock: /private/repo: permission denied")"#,
                    text(
                        "Could not start the operation. Review its details in Operation Log.",
                        "操作を開始できませんでした。操作ログで詳細を確認してください。",
                    ),
                ),
                (
                    kagi_domain::advice_template_en!(ConflictsRepositoryBusy),
                    Msg::AdviceConflictsRepositoryBusy.t(),
                ),
            ];
            for (raw, expected) in cases {
                let shown = present(raw);
                assert_eq!(shown.as_ref(), expected);
                assert!(!shown.contains("/private/") && !shown.contains("Identity("));
            }
            let shown =
                present("git error: repository write lock: /private/repo: permission denied");
            assert!(!shown.contains("/private/") && !shown.contains("permission denied"));
            assert!(shown.contains(text(
                "Could not start the operation. Review its details in Operation Log.",
                "操作を開始できませんでした。操作ログで詳細を確認してください。",
            )));
            let shown = super::super::state_text("unobserved");
            assert_eq!(
                shown.as_ref(),
                text("State unavailable", "状態を確認できません")
            );
        }
        crate::i18n::set_lang_no_persist(previous);
    }
}
