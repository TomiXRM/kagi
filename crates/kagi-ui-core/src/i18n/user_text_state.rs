//! Display adapters for historical receipt prose. Never rewrite stored summaries.
//! Match producer grammars, not words inside a user's branch, path or Git error.

use crate::i18n::{lang, Lang};

pub(super) fn state_text(text: &str) -> Option<String> {
    state_text_for(text, lang())
}

fn choose<'a>(language: Lang, en: &'a str, ja: &'a str) -> &'a str {
    match language {
        Lang::En => en,
        Lang::Ja => ja,
    }
}

fn conflict_operation(op: &str) -> Option<&str> {
    match op {
        "merge" | "rebase" | "cherry-pick" | "revert" | "bisect" | "stash" => Some(op),
        "other" => Some("Git"),
        _ => None,
    }
}

fn state_text_for(text: &str, language: Lang) -> Option<String> {
    let fixed = match text {
        "remote (read-only view)" => Some(("Remote repository", "remote repository")),
        "not inspected at admission" => Some((
            "Working tree status unavailable",
            "working tree の状態を確認できません",
        )),
        "unobserved" => Some(("State unavailable", "状態を確認できません")),
        "staged (stage 0)" => Some(("Staged", "stage 済み")),
        "choice=keep-directory" => Some(("Keep folder", "フォルダーを残す")),
        "choice=keep-file" => Some(("Keep file", "ファイルを残す")),
        _ => None,
    };
    if let Some((en, ja)) = fixed {
        return Some(choose(language, en, ja).to_owned());
    }
    if let Some(rest) = text.strip_prefix("session=") {
        let (op, path) = rest.split_once(" file=")?;
        let op = conflict_operation(op)?;
        return Some(match language {
            Lang::En => format!("Resolve {op} conflicts in {path}"),
            Lang::Ja => format!("{path} の {op} の conflict を解決"),
        });
    }
    if let Some(op) = text.strip_prefix("op=").and_then(conflict_operation) {
        return Some(match language {
            Lang::En => format!("Resolving {op} conflicts"),
            Lang::Ja => format!("{op} の conflict を解決中"),
        });
    }
    if let Some(revision) = text.strip_prefix("conflict revision ") {
        if !revision.is_empty()
            && revision.len() <= 12
            && revision.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Some(
                choose(
                    language,
                    "Conflict resolution was not saved.",
                    "conflict の解決内容は保存されていません。",
                )
                .to_owned(),
            );
        }
    }
    for (action, en, ja) in [
        ("conflict-save", "Save resolution", "解決内容を保存"),
        ("conflict-dir-file:keep-file", "Keep file", "ファイルを残す"),
        (
            "conflict-dir-file:keep-directory",
            "Keep folder",
            "フォルダーを残す",
        ),
        (
            "conflict-abort",
            "Abort conflict resolution",
            "conflict の解決を中止",
        ),
    ] {
        if text == action {
            return Some(choose(language, en, ja).to_owned());
        }
        if let Some(path) = text.strip_prefix(action).and_then(|s| s.strip_prefix(' ')) {
            return Some(format!("{}: {path}", choose(language, en, ja)));
        }
    }
    if let Some(rest) = text.strip_prefix("hunks=[") {
        let (_, hash) = rest.split_once("] before=")?;
        if hash.is_empty() || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        return Some(
            choose(
                language,
                "Resolution not saved.",
                "解決内容は保存されていません。",
            )
            .to_owned(),
        );
    }
    if let Some(rest) = text.strip_prefix("staged (stage 0) before=") {
        let (before, after) = rest.split_once(" after=")?;
        if before.is_empty()
            || after.is_empty()
            || !before
                .bytes()
                .chain(after.bytes())
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return None;
        }
        return Some(
            choose(
                language,
                "Resolution saved and staged.",
                "解決内容を保存して stage しました。",
            )
            .to_owned(),
        );
    }
    if let Some(path) = text.strip_prefix("dir-file conflict ") {
        return Some(match language {
            Lang::En => format!("File/folder conflict: {path}"),
            Lang::Ja => format!("ファイルとフォルダーの conflict: {path}"),
        });
    }
    for (prefix, en, ja) in [
        (
            "kept keep-directory side of ",
            "Kept folder",
            "フォルダーを残しました",
        ),
        (
            "kept keep-file side of ",
            "Kept file",
            "ファイルを残しました",
        ),
    ] {
        if let Some(path) = text.strip_prefix(prefix) {
            return Some(format!("{}: {path}", choose(language, en, ja)));
        }
    }
    if let Some(head) = head_debug(text, language) {
        return Some(head);
    }
    if let Some(state) = removal_backup(text, language) {
        return Some(state);
    }
    if let Some(state) = state_suffix(text, language) {
        return Some(state);
    }
    if let Some(state) = recovery_state(text, language) {
        return Some(state);
    }
    if let Some(state) = reconciliation_state(text, language) {
        return Some(state);
    }
    // The count grammar is also used by the plan's individual status chips.
    if language == Lang::Ja
        && !text.is_empty()
        && text.split(", ").all(|part| {
            part.split_once(' ').is_some_and(|(count, kind)| {
                count.parse::<usize>().is_ok()
                    && matches!(kind, "staged" | "modified" | "untracked" | "conflicted")
            })
        })
    {
        return Some(
            text.split(", ")
                .filter_map(|part| {
                    let (count, kind) = part.split_once(' ')?;
                    Some(match language {
                        Lang::En => part.to_owned(),
                        Lang::Ja => match kind {
                            "staged" => format!("stage 済み {count} 件"),
                            "modified" => format!("変更 {count} 件"),
                            "untracked" => format!("untracked {count} 件"),
                            _ => format!("conflict {count} 件"),
                        },
                    })
                })
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    None
}

/// Split only outside Rust Debug strings and nested containers. In particular,
/// a path containing `, untracked: [` must not become a field delimiter.
fn debug_fields(text: &str) -> Option<Vec<&str>> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, ch) in text.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        match ch {
            '"' => quoted = true,
            '[' | '{' | '(' => depth += 1,
            ']' | '}' | ')' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                fields.push(text[start..offset].trim());
                start = offset + 1;
            }
            _ => {}
        }
    }
    if quoted || depth != 0 {
        return None;
    }
    if start < text.len() {
        fields.push(text[start..].trim());
    }
    Some(fields)
}

fn debug_string(text: &str) -> Option<String> {
    // Rust's quoted Debug paths can contain Unicode and hexadecimal escapes
    // that JSON does not accept. Decode only a complete quoted string.
    let mut chars = text.strip_prefix('"')?.strip_suffix('"')?.chars();
    let mut out = String::new();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        out.push(match chars.next()? {
            '\\' => '\\',
            '"' => '"',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '0' => '\0',
            'x' => {
                let high = chars.next()?.to_digit(16)?;
                let low = chars.next()?.to_digit(16)?;
                char::from_u32(high * 16 + low)?
            }
            'u' => {
                if chars.next()? != '{' {
                    return None;
                }
                let mut value = 0u32;
                loop {
                    let digit = chars.next()?;
                    if digit == '}' {
                        break;
                    }
                    value = value.checked_mul(16)?.checked_add(digit.to_digit(16)?)?;
                }
                char::from_u32(value)?
            }
            _ => return None,
        });
    }
    Some(out)
}

fn debug_file(text: &str, language: Lang) -> Option<String> {
    let fields = debug_fields(text.strip_prefix("FileStatus { ")?.strip_suffix(" }")?)?;
    let [path, change] = fields.as_slice() else {
        return None;
    };
    let path = debug_string(path.strip_prefix("path: ")?)?;
    let change = change.strip_prefix("change: ")?;
    let (en, ja) = match change {
        "Added" => ("added", "追加"),
        "Modified" => ("modified", "変更"),
        "Deleted" => ("deleted", "削除"),
        "TypeChange" => ("file type changed", "ファイル種別の変更"),
        _ => {
            let from = debug_string(
                change
                    .strip_prefix("Renamed { from: ")?
                    .strip_suffix(" }")?,
            )?;
            return Some(format!("{from} → {path}"));
        }
    };
    Some(format!("{path} ({})", choose(language, en, ja)))
}

fn reconciliation_state(text: &str, language: Lang) -> Option<String> {
    let rest = text.strip_prefix("head=")?;
    if let Some((head, rest)) = rest.rsplit_once("; registered=") {
        let (registered, exists) = rest.split_once("; target_exists=")?;
        let registered = match registered {
            "true" => choose(language, "registered", "登録済み"),
            "false" => choose(language, "not registered", "未登録"),
            _ => return None,
        };
        let exists = match exists {
            "true" => choose(language, "present", "あり"),
            "false" => choose(language, "missing", "なし"),
            _ => return None,
        };
        let head = head_debug(head, language)?;
        return Some(match language {
            Lang::En => {
                format!("Current branch: {head}. Worktree: {registered}. Folder: {exists}.")
            }
            Lang::Ja => {
                format!("現在の branch: {head}。worktree: {registered}。フォルダー: {exists}。")
            }
        });
    }
    let (rest, stashes) = rest.rsplit_once("; stashes=")?;
    let (head, status) = rest.split_once("; status=WorkingTreeStatus { ")?;
    let status = debug_fields(status.strip_suffix(" }")?)?;
    let [staged, unstaged, untracked, conflicted] = status.as_slice() else {
        return None;
    };
    let mut out = match language {
        Lang::En => format!("Current branch: {head}"),
        Lang::Ja => format!("現在の branch: {head}"),
    };
    for (field, prefix, en, ja, structured) in [
        (staged, "staged: [", "Staged", "stage 済み", true),
        (unstaged, "unstaged: [", "Unstaged", "stage 前", true),
        (untracked, "untracked: [", "Untracked", "untracked", false),
        (conflicted, "conflicted: [", "Conflicted", "conflict", false),
    ] {
        let fields = debug_fields(field.strip_prefix(prefix)?.strip_suffix(']')?)?;
        let paths = fields
            .iter()
            .map(|field| {
                if structured {
                    debug_file(field, language)
                } else {
                    debug_string(field)
                }
            })
            .collect::<Option<Vec<_>>>()?;
        out.push_str(&format!(
            "\n{} ({}): {}",
            choose(language, en, ja),
            paths.len(),
            paths.join(", ")
        ));
    }
    let stashes = debug_fields(stashes.strip_prefix('[')?.strip_suffix(']')?)?
        .into_iter()
        .map(debug_string)
        .collect::<Option<Vec<_>>>()?;
    out.push_str(&format!(
        "\nstash ({}): {}",
        stashes.len(),
        stashes.join(", ")
    ));
    Some(out)
}

fn head_debug(text: &str, language: Lang) -> Option<String> {
    // Decode only the complete, known Head grammar, preserving the ref and OID.
    if let Some(rest) = text
        .strip_prefix("Attached { branch: ")
        .and_then(|s| s.strip_suffix(" }"))
    {
        let (branch, target) = rest.rsplit_once(", target: ")?;
        let branch = debug_string(branch)?;
        let target = debug_string(target)?;
        return Some(format!("{branch} @ {target}"));
    }
    if let Some(target) = text
        .strip_prefix("Detached { target: ")
        .and_then(|s| s.strip_suffix(" }"))
    {
        let target = debug_string(target)?;
        return Some(format!("detached HEAD @ {target}"));
    }
    if let Some(branch) = text
        .strip_prefix("Unborn { branch: ")
        .and_then(|s| s.strip_suffix(" }"))
    {
        let branch = debug_string(branch)?;
        return Some(match language {
            Lang::En => format!("{branch} (no commits yet)"),
            Lang::Ja => format!("{branch}（まだ commit がありません）"),
        });
    }
    None
}

fn removal_backup(text: &str, language: Lang) -> Option<String> {
    let (stage, rest) = text.strip_prefix("stage=")?.split_once("; backup: ")?;
    let (files, tip) = rest.rsplit_once("; branch_tip=")?;
    let (en, ja) = match stage {
        "NotStarted" => (
            "Worktree removal not started",
            "worktree の削除は開始していません",
        ),
        "BackupCaptured" => ("Recovery backup saved", "復元用バックアップを保存しました"),
        "DeletionStarted" => (
            "Worktree removal started; completion not confirmed",
            "worktree の削除を開始しました。完了は未確認です",
        ),
        "AdminPruned" => (
            "Worktree registration removed",
            "worktree の登録を削除しました",
        ),
        "BranchDeleted" => (
            "Worktree branch deleted",
            "worktree の branch を削除しました",
        ),
        "Done" => ("Worktree removed", "worktree を削除しました"),
        _ if stage
            .strip_prefix("PreRemove { step: ")
            .and_then(|s| s.strip_suffix(" }"))
            .is_some_and(|s| s.parse::<usize>().is_ok()) =>
        {
            ("Preparing worktree removal", "worktree の削除を準備中")
        }
        _ => return None,
    };
    let mut out = choose(language, en, ja).to_owned();
    if !files.is_empty() {
        out.push_str(&format!(
            "\n{}: {files}",
            choose(
                language,
                "Recovery backups (file = content OID)",
                "復元用バックアップ（ファイル = 内容の OID）"
            )
        ));
    }
    if tip != "unavailable" {
        out.push_str(&format!(
            "\n{}: {tip}",
            choose(language, "Previous branch tip", "削除前の branch の先端")
        ));
    }
    Some(out)
}

fn nested_state(text: &str, language: Lang) -> String {
    state_text_for(text, language).unwrap_or_else(|| super::diagnostic(text).into_owned())
}

fn state_suffix(text: &str, language: Lang) -> Option<String> {
    if let Some((before, rest)) = text.rsplit_once("; stash oid=") {
        let (oid, rest) = rest.split_once("; applied=")?;
        let (applied, snapshot) = rest.split_once("; snapshot=")?;
        let (en, ja) = match applied {
            "true" => ("Changes restored", "変更を復元しました"),
            "false" => ("Changes not restored", "変更は復元していません"),
            _ => return None,
        };
        let mut out = format!(
            "{}. {}",
            nested_state(before, language),
            choose(language, en, ja)
        );
        if oid != "unavailable" {
            out.push_str(&format!(". stash: {oid}"));
        }
        if snapshot != "none" {
            out.push_str(&format!(
                "\n{}: {snapshot}",
                choose(language, "Recovery snapshot", "復元用スナップショット")
            ));
        }
        return Some(out);
    }
    if let Some((before, target)) = text
        .strip_suffix("; checkout_changed=true")
        .and_then(|s| s.rsplit_once("; checkout_target="))
    {
        return Some(match language {
            Lang::En => format!(
                "{}. Some files were updated toward {target} before pull stopped.",
                nested_state(before, language)
            ),
            Lang::Ja => format!(
                "{}。pull が停止する前に、一部のファイルを {target} に更新しました。",
                nested_state(before, language)
            ),
        });
    }
    if let Some((before, error)) = text.split_once("; index observation failed: ") {
        let error = super::diagnostic(error);
        return Some(match language {
            Lang::En => format!(
                "{}. Could not check staged changes: {error}",
                nested_state(before, language)
            ),
            Lang::Ja => format!(
                "{}。stage 済みの変更を確認できませんでした: {error}",
                nested_state(before, language)
            ),
        });
    }
    None
}

fn recovery_commands(text: &str, language: Lang) -> Option<String> {
    let mut out = String::new();
    for part in text.split("; ") {
        let (label, command) = if let Some(command) = part.strip_prefix("restore tip: ") {
            (choose(language, "Restore branch", "branch を復元"), command)
        } else if let Some(command) = part.strip_prefix("restore work: ") {
            (choose(language, "Restore files", "ファイルを復元"), command)
        } else if let Some(command) = part.strip_prefix("restore: ") {
            (choose(language, "Restore", "復元"), command)
        } else {
            return None;
        };
        if !command.starts_with("git ") {
            return None;
        }
        out.push_str(&format!("\n{label}: {command}"));
    }
    Some(out)
}

fn recovery_state(text: &str, language: Lang) -> Option<String> {
    if let Some(rest) = text.strip_prefix("branch '") {
        if let Some((branch, rest)) = rest.split_once("' deleted (tip ") {
            let (tip, commands) = rest.split_once("); ")?;
            let recovery = recovery_commands(commands, language)?;
            return Some(match language {
                Lang::En => format!("Branch '{branch}' deleted. Previous tip: {tip}.{recovery}"),
                Lang::Ja => {
                    format!("branch '{branch}' を削除しました。削除前の先端: {tip}。{recovery}")
                }
            });
        }
        if let Some((branch, rest)) =
            rest.split_once("' reflog removed; deletion not verified (tip ")
        {
            let (tip, commands) = rest.split_once("); ")?;
            let recovery = recovery_commands(commands, language)?;
            return Some(match language {
                Lang::En => format!("Could not confirm deletion of branch '{branch}'. Previous tip: {tip}. Check the branch list.{recovery}"),
                Lang::Ja => format!("branch '{branch}' の削除を確認できませんでした。削除前の先端: {tip}。branch 一覧を確認してください。{recovery}"),
            });
        }
    }
    if let Some(rest) = text.strip_prefix("replayed '") {
        let (branch, rest) = rest.split_once("' ")?;
        let (range, rest) = rest.split_once(" (")?;
        let (count, commands) = rest.split_once(" ref(s) updated); ")?;
        count.parse::<usize>().ok()?;
        let (from, to) = range.split_once('→')?;
        let recovery = recovery_commands(commands, language)?;
        return Some(match language {
            Lang::En => format!("Replayed '{branch}': {from} → {to}. Updated {count} refs.{recovery}"),
            Lang::Ja => format!("'{branch}' を replay しました: {from} → {to}。ref を {count} 件更新しました。{recovery}"),
        });
    }
    if let Some(rest) = text.strip_prefix("synced '") {
        let (branch, rest) = rest.split_once("' ")?;
        let (range, rest) = rest.split_once("; ")?;
        let (from, to) = range.split_once('→')?;
        let (removed, commands) = match rest.split_once(" untracked removed; ") {
            Some((count, commands)) => (Some(count.parse::<usize>().ok()?), commands),
            None => (None, rest),
        };
        let recovery = recovery_commands(commands, language)?;
        let mut out = match language {
            Lang::En => format!("Synced '{branch}': {from} → {to}."),
            Lang::Ja => format!("'{branch}' を同期しました: {from} → {to}。"),
        };
        if let Some(count) = removed {
            out.push_str(&match language {
                Lang::En => format!(" Removed {count} untracked files."),
                Lang::Ja => format!(" untracked ファイルを {count} 件削除しました。"),
            });
        }
        out.push_str(&recovery);
        return Some(out);
    }
    if let Some(rest) = text.strip_prefix("verify failed: ") {
        let (error, commands) = rest.rsplit_once("; restore tip: ")?;
        let recovery = recovery_commands(&format!("restore tip: {commands}"), language)?;
        let error = super::diagnostic(error);
        return Some(match language {
            Lang::En => format!("Could not confirm the sync result: {error}.{recovery}"),
            Lang::Ja => format!("同期の結果を確認できませんでした: {error}。{recovery}"),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audited_state_formats_have_both_languages() {
        for raw in [
            "remote (read-only view)", "not inspected at admission", "unobserved",
            "session=merge file=dir/session=foo_file.txt", "op=rebase",
            "hunks=[0:current,1:incoming] before=a1b2", "conflict revision a1b2c3d4e5f6",
            "session=stash file=my_file.rs", "op=other",
            "conflict-dir-file:keep-directory my_file", "conflict-save my_file",
            "verify failed: raw Git reason; restore tip: git update-ref refs/heads/my_branch refs/kagi/backups/id/0",
            "choice=keep-file", "choice=keep-directory",
            "staged (stage 0) before=a1b2 after=c3d4", "staged (stage 0)",
            "dir-file conflict a/b", "kept keep-file side of a/b",
            "stage=Done; backup: a/b=abcdef; branch_tip=012345",
            "clean; stash oid=abc; applied=true; snapshot=save-123",
            "clean; checkout_target=abc; checkout_changed=true",
            "clean; index observation failed: raw Git reason",
            "branch 'my_branch' deleted (tip abc); restore: git branch my_branch refs/kagi/backups/id/0",
            "replayed 'my_branch' abc→def (2 ref(s) updated); restore: git update-ref refs/heads/my_branch refs/kagi/backups/id/0",
            "synced 'my_branch' abc→def; 2 untracked removed; restore tip: git update-ref refs/heads/my_branch refs/kagi/backups/id/0; restore work: git stash apply --index refs/kagi/backups/id/1",
        ] {
            for language in [Lang::En, Lang::Ja] {
                let shown = state_text_for(raw, language).expect(raw);
                assert_ne!(shown, raw);
                for internal in ["session=", "hunks=[", "stage=", "stage 0", "checkout_target=", "applied=", "branch_tip="] {
                    assert!(!shown.contains(internal) || shown.contains("dir/session=foo_file.txt"), "{shown}");
                }
            }
        }
    }

    #[test]
    fn user_data_and_recovery_commands_remain_intact() {
        for language in [Lang::En, Lang::Ja] {
            let path = "my_branch/session=keep-file_before=123";
            let shown = state_text_for(&format!("session=merge file={path}"), language).unwrap();
            assert!(shown.contains(path));
            let command = "git branch my_branch refs/kagi/backups/id/0";
            let raw = format!("branch 'my_branch' deleted (tip abc123); restore: {command}");
            let shown = state_text_for(&raw, language).unwrap();
            assert!(shown.contains(command));
            assert!(shown.contains("abc123"));
            assert!(shown.contains("\n"));
            assert_eq!(state_text_for("my_branch-stage=Done", language), None);
            assert_eq!(
                state_text_for("session=custom file=user_path", language),
                None
            );
            assert_eq!(
                state_text_for("unknown Git reason_with_underscores", language),
                None
            );
        }
    }

    #[test]
    fn head_debug_preserves_quoted_branch_and_oid() {
        let raw = r#"Attached { branch: "a\"b", target: "abc123" }"#;
        assert_eq!(
            state_text_for(raw, Lang::Ja).as_deref(),
            Some("a\"b @ abc123")
        );
        assert_eq!(
            state_text_for(r#"Detached { target: "abc123" }"#, Lang::En).as_deref(),
            Some("detached HEAD @ abc123")
        );
    }

    #[test]
    fn reconciliation_preserves_paths_oids_and_does_not_expose_debug_fields() {
        let stash = r#"head=branch: main; status=WorkingTreeStatus { staged: [FileStatus { path: "my_file, untracked: [.txt", change: Modified }], unstaged: [], untracked: ["a\nb"], conflicted: [] }; stashes=["abc123"]"#;
        let removal = r#"head=Attached { branch: "my_branch", target: "abc123" }; registered=false; target_exists=true"#;
        for language in [Lang::En, Lang::Ja] {
            let shown = state_text_for(stash, language).unwrap();
            assert!(shown.contains("my_file, untracked: [.txt"));
            assert!(shown.contains("a\nb"));
            assert!(shown.contains("abc123"));
            assert!(!shown.contains("WorkingTreeStatus"));
            assert!(!shown.contains("FileStatus"));
            let shown = state_text_for(removal, language).unwrap();
            assert!(shown.contains("my_branch @ abc123"));
            assert!(!shown.contains("target_exists"));
        }
    }
}
