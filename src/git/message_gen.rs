//! Smart Commit Message generation — W14-SMART (T-COMMIT-015, ADR-0044)
//!
//! This backend half collects staged git data and optionally talks to local
//! Ollama. Pure generation, truncation, and JSON helpers live in `kagi-domain`
//! and are re-exported here for stable `kagi::git::message_gen::*` paths.

use std::path::Path;
use std::time::Duration;

use git2::{DiffOptions, Repository};
use kagi_domain::status::{ChangeKind, FileStatus};

use super::{resolve_head, Head};

pub use kagi_domain::message_gen::{
    clean_llm_message, clean_llm_message_multiline, file_summary, ollama_generate_request_body,
    parse_ollama_response, parse_ollama_tags, rule_based, truncate_with_summary, GenError,
    GenInput, Lang, MessageBackend, Style, DEFAULT_OLLAMA_HOST, DIFF_TRUNCATE_BYTES,
};

/// Global timeout for a single Ollama HTTP request. Local models (especially
/// larger ones) can take a while to load + generate even a short subject, so
/// this is generous; the spinner keeps the UI responsive during the wait.
const HTTP_TIMEOUT: Duration = Duration::from_secs(45);

// ──────────────────────────────────────────────────────────────────────────
// Offline switch
// ──────────────────────────────────────────────────────────────────────────

/// Whether all network access is disabled (`KAGI_OFFLINE=1`).
///
/// Headless / fixture runs set this so LLM behaviour is deterministic and the
/// staged diff never leaves the machine.
pub fn offline() -> bool {
    std::env::var("KAGI_OFFLINE").as_deref() == Ok("1")
}

// ──────────────────────────────────────────────────────────────────────────
// Staged diff collection (staged ONLY — never unstaged)
// ──────────────────────────────────────────────────────────────────────────

/// Collect the staged file set as [`FileStatus`] entries (index side only).
///
/// This is `git diff --cached --name-status` in spirit: it diffs the HEAD tree
/// against the index, so **only staged** changes are reported.  Unstaged /
/// untracked changes never appear.  Used both for the rule-based generator and
/// the file summary appended to a truncated diff.
pub fn collect_staged_files(repo: &Repository) -> Vec<FileStatus> {
    let head = resolve_head(repo);
    let old_tree = match head {
        Ok(Head::Unborn { .. }) | Err(_) => None,
        Ok(_) => repo
            .head()
            .ok()
            .and_then(|r| r.target())
            .and_then(|oid| repo.find_commit(oid).ok())
            .and_then(|c| c.tree().ok()),
    };

    let diff = match repo.diff_tree_to_index(old_tree.as_ref(), None, None) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    let mut out = Vec::new();
    for delta in diff.deltas() {
        use git2::Delta;
        let change = match delta.status() {
            Delta::Added | Delta::Untracked => ChangeKind::Added,
            Delta::Deleted => ChangeKind::Deleted,
            Delta::Renamed => {
                let from = delta
                    .old_file()
                    .path()
                    .map(Path::to_path_buf)
                    .unwrap_or_default();
                ChangeKind::Renamed { from }
            }
            Delta::Typechange => ChangeKind::TypeChange,
            _ => ChangeKind::Modified,
        };
        // Prefer the new path; fall back to the old path (deletes).
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(Path::to_path_buf)
            .unwrap_or_default();
        if !path.as_os_str().is_empty() {
            out.push(FileStatus { path, change });
        }
    }
    out
}

/// Collect the full **staged** diff as unified-diff text, truncated to
/// [`DIFF_TRUNCATE_BYTES`].
///
/// Diffs the HEAD tree against the index (`git diff --cached`).  Only staged
/// content is included — unstaged / untracked changes are never collected, per
/// ADR-0044.  When the diff exceeds the truncation budget, it is cut at the
/// nearest line boundary within the budget and a one-line `[... truncated ...]`
/// marker plus a file summary is appended so the model still sees *which* files
/// changed even if it can't see every line.
///
/// Always returns a `String` (possibly empty when nothing is staged); never
/// performs any network access.
pub fn collect_staged_diff(repo: &Repository) -> String {
    let head = resolve_head(repo);
    let old_tree = match head {
        Ok(Head::Unborn { .. }) | Err(_) => None,
        Ok(_) => repo
            .head()
            .ok()
            .and_then(|r| r.target())
            .and_then(|oid| repo.find_commit(oid).ok())
            .and_then(|c| c.tree().ok()),
    };

    let mut opts = DiffOptions::new();
    // Keep binary deltas out of the text body (they add noise / no value).
    opts.show_binary(false);

    let diff = match repo.diff_tree_to_index(old_tree.as_ref(), None, Some(&mut opts)) {
        Ok(d) => d,
        Err(_) => return String::new(),
    };

    let mut text = String::new();
    let _ = diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        // Re-prepend the origin marker that `print` strips for +/-/context.
        match line.origin() {
            '+' | '-' | ' ' => text.push(line.origin()),
            _ => {}
        }
        text.push_str(&String::from_utf8_lossy(line.content()));
        true
    });

    let files = collect_staged_files(repo);
    truncate_with_summary(&text, &files)
}

// ──────────────────────────────────────────────────────────────────────────
// Dispatch
// ──────────────────────────────────────────────────────────────────────────

/// Generate a commit message using `backend`.
///
/// * `RuleBased` is infallible — it always returns a non-empty `Ok(String)`.
/// * `Ollama` returns `Err(GenError)` on offline / network / parse failure, so
///   the caller can fall back quietly to [`rule_based`].
///
/// The `files` slice (staged file set) is needed by the rule-based generator and
/// the prompt's file summary; callers obtain it via [`collect_staged_files`].
pub fn generate_message(
    backend: &MessageBackend,
    input: &GenInput,
    files: &[FileStatus],
) -> Result<String, GenError> {
    match backend {
        MessageBackend::RuleBased => Ok(rule_based(input, files)),
        MessageBackend::Ollama { host, model } => {
            if offline() {
                return Err(GenError::Offline);
            }
            if files.is_empty() && input.diff.trim().is_empty() {
                return Err(GenError::NoStagedChanges);
            }
            let prompt = build_prompt(input, files);
            let raw = ollama_generate(host, model, &prompt)?;
            // Keep the body when the caller wants it (template mode); otherwise
            // collapse to the subject line.
            let cleaned = if input.want_body {
                clean_llm_message_multiline(&raw)
            } else {
                clean_llm_message(&raw)
            };
            if cleaned.is_empty() {
                Err(GenError::EmptyResponse)
            } else {
                Ok(cleaned)
            }
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Ollama prompt + HTTP
// ──────────────────────────────────────────────────────────────────────────

/// Build the LLM prompt: instruction (lang/style aware) + truncated diff +
/// file summary.
fn build_prompt(input: &GenInput, files: &[FileStatus]) -> String {
    // OpenCommit-inspired: a clear role, the Conventional Commits spec with the
    // allowed types, concrete rules, and one worked example. Local models adhere
    // far better to format + imperative mood with this structure than with a
    // one-line instruction (ADR-0090).
    let (format_rules, example) = match input.style {
        Style::ConventionalCommits => (
            "Format: <type>(<optional scope>): <subject>\n\
             - Allowed types: feat, fix, docs, style, refactor, perf, test, build, ci, chore, revert.\n\
             - Pick the single most appropriate type. The scope is optional and is a short noun (e.g. a module name).",
            "feat(auth): refresh the access token on a 401 response",
        ),
        Style::Plain => (
            "Format: a single concise subject line with no type prefix.",
            "Refresh the access token on a 401 response",
        ),
    };
    let lang_line = match input.lang {
        Lang::Ja => "Write it in Japanese. Keep the type and any code identifiers in English.",
        Lang::En => "Write it in English.",
    };
    let summary = file_summary(files);
    if input.want_body {
        format!(
            "You are an expert software engineer writing a high-quality git commit message.\n\
             Summarise the STAGED changes below into a commit message: a subject line, then a \
             blank line, then a short body.\n\n\
             Rules:\n\
             - {format_rules}\n\
             - Use the imperative mood: \"add\", not \"added\" or \"adds\".\n\
             - Subject: specific, under 72 characters, no trailing period.\n\
             - Body: 1–4 short bullet points (\"- …\") explaining WHAT changed and WHY. Be concise.\n\
             - Do not invent changes that are not in the diff.\n\
             - {lang_line}\n\
             - Output ONLY the commit message (subject, blank line, then the body). No quotes, no \
             code fences, no preamble, no explanation.\n\n\
             Example of the exact output format:\n\
             {example}\n\n\
             - explain the main change in one line\n\
             - note a secondary change if any\n\n\
             {summary}\n\
             --- staged diff ---\n{}\n--- end diff ---\n",
            input.diff
        )
    } else {
        format!(
            "You are an expert software engineer writing a high-quality git commit message.\n\
             Summarise the STAGED changes below into ONE commit message subject line.\n\n\
             Rules:\n\
             - {format_rules}\n\
             - Use the imperative mood: \"add\", not \"added\" or \"adds\".\n\
             - Say specifically WHAT changed; keep the subject under 72 characters.\n\
             - No trailing period. Do not invent changes that are not in the diff.\n\
             - {lang_line}\n\
             - Output ONLY the commit subject. No quotes, no code fences, no preamble, no explanation.\n\n\
             Example of the exact output format:\n\
             {example}\n\n\
             {summary}\n\
             --- staged diff ---\n{}\n--- end diff ---\n",
            input.diff
        )
    }
}

/// POST a generation request to a local Ollama server and return the raw
/// `response` string.
///
/// `host` is `host:port` (e.g. `localhost:11434`).  Uses ureq 3 with a global
/// timeout (same pattern as `avatar_fetch.rs`).  Returns `Err(GenError::Http)`
/// on transport / status failure and `Err(GenError::EmptyResponse)` when the
/// reply has no `response` field.
fn ollama_generate(host: &str, model: &str, prompt: &str) -> Result<String, GenError> {
    // First try with `think:false` so a *thinking* model answers the subject
    // directly instead of burning its token budget on reasoning (which returns
    // an empty `response`). A non-thinking model may reject the `think` field or
    // simply ignore it; on any failure or empty reply, retry once WITHOUT the
    // field so plain instruct models still work (ADR-0090).
    match ollama_generate_once(host, model, prompt, Some(false)) {
        Ok(msg) => Ok(msg),
        Err(e) => {
            // Only retry without `think` for a *quick rejection* (some plain
            // instruct models 400 on the field). On a timeout, don't retry — it
            // would just double the wait and re-enable the reasoning we're
            // avoiding.
            let is_timeout = matches!(&e, GenError::Http(m) if m.contains("timeout"));
            if is_timeout {
                Err(e)
            } else {
                ollama_generate_once(host, model, prompt, None)
            }
        }
    }
}

/// One `/api/generate` round-trip with a specific `think` setting.
fn ollama_generate_once(
    host: &str,
    model: &str,
    prompt: &str,
    think: Option<bool>,
) -> Result<String, GenError> {
    let url = format!("http://{}/api/generate", host);
    let body = ollama_generate_request_body(model, prompt, think);

    let mut resp = ureq::post(&url)
        .header("Content-Type", "application/json")
        .config()
        .timeout_global(Some(HTTP_TIMEOUT))
        .build()
        .send(body.as_bytes())
        .map_err(|e| GenError::Http(e.to_string()))?;

    if resp.status().as_u16() != 200 {
        return Err(GenError::Http(format!("status {}", resp.status().as_u16())));
    }

    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| GenError::Http(e.to_string()))?;

    parse_ollama_response(&text).ok_or(GenError::EmptyResponse)
}

// ──────────────────────────────────────────────────────────────────────────
// Ollama detection (reachability + model list)
// ──────────────────────────────────────────────────────────────────────────

/// Check whether an Ollama server is reachable at `host` (loopback).
///
/// Performs a single short GET to `/api/tags`.  Returns `true` only on a 200.
/// Honours `KAGI_OFFLINE=1` (always `false`).  This is *only* a reachability
/// probe — no diff is ever sent here.
pub fn ollama_available(host: &str) -> bool {
    if offline() {
        return false;
    }
    let url = format!("http://{}/api/tags", host);
    matches!(
        ureq::get(&url)
            .config()
            .timeout_global(Some(Duration::from_secs(3)))
            .build()
            .call(),
        Ok(resp) if resp.status().as_u16() == 200
    )
}

/// List installed models from `host`'s `/api/tags`.
///
/// Returns an empty `Vec` when offline, unreachable, or on parse failure.  Only
/// the model *names* are returned (no diff is sent).
pub fn ollama_list_models(host: &str) -> Vec<String> {
    if offline() {
        return Vec::new();
    }
    let url = format!("http://{}/api/tags", host);
    let resp = ureq::get(&url)
        .config()
        .timeout_global(Some(Duration::from_secs(3)))
        .build()
        .call();
    let Ok(mut resp) = resp else {
        return Vec::new();
    };
    if resp.status().as_u16() != 200 {
        return Vec::new();
    }
    match resp.body_mut().read_to_string() {
        Ok(text) => parse_ollama_tags(&text),
        Err(_) => Vec::new(),
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Unit tests (no real network — ureq paths are not exercised here)
// ──────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fs(path: &str, change: ChangeKind) -> FileStatus {
        FileStatus {
            path: PathBuf::from(path),
            change,
        }
    }

    fn input(lang: Lang, style: Style) -> GenInput {
        GenInput {
            diff: String::new(),
            lang,
            style,
            want_body: false,
        }
    }

    // ── rule_based: never empty ───────────────────────────────

    #[test]
    fn rule_based_never_empty_even_with_no_files() {
        let en = rule_based(&input(Lang::En, Style::Plain), &[]);
        assert!(!en.is_empty());
        let ja = rule_based(&input(Lang::Ja, Style::ConventionalCommits), &[]);
        assert!(!ja.is_empty());
    }

    // ── rule_based: single file verbs ─────────────────────────

    #[test]
    fn rule_based_single_add_en_plain() {
        let files = vec![fs("src/widget.rs", ChangeKind::Added)];
        let msg = rule_based(&input(Lang::En, Style::Plain), &files);
        assert_eq!(msg, "add widget.rs");
    }

    #[test]
    fn rule_based_single_delete_conventional() {
        let files = vec![fs("src/old.rs", ChangeKind::Deleted)];
        let msg = rule_based(&input(Lang::En, Style::ConventionalCommits), &files);
        // single code-file delete → chore: remove old.rs
        assert_eq!(msg, "chore: remove old.rs");
    }

    #[test]
    fn rule_based_single_modify_ja() {
        let files = vec![fs("src/main.rs", ChangeKind::Modified)];
        let msg = rule_based(&input(Lang::Ja, Style::Plain), &files);
        assert_eq!(msg, "main.rs を更新");
    }

    // ── rule_based: type inference by file kind ───────────────

    #[test]
    fn infer_type_tests_only_is_test() {
        let files = vec![
            fs("tests/foo_test.rs", ChangeKind::Modified),
            fs("tests/bar_test.rs", ChangeKind::Added),
        ];
        let msg = rule_based(&input(Lang::En, Style::ConventionalCommits), &files);
        assert!(msg.starts_with("test"), "got: {msg}");
    }

    #[test]
    fn infer_type_docs_only_is_docs() {
        let files = vec![
            fs("docs/guide.md", ChangeKind::Modified),
            fs("README.md", ChangeKind::Modified),
        ];
        let msg = rule_based(&input(Lang::En, Style::ConventionalCommits), &files);
        assert!(msg.starts_with("docs"), "got: {msg}");
    }

    #[test]
    fn infer_type_config_only_is_chore() {
        let files = vec![
            fs("config.toml", ChangeKind::Modified),
            fs(".gitignore", ChangeKind::Modified),
        ];
        let msg = rule_based(&input(Lang::En, Style::ConventionalCommits), &files);
        assert!(msg.starts_with("chore"), "got: {msg}");
    }

    #[test]
    fn infer_type_new_code_is_feat() {
        let files = vec![fs("src/feature.rs", ChangeKind::Added)];
        let msg = rule_based(&input(Lang::En, Style::ConventionalCommits), &files);
        assert!(msg.starts_with("feat"), "got: {msg}");
    }

    #[test]
    fn infer_type_modified_code_is_fix() {
        let files = vec![fs("src/lib.rs", ChangeKind::Modified)];
        let msg = rule_based(&input(Lang::En, Style::ConventionalCommits), &files);
        assert!(msg.starts_with("fix"), "got: {msg}");
    }

    // ── rule_based: multi-file scope ──────────────────────────

    #[test]
    fn rule_based_multi_file_with_common_scope() {
        let files = vec![
            fs("src/a.rs", ChangeKind::Modified),
            fs("src/b.rs", ChangeKind::Added),
        ];
        let msg = rule_based(&input(Lang::En, Style::ConventionalCommits), &files);
        // common dir "src" → scope; an add present → feat
        assert_eq!(msg, "feat(src): update 2 files");
    }

    #[test]
    fn rule_based_multi_file_no_common_scope() {
        let files = vec![
            fs("src/a.rs", ChangeKind::Modified),
            fs("lib/b.rs", ChangeKind::Modified),
        ];
        let msg = rule_based(&input(Lang::En, Style::ConventionalCommits), &files);
        // different top dirs → no scope; all modifications → fix
        assert_eq!(msg, "fix: update 2 files");
    }

    #[test]
    fn rule_based_multi_file_ja_plain() {
        let files = vec![
            fs("src/a.rs", ChangeKind::Modified),
            fs("src/b.rs", ChangeKind::Modified),
        ];
        let msg = rule_based(&input(Lang::Ja, Style::Plain), &files);
        assert_eq!(msg, "2 ファイルを更新");
    }

    // ── generate_message dispatch ─────────────────────────────

    #[test]
    fn generate_rule_based_is_infallible_and_nonempty() {
        let files = vec![fs("src/a.rs", ChangeKind::Modified)];
        let out = generate_message(
            &MessageBackend::RuleBased,
            &input(Lang::En, Style::ConventionalCommits),
            &files,
        );
        assert!(out.is_ok());
        assert!(!out.unwrap().is_empty());
    }

    #[test]
    fn generate_ollama_offline_is_err() {
        // Force offline so no network is touched; Ollama must Err → caller
        // falls back to rule_based.
        let prev = std::env::var("KAGI_OFFLINE").ok();
        std::env::set_var("KAGI_OFFLINE", "1");
        let files = vec![fs("src/a.rs", ChangeKind::Modified)];
        let out = generate_message(
            &MessageBackend::Ollama {
                host: "localhost:11434".to_string(),
                model: "gemma".to_string(),
            },
            &GenInput {
                diff: "diff --git a/x b/x".to_string(),
                lang: Lang::En,
                style: Style::ConventionalCommits,
                want_body: false,
            },
            &files,
        );
        assert_eq!(out, Err(GenError::Offline));
        // restore
        match prev {
            Some(v) => std::env::set_var("KAGI_OFFLINE", v),
            None => std::env::remove_var("KAGI_OFFLINE"),
        }
    }

    // ── truncation + summary ──────────────────────────────────

    #[test]
    fn truncate_short_diff_unchanged() {
        let diff = "diff --git a/x b/x\n+hello\n";
        let files = vec![fs("x", ChangeKind::Modified)];
        assert_eq!(truncate_with_summary(diff, &files), diff);
    }

    #[test]
    fn truncate_long_diff_adds_marker_and_summary() {
        // Build a diff well over the budget.
        let mut diff = String::new();
        for i in 0..2000 {
            diff.push_str(&format!("+line {}\n", i));
        }
        assert!(diff.len() > DIFF_TRUNCATE_BYTES);
        let files = vec![fs("src/big.rs", ChangeKind::Modified)];
        let out = truncate_with_summary(&diff, &files);
        assert!(out.len() < diff.len());
        assert!(out.contains("[... diff truncated for length ...]"));
        assert!(out.contains("Files: M src/big.rs"));
        // Truncation happened at a line boundary (ends with our marker block).
        assert!(out.ends_with("Files: M src/big.rs\n"));
    }

    #[test]
    fn file_summary_lists_change_letters() {
        let files = vec![
            fs("a.rs", ChangeKind::Added),
            fs("b.rs", ChangeKind::Modified),
            fs("c.rs", ChangeKind::Deleted),
        ];
        let s = file_summary(&files);
        assert!(s.contains("A a.rs"));
        assert!(s.contains("M b.rs"));
        assert!(s.contains("D c.rs"));
    }

    // ── ollama JSON parse ─────────────────────────────────────

    #[test]
    fn parse_ollama_response_basic() {
        let json = r#"{"model":"gemma","response":"feat: add login","done":true}"#;
        assert_eq!(
            parse_ollama_response(json).as_deref(),
            Some("feat: add login")
        );
    }

    #[test]
    fn parse_ollama_response_with_escapes() {
        let json = r#"{"response":"fix: handle \"quoted\" path\nand newline"}"#;
        assert_eq!(
            parse_ollama_response(json).as_deref(),
            Some("fix: handle \"quoted\" path\nand newline")
        );
    }

    #[test]
    fn parse_ollama_response_missing_or_empty() {
        assert_eq!(parse_ollama_response(r#"{"done":true}"#), None);
        assert_eq!(parse_ollama_response(r#"{"response":""}"#), None);
        assert_eq!(parse_ollama_response(r#"{"response":"   "}"#), None);
        assert_eq!(parse_ollama_response(""), None);
    }

    #[test]
    fn parse_ollama_tags_lists_models() {
        let json = r#"{
          "models": [
            { "name": "gemma:2b", "size": 1234 },
            { "name": "llama3:8b", "size": 5678 }
          ]
        }"#;
        let models = parse_ollama_tags(json);
        assert_eq!(
            models,
            vec!["gemma:2b".to_string(), "llama3:8b".to_string()]
        );
    }

    #[test]
    fn parse_ollama_tags_empty() {
        assert!(parse_ollama_tags(r#"{"models":[]}"#).is_empty());
        assert!(parse_ollama_tags("").is_empty());
    }

    // ── request body escaping ─────────────────────────────────

    #[test]
    fn request_body_escapes_quotes_and_newlines() {
        let body = ollama_generate_request_body("gemma", "say \"hi\"\nnow", Some(false));
        assert!(body.contains("\\\"hi\\\""));
        assert!(body.contains("\\n"));
        assert!(body.contains("\"stream\":false"));
        assert!(body.contains("\"model\":\"gemma\""));
    }

    // ── clean_llm_message ─────────────────────────────────────

    #[test]
    fn clean_strips_fences_and_quotes() {
        assert_eq!(clean_llm_message("```\nfeat: add x\n```"), "feat: add x");
        assert_eq!(clean_llm_message("\"feat: add x\""), "feat: add x");
        assert_eq!(clean_llm_message("feat: add x\n\nbody text"), "feat: add x");
        assert_eq!(clean_llm_message("   "), "");
    }

    // ── lang / style slugs ────────────────────────────────────

    #[test]
    fn lang_style_slug_roundtrip() {
        assert_eq!(Lang::from_slug(Lang::Ja.slug()), Lang::Ja);
        assert_eq!(Lang::from_slug(Lang::En.slug()), Lang::En);
        assert_eq!(Lang::from_slug("garbage"), Lang::En);
        assert_eq!(Style::from_slug(Style::Plain.slug()), Style::Plain);
        assert_eq!(
            Style::from_slug(Style::ConventionalCommits.slug()),
            Style::ConventionalCommits
        );
        assert_eq!(Style::from_slug("garbage"), Style::ConventionalCommits);
    }
}
