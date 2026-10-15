//! Display-only translations of Kagi's durable Git diagnostics (#1588).
//! Match producer formats, not words: paths, branch names and Git/OS causes
//! are data. Never use these translations for execution or receipt persistence.
use crate::i18n::{lang, Lang};

fn localized<'a>(en: &'a str, ja: &'a str) -> &'a str {
    match lang() {
        Lang::En => en,
        Lang::Ja => ja,
    }
}

macro_rules! text {
    ($en:literal, $ja:literal $(, $arg:expr)* $(,)?) => {
        match lang() {
            Lang::En => format!($en $(, $arg)*),
            Lang::Ja => format!($ja $(, $arg)*),
        }
    };
}

#[path = "user_text_git_messages.rs"]
mod messages;
use messages::{CAUSES, EXACT};

pub(super) fn diagnostic(text: &str) -> Option<String> {
    if let Some((_, en, ja)) = EXACT.iter().find(|(raw, _, _)| *raw == text) {
        return Some(localized(en, ja).to_owned());
    }
    if text
        .strip_prefix("kagi:auto:")
        .is_some_and(|owner| !owner.is_empty() && !owner.chars().any(char::is_whitespace))
    {
        return Some(
            localized(
                "Locked while a Kagi terminal was open.",
                "Kagi のターミナルを使用中だったためロックされています。",
            )
            .to_owned(),
        );
    }
    // Handle outer safety qualifiers before any cause prefix can consume them.
    if let Some(reason) = text.strip_suffix("; the repository was not re-read") {
        return Some(text!(
            "{}. The result is unconfirmed. Do not retry until you have checked the repository.",
            "{}。結果を確認できません。repository を確認するまで再実行しないでください。",
            super::diagnostic(reason)
        ));
    }
    if let Some(reason) = text.strip_suffix(" (the local child could not be stopped)") {
        return Some(text!(
            "{}. Git may still be running. Do not retry yet.",
            "{}。Git がまだ動作中の可能性があります。まだ再実行しないでください。",
            super::diagnostic(reason)
        ));
    }
    if let Some(reason) = text.strip_prefix("git error: ") {
        return Some(text!("Git: {}", "Git: {}", super::diagnostic(reason)));
    }
    for &(prefix, en, ja) in CAUSES {
        if let Some(reason) = text.strip_prefix(prefix) {
            return Some(format!(
                "{}: {}",
                localized(en, ja),
                super::diagnostic(reason)
            ));
        }
    }
    paths(text)
        .or_else(|| planning(text))
        .or_else(|| storage(text))
        .or_else(|| replay_parser(text))
        .or_else(|| safety(text))
}

fn paths(text: &str) -> Option<String> {
    for (prefix, en, ja) in [
        ("path not found: ", "Path not found", "パスが見つかりません"),
        (
            "not a git repository: ",
            "Not a Git repository",
            "Git repository ではありません",
        ),
        (
            "bare repository (no working tree): ",
            "This repository has no working files and cannot be opened here",
            "作業ファイルのない repository はここでは開けません",
        ),
        (
            "repository is not trusted for writes (owned by another user): ",
            "Approve this repository before changing it; it belongs to another user",
            "別のユーザーが所有する repository です。変更する前に信頼を確認してください",
        ),
        (
            "non-UTF-8 path cannot be expressed as a git pathspec: ",
            "This filename is not supported for this operation",
            "この操作ではこのファイル名を扱えません",
        ),
    ] {
        if let Some(path) = text.strip_prefix(prefix) {
            return Some(format!("{}: {path}", localized(en, ja)));
        }
    }
    if let Some(path) = text.strip_prefix("no resolution draft for ") {
        return Some(text!(
            "Choose a side or edit {path} before saving.",
            "保存する前に {path} の採用する側を選ぶか、内容を編集してください。"
        ));
    }
    if let Some(path) = between(text, "no resolution for ", " — re-plan before executing") {
        return Some(text!(
            "Resolve {path} before continuing.",
            "続行する前に {path} の conflict を解決してください。"
        ));
    }
    for (prefix, en, ja) in [
        ("index.add_path(", "Could not stage", "stage できません"),
        (
            "index.remove_path(",
            "Could not stage the deletion of",
            "削除を stage できません",
        ),
    ] {
        if let Some((path, reason)) = text
            .strip_prefix(prefix)
            .and_then(|s| s.split_once(") failed: "))
        {
            return Some(format!(
                "{} {path}: {}",
                localized(en, ja),
                super::diagnostic(reason)
            ));
        }
    }
    if let Some((path, reason)) = text
        .strip_prefix("verify ")
        .and_then(|s| s.split_once(" failed: "))
    {
        return Some(text!(
            "Could not confirm that {path} was saved: {}. Review it before continuing.",
            "{path} を保存できたか確認できません: {}。続行する前に確認してください。",
            super::diagnostic(reason)
        ));
    }
    for suffix in [
        " bytes, blob, or mode differ after save",
        " raw OID or mode differs after save",
    ] {
        if let Some(path) = text.strip_suffix(suffix) {
            return Some(text!("The saved result for {path} does not match your resolution. Review it before continuing.", "{path} の保存結果が解決内容と一致しません。続行する前に確認してください。"));
        }
    }
    if let Some(raw) = between(text, "rename ", " (files before this point were already resolved; the index was not written, so the conflict is still recorded — re-resolve and retry)") {
        let (paths, reason) = raw.split_once(" failed: ")?;
        let (_, path) = paths.split_once(" -> ")?;
        return Some(text!("Could not save {path}: {}. Some other files were saved. Review all resolutions before retrying.", "{path} を保存できません: {}。ほかの一部のファイルは保存されました。再実行する前にすべての解決内容を確認してください。", super::diagnostic(reason)));
    }
    suggestion_and_discard(text)
}

fn suggestion_and_discard(text: &str) -> Option<String> {
    for (prefix, en, ja) in [
        (
            "apply-suggestion refused: cannot read '",
            "Could not read the file to apply the suggestion",
            "提案を適用するファイルを読み取れません",
        ),
        (
            "apply-suggestion aborted: backup failed for '",
            "Could not back up the file; the suggestion was not applied",
            "ファイルを backup できないため提案を適用しませんでした",
        ),
        (
            "apply-suggestion: failed to write '",
            "Could not save the suggestion",
            "提案を保存できません",
        ),
        (
            "apply-suggestion verify: cannot re-read '",
            "Could not confirm the saved suggestion. Check the file before continuing",
            "提案が保存されたか確認できません。続行する前にファイルを確認してください",
        ),
        (
            "discard verify: inspect '",
            "Could not confirm the discard. Check the file before continuing",
            "破棄できたか確認できません。続行する前にファイルを確認してください",
        ),
        (
            "discard verify: read link '",
            "Could not confirm the discarded link. Check the file before continuing",
            "link を破棄できたか確認できません。続行する前にファイルを確認してください",
        ),
    ] {
        if let Some((path, reason)) = text.strip_prefix(prefix).and_then(|s| s.split_once("': ")) {
            return Some(format!(
                "{}: {path}: {}",
                localized(en, ja),
                super::diagnostic(reason)
            ));
        }
    }
    for (prefix, suffix, en, ja) in [
        ("apply-suggestion refused: '", "' changed since the suggestion was reviewed (stale range) — refusing to edit the wrong lines", "The file changed. Review the suggestion again", "ファイルが変わりました。提案を確認し直してください"),
        ("apply-suggestion verify failed: '", "' does not match the applied content", "Could not confirm the saved suggestion. Check the file before continuing", "提案が保存されたか確認できません。続行する前にファイルを確認してください"),
        ("discard verify: non-UTF-8 link target '", "'", "Could not check this link's filename. Review the discard before continuing", "link のファイル名を確認できません。続行する前に破棄の結果を確認してください"),
        ("hunk staging verify failed: the index entry for '", "' did not change", "Could not confirm that the change was staged. Review staged changes before continuing", "変更を stage できたか確認できません。続行する前に staged の変更を確認してください"),
    ] {
        if let Some(path) = between(text, prefix, suffix) {
            return Some(format!("{}: {path}", localized(en, ja)));
        }
    }
    if let Some(raw) = between(text, "apply-suggestion: range ", "'") {
        let (range, path) = raw.split_once(" out of bounds in '")?;
        let (start, end) = range.split_once('-')?;
        if number(start) && number(end) {
            return Some(text!("Lines {start}–{end} are no longer available in '{path}'. Review the suggestion again.", "「{path}」に行 {start}–{end} がありません。提案を確認し直してください。"));
        }
    }
    if let Some((count, paths)) = text
        .strip_prefix("discard verify failed: ")
        .and_then(|s| s.split_once(" target(s) not discarded: "))
    {
        if number(count) {
            return Some(text!("Could not confirm that {count} files were discarded: {paths}. Review them before continuing.", "{count} 件のファイルを破棄できたか確認できません: {paths}。続行する前に確認してください。"));
        }
    }
    None
}

fn planning(text: &str) -> Option<String> {
    for (prefix, en, ja) in [
        (
            "discard refused: plan has ",
            "These files cannot be discarded. Review the warnings.",
            "このファイルは破棄できません。注意事項を確認してください。",
        ),
        (
            "apply-suggestion refused: plan has ",
            "This suggestion cannot be applied. Review the warnings.",
            "この提案は適用できません。注意事項を確認してください。",
        ),
        (
            "remove-worktree refused: plan has ",
            "This worktree cannot be removed. Review the warnings.",
            "この worktree は削除できません。注意事項を確認してください。",
        ),
    ] {
        if between(text, prefix, " blocker(s)").is_some_and(number) {
            return Some(localized(en, ja).to_owned());
        }
    }
    if let Some(op) = between(
        text,
        "sync-to-remote refused at preflight: ",
        " is in progress",
    ) {
        return Some(text!(
            "Finish or abort {op} before syncing.",
            "同期の前に {op} を完了または中止してください。"
        ));
    }
    for prefix in ["op-revert refused: ", "restore-to-point refused: "] {
        if let Some(reason) = text.strip_prefix(prefix) {
            let reason = reason.strip_prefix("at preflight: ").unwrap_or(reason);
            if matches!(
                reason,
                "plan has blockers"
                    | "the plan carries no ref restores"
                    | "not an Operation Log restore"
                    | "the local refs changed since planning; please re-plan"
            ) {
                return Some(
                    localized(
                        "Could not restore the selected operation. Review the restore again.",
                        "選択した操作を復元できません。復元内容を確認し直してください。",
                    )
                    .to_owned(),
                );
            }
            return Some(text!(
                "Could not restore the selected operation: {}. Review the restore again.",
                "選択した操作を復元できません: {}。復元内容を確認し直してください。",
                super::diagnostic(reason)
            ));
        }
    }
    if let Some(raw) = between(
        text,
        "Repository state changed since planning. HEAD was ",
        ". Please re-plan before proceeding.",
    ) {
        let (old, new) = raw.split_once(" at plan time but is now ")?;
        return Some(text!(
            "The current branch or commit changed: {} → {}. Review the operation again.",
            "現在の branch または commit が変わりました: {} → {}。操作内容を確認し直してください。",
            super::state_text(old),
            super::state_text(new)
        ));
    }
    for prefix in [
        "git replay printed no update for refs/heads/",
        "plan carries no update for ",
    ] {
        if let Some(branch) = text.strip_prefix(prefix) {
            return Some(text!(
                "Could not confirm replay for '{branch}'. Refresh and review it again.",
                "「{branch}」の replay を確認できません。更新して確認し直してください。"
            ));
        }
    }
    if let Some(raw) = between(text, "git replay saw '", "; please re-plan") {
        let (branch, values) = raw.split_once("' at ")?;
        let (now, old) = values.split_once(" but the plan read ")?;
        return Some(text!(
            "Branch '{branch}' changed: {old} → {now}. Review replay again.",
            "branch「{branch}」が変わりました: {old} → {now}。replay を確認し直してください。"
        ));
    }
    if let Some(raw) = between(
        text,
        "replay-onto refused at preflight: '",
        " with uncommitted changes",
    ) {
        let (branch, path) = raw.split_once("' is checked out in ")?;
        return Some(text!(
            "Save changes in '{path}' before replaying '{branch}'.",
            "「{branch}」を replay する前に「{path}」の変更を保存してください。"
        ));
    }
    if let Some(run) = text.strip_prefix("pre-remove command cannot run under the headless harness, so cleanup cannot be verified — aborting removal: ") {
        return Some(text!("The cleanup command could not run. The worktree was not removed: {run}", "後処理コマンドを実行できなかったため、worktree は削除していません: {run}"));
    }
    None
}

fn storage(text: &str) -> Option<String> {
    for prefix in [
        "draft: serialize failed: ",
        "draft: temporary file: ",
        "draft: write failed: ",
    ] {
        if let Some(reason) = text.strip_prefix(prefix) {
            return Some(text!(
                "Could not save your draft: {}. Keep this window open until it is saved.",
                "下書きを保存できません: {}。保存されるまでこのウィンドウを閉じないでください。",
                super::diagnostic(reason)
            ));
        }
    }
    for (prefix, separator, en, ja) in [
        (
            "draft: mkdir failed for ",
            ": ",
            "Could not save your draft. Keep this window open until it is saved",
            "下書きを保存できません。保存されるまでこのウィンドウを閉じないでください",
        ),
        (
            "draft: replace ",
            ": ",
            "Could not save your draft. Keep this window open until it is saved",
            "下書きを保存できません。保存されるまでこのウィンドウを閉じないでください",
        ),
        (
            "draft: delete failed for ",
            ": ",
            "Could not remove the saved draft",
            "保存済みの下書きを削除できません",
        ),
        (
            "draft: read ",
            " before replacing it: ",
            "Could not preserve the previous draft. Your new draft has not been saved",
            "以前の下書きを保護できません。新しい下書きはまだ保存されていません",
        ),
        (
            "draft: move unreadable ",
            " aside: ",
            "Could not preserve the previous draft. Your new draft has not been saved",
            "以前の下書きを保護できません。新しい下書きはまだ保存されていません",
        ),
        (
            "draft: reserve ",
            ": ",
            "Could not preserve the previous draft. Your new draft has not been saved",
            "以前の下書きを保護できません。新しい下書きはまだ保存されていません",
        ),
        (
            "oplog: mkdir failed for ",
            ": ",
            "Could not save this operation in Operation Log",
            "この操作を Operation Log に記録できません",
        ),
        (
            "oplog: open failed for ",
            ": ",
            "Could not save this operation in Operation Log",
            "この操作を Operation Log に記録できません",
        ),
        (
            "oplog: write failed for ",
            ": ",
            "Could not save this operation in Operation Log",
            "この操作を Operation Log に記録できません",
        ),
    ] {
        if let Some((path, reason)) = text
            .strip_prefix(prefix)
            .and_then(|s| s.split_once(separator))
        {
            return Some(format!(
                "{}: {path}: {}",
                localized(en, ja),
                super::diagnostic(reason)
            ));
        }
    }
    if let Some(path) = between(text, "draft: ", " has no name") {
        return Some(text!(
            "Could not preserve the previous draft at {path}. Your new draft has not been saved.",
            "{path} の以前の下書きを保護できません。新しい下書きはまだ保存されていません。"
        ));
    }
    None
}

fn safety(text: &str) -> Option<String> {
    if let Some(raw) = text.strip_prefix("stage=") {
        let (_, raw) = raw.split_once("; verification=")?;
        let (_, raw) = raw.split_once("; termination_unknown=")?;
        let (unknown, observations) = raw.split_once("; observations=")?;
        if !matches!(unknown, "true" | "false") {
            return None;
        }
        let reason = after_debug_list(observations)?.strip_prefix("; ")?;
        return Some(text!("Worktree removal could not be completed or confirmed: {}. Do not retry until you have checked the worktree.", "worktree の削除を完了または確認できません: {}。worktree を確認するまで再実行しないでください。", super::diagnostic(reason)));
    }
    if let Some(raw) = text.strip_prefix("auto-unlock-worktree refused: the lock was no longer this terminal's, and a newer lock appeared before it could be put back (") {
        let (reason, path) = raw.split_once("); the moved lock is kept at ")?;
        return Some(text!("The worktree's lock changed. Check it before continuing. The previous lock is saved at {path}: {}", "worktree のロックが変わりました。続行する前に確認してください。以前のロックの保存先: {path}: {}", super::diagnostic(reason)));
    }
    if let Some(raw) = between(text, "none of the ", " as first parent with kagi's message") {
        let (count, head) = raw.split_once(" stash entries added during the push has HEAD ")?;
        if number(count) {
            return Some(text!("Could not identify the new stash among {count} entries (HEAD {head}). Do not retry until you have checked your stashes.", "追加された {count} 件から新しい stash を確認できません (HEAD {head})。stash 一覧を確認するまで再実行しないでください。"));
        }
    }
    if let Some(raw) = text.strip_suffix(" stash entries added during the push are indistinguishable from kagi's (same HEAD and message)") {
        let (matched, added) = raw.split_once(" of the ")?;
        if number(matched) && number(added) {
            return Some(text!("{matched} of {added} new stashes match this operation. Do not retry until you have checked your stashes.", "新しい stash {added} 件のうち {matched} 件がこの操作に一致します。stash 一覧を確認するまで再実行しないでください。"));
        }
    }
    if let Some(raw) = text.strip_prefix("refs/kagi/backups/") {
        for suffix in [
            " does not hold the pre-sync tip",
            " does not hold the work backup",
        ] {
            if let Some(reference) = raw.strip_suffix(suffix) {
                return Some(text!("Could not confirm the recovery backup ({reference}). Keep your files and review the operation before continuing.", "復旧用 backup ({reference}) を確認できません。ファイルを保持し、続行する前に操作内容を確認してください。"));
            }
        }
        if let Some((reference, reason)) = raw.split_once(": ") {
            return Some(text!("Could not confirm the recovery backup ({reference}): {}. Check it before continuing.", "復旧用 backup ({reference}) を確認できません: {}。続行する前に確認してください。", super::diagnostic(reason)));
        }
    }
    process(text)
}

fn replay_parser(text: &str) -> Option<String> {
    if let Some(raw) = text.strip_prefix("unsupported ref update verb ") {
        let (verb, line) = raw.split_once(": ")?;
        return Some(text!("Git returned an unsupported instruction {verb}: {line}. Review replay again.", "Git から対応していない指示 {verb} が返されました: {line}。replay を確認し直してください。"));
    }
    if let Some(raw) = text.strip_prefix("bad object id ") {
        let (oid, line) = raw.split_once(" in ")?;
        return Some(text!("Git returned an invalid object ID {oid}: {line}. Review replay again.", "Git から無効な object ID {oid} が返されました: {line}。replay を確認し直してください。"));
    }
    None
}

fn process(text: &str) -> Option<String> {
    // CLI wraps ProcStop/ProcIo with `git <arguments>`. Match only their
    // known delimiters, never reinterpret arbitrary stderr or command text.
    if let Some(raw) = text.strip_prefix("git ") {
        if let Some((command, reason)) = raw.split_once(" wait failed: ") {
            return Some(text!("Could not confirm that Git finished ({command}): {}. Do not retry yet.", "Git の処理 ({command}) が完了したか確認できません: {}。まだ再実行しないでください。", super::diagnostic(reason)));
        }
        if let Some((command, seconds)) = raw
            .split_once(" timed out after ")
            .and_then(|(c, s)| s.strip_suffix('s').map(|s| (c, s)))
        {
            if number(seconds) {
                return Some(text!("Git did not finish within {seconds} seconds ({command}). Do not retry until you have checked the result.", "Git の処理 ({command}) が {seconds} 秒以内に完了しませんでした。結果を確認するまで再実行しないでください。"));
            }
        }
        if let Some((command, reason)) = raw.split_once(": ") {
            if reason.starts_with("output collection did not finish — ")
                || reason == "an output collector thread panicked"
                || reason.starts_with("input was not fully written: ")
                || reason.starts_with("output could not be read: ")
            {
                return Some(format!("Git ({command}): {}", super::diagnostic(reason)));
            }
        }
    }
    if let Some(seconds) = between(text, "timed out after ", "s") {
        if number(seconds) {
            return Some(text!("Git did not finish within {seconds} seconds. Do not retry until you have checked the result.", "Git が {seconds} 秒以内に完了しませんでした。結果を確認するまで再実行しないでください。"));
        }
    }
    None
}

fn between<'a>(text: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    text.strip_prefix(prefix)?.strip_suffix(suffix)
}

fn number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

// Rust Debug string lists can contain `]; ` in a quoted user path. Walk the
// quote/escape structure rather than truncating a meaningful error at that text.
fn after_debug_list(text: &str) -> Option<&str> {
    let raw = text.strip_prefix('[')?;
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in raw.char_indices() {
        if escaped {
            escaped = false;
        } else if quoted && c == '\\' {
            escaped = true;
        } else if c == '"' {
            quoted = !quoted;
        } else if c == ']' && !quoted {
            return Some(&raw[i + 1..]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{set_lang_no_persist, tests::LOCK};

    fn both_languages(check: impl Fn(Lang)) {
        let _guard = LOCK.lock();
        let previous = lang();
        for language in [Lang::En, Lang::Ja] {
            set_lang_no_persist(language);
            check(language);
        }
        set_lang_no_persist(previous);
    }

    #[test]
    fn known_templates_remove_implementation_words_and_preserve_causes() {
        both_languages(|language| {
            for &(raw, _, _) in EXACT {
                let shown = diagnostic(raw).unwrap();
                assert_ne!(shown, raw);
                for internal in [
                    "executor",
                    "preflight",
                    "identity",
                    "buffer",
                    "plan has",
                    "KAGI_LOG_DIR",
                    "collector",
                    "re-plan",
                    "stage 0",
                ] {
                    assert!(!shown.contains(internal), "{language:?}: {raw}: {shown}");
                }
            }
            let reason = "Permission denied: /tmp/feature_preflight-確認 (os error 13)";
            for &(prefix, _, _) in CAUSES {
                let shown = diagnostic(&format!("{prefix}{reason}")).unwrap();
                assert!(shown.ends_with(reason), "{prefix}: {shown}");
                assert!(!shown.starts_with(prefix), "{prefix}: {shown}");
            }
        });
    }

    #[test]
    fn dynamic_paths_branches_ids_and_raw_reasons_survive() {
        both_languages(|_| {
            let path = "feature_preflight-確認/it's a file.rs";
            let cause = "Permission denied (os error 13)";
            for raw in [
                format!("index.add_path({path}) failed: {cause}"),
                format!("index.remove_path({path}) failed: {cause}"),
                format!("apply-suggestion: failed to write '{path}': {cause}"),
                format!("discard verify: inspect '{path}': {cause}"),
                format!("verify {path} failed: {cause}"),
                format!("draft: read {path} before replacing it: {cause}"),
                format!("oplog: write failed for {path}: {cause}"),
                format!("rename /tmp/temporary -> {path} failed: {cause} (files before this point were already resolved; the index was not written, so the conflict is still recorded — re-resolve and retry)"),
            ] {
                let shown = diagnostic(&raw).unwrap();
                assert!(shown.contains(path), "{raw}: {shown}");
                assert!(shown.contains(cause), "{raw}: {shown}");
            }
            let shown = diagnostic("git replay saw 'feature_preflight' at abc123 but the plan read def456; please re-plan").unwrap();
            for value in ["feature_preflight", "abc123", "def456"] {
                assert!(shown.contains(value));
            }
            let shown = diagnostic("none of the 2 stash entries added during the push has HEAD abc123 as first parent with kagi's message").unwrap();
            assert!(shown.contains("abc123"));
            assert!(shown.contains('2'));
        });
    }

    #[test]
    fn nested_errors_keep_uncertainty_without_debug_observations() {
        both_languages(|language| {
            let raw = "stage=Deleting; verification=NotRun; termination_unknown=true; observations=[\"step 0 started: path \\\"a]; b\\\"\"]; git error: executor panic; do not retry";
            let shown = diagnostic(raw).unwrap();
            assert!(!shown.contains("observations="));
            assert!(!shown.contains("executor"));
            assert!(!shown.contains("step 0"));
            assert!(
                shown.contains(localized("Do not retry", "再実行しないでください")),
                "{language:?}: {shown}"
            );
            for raw in [
                "git error: git push origin feature_preflight wait failed: the child handle was taken from under the run (the local child could not be stopped)",
                "git push origin feature_preflight: output collection did not finish — a descendant still holds the pipes, so what was captured may be truncated",
                "stash executor panicked; the repository was not re-read",
            ] {
                let shown = diagnostic(raw).unwrap();
                assert!(!shown.contains("child"));
                assert!(!shown.contains("pipes"));
                assert!(!shown.contains("executor"));
                assert!(shown.contains(localized("Do not retry", "再実行しないでください")), "{shown}");
            }
        });
    }

    #[test]
    fn audited_dynamic_formats_are_recognized_in_both_languages() {
        both_languages(|_| {
            for raw in [
                "kagi:auto:terminal-12",
                "no resolution draft for src/確認.rs",
                "no resolution for src/確認.rs — re-plan before executing",
                "src/確認.rs bytes, blob, or mode differ after save",
                "src/確認.rs raw OID or mode differs after save",
                "apply-suggestion refused: 'src/確認.rs' changed since the suggestion was reviewed (stale range) — refusing to edit the wrong lines",
                "apply-suggestion aborted: backup failed for 'src/確認.rs': permission denied",
                "apply-suggestion verify: cannot re-read 'src/確認.rs': permission denied",
                "apply-suggestion verify failed: 'src/確認.rs' does not match the applied content",
                "apply-suggestion: range 2-4 out of bounds in 'src/確認.rs'",
                "discard verify: read link 'src/確認.rs': permission denied",
                "discard verify: non-UTF-8 link target 'src/確認.rs'",
                "discard verify failed: 2 target(s) not discarded: src/確認.rs, src/other.rs",
                "hunk staging verify failed: the index entry for 'src/確認.rs' did not change",
                "discard refused: plan has 2 blocker(s)",
                "apply-suggestion refused: plan has 2 blocker(s)",
                "remove-worktree refused: plan has 2 blocker(s)",
                "sync-to-remote refused at preflight: rebase is in progress",
                "op-revert refused: plan has blockers",
                "restore-to-point refused: the plan carries no ref restores",
                "op-revert refused: not an Operation Log restore",
                "restore-to-point refused: at preflight: plan has blockers",
                "restore-to-point refused: not a restore line: restore refs/heads/main abc123",
                "Repository state changed since planning. HEAD was Unborn { branch: \"main\" } at plan time but is now Detached { target: \"abc123\" }. Please re-plan before proceeding.",
                "git replay printed no update for refs/heads/feature",
                "plan carries no update for refs/heads/feature",
                "replay-onto refused at preflight: 'feature' is checked out in /tmp/確認 with uncommitted changes",
                "pre-remove command cannot run under the headless harness, so cleanup cannot be verified — aborting removal: npm stop",
                "draft: serialize failed: permission denied",
                "draft: temporary file: permission denied",
                "draft: write failed: permission denied",
                "draft: mkdir failed for /tmp/確認: permission denied",
                "draft: replace /tmp/確認: permission denied",
                "draft: delete failed for /tmp/確認: permission denied",
                "draft: move unreadable /tmp/確認 aside: permission denied",
                "draft: reserve /tmp/確認: permission denied",
                "draft: /tmp/確認 has no name",
                "auto-unlock-worktree refused: the lock was no longer this terminal's, and a newer lock appeared before it could be put back (permission denied); the moved lock is kept at /tmp/確認",
                "2 of the 3 stash entries added during the push are indistinguishable from kagi's (same HEAD and message)",
                "refs/kagi/backups/abc/0 does not hold the pre-sync tip",
                "refs/kagi/backups/abc/0 does not hold the work backup",
                "refs/kagi/backups/abc/0: permission denied",
                "git push origin feature timed out after 60s",
                "timed out after 60s",
                "unexpected git replay output: unsupported ref update verb \"create\": \"create refs/heads/feature abc123\"",
                "replay plan is not a ref update list: bad object id \"abc123\" in \"update refs/heads/feature abc123 def456\"",
            ] {
                let shown = diagnostic(raw).unwrap_or_else(|| panic!("unmapped: {raw}"));
                assert_ne!(shown, raw);
                for internal in ["preflight", "re-plan", "blocker(s)", "stale range", "refs/kagi/backups/", "headless harness", "ref update verb"] {
                    assert!(!shown.contains(internal), "{raw}: {shown}");
                }
            }
        });
    }

    #[test]
    fn unrecognized_text_and_user_data_are_not_sanitized() {
        both_languages(|_| {
            for raw in [
                "feature_preflight-identity",
                "a/path_with_under_scores",
                "fatal: bad revision 'plan has blockers'",
                "unknown API failed: keep this",
                "discard refused: plan has many blocker(s)",
                "stage=user text",
                "git fetch origin: raw Git error",
            ] {
                assert_eq!(diagnostic(raw), None, "{raw}");
            }
            let raw =
                "fatal: unable to access 'ssh://example/feature_preflight': Permission denied";
            let shown = diagnostic(&format!("git error: {raw}")).unwrap();
            assert!(shown.ends_with(raw));
        });
    }
}
